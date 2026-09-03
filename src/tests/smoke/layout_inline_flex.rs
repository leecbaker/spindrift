use super::*;

fn first_visible_glyph_x(line: &crate::document::paint::text::RenderedLine) -> f32 {
    for run in &line.runs {
        let mut pen_x = line.x() + run.x_offset;
        if let Some(glyphs) = &run.glyphs {
            for glyph in glyphs {
                if !glyph.unicode.chars().all(char::is_whitespace) {
                    return pen_x + glyph.x_offset;
                }
                pen_x += glyph.x_advance;
            }
        }
    }
    line.x()
}

async fn render_text_box_trim_case(
    target_extra: &str,
    child_extra: &str,
    body: &str,
) -> spindrift::Document {
    Html::from_string(format!(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 600px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 50px }}
  .target {{
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
  .child {{ {child_extra} }}
</style>
<div class="spacer"></div>
<div class="target">{body}</div>
<div class="spacer"></div>"#
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

fn bottom_lightgray_spacer_y(document: &spindrift::Document) -> f32 {
    document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(211, 211, 211)))
        .map(|rect| rect.y())
        .min_by(f32::total_cmp)
        .expect("bottom lightgray spacer should paint")
}

fn rendered_text_lines(document: &spindrift::Document) -> Vec<String> {
    document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

async fn render_inline_box_text_box_trim_case(span_extra: &str, body: &str) -> spindrift::Document {
    Html::from_string(format!(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 240px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
  }}
  .trimmed {{
    background: rgb(10, 20, 30);
    text-box-edge: cap alphabetic;
    {span_extra}
  }}
</style>
<div class="target">{body}</div>"#
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

async fn render_inline_box_text_box_trim_layout_case(span_extra: &str) -> spindrift::Document {
    Html::from_string(format!(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 240px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 50px;
    line-height: 1;
    font-family: sans-serif;
  }}
  .trimmed {{
    line-height: 2;
    text-box-edge: cap alphabetic;
    {span_extra}
  }}
  .spacer {{ background: lightgray; block-size: 20px }}
</style>
<div class="target">A<span class="trimmed">B</span>C</div>
<div class="spacer"></div>"#
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

async fn render_vertical_inline_box_text_box_trim_case(
    span_extra: &str,
    body: &str,
) -> spindrift::Document {
    Html::from_string(format!(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 240px 240px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    writing-mode: vertical-rl;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
  }}
  .trimmed {{
    background: rgb(10, 20, 30);
    text-box-edge: cap alphabetic;
    {span_extra}
  }}
</style>
<div class="target">{body}</div>"#
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

fn inline_trim_background_rect(document: &spindrift::Document) -> (f32, f32, f32, f32) {
    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .expect("inline background should paint");
    (rect.x(), rect.y(), rect.width(), rect.height())
}

fn inline_trim_background_rects(document: &spindrift::Document) -> Vec<(f32, f32, f32, f32)> {
    document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect()
}

fn inline_trim_decoration_rect(document: &spindrift::Document) -> (f32, f32, f32, f32) {
    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("inline decoration should paint");
    (rect.x(), rect.y(), rect.width(), rect.height())
}

async fn render_inline_decoration_break_case(
    box_decoration_break: &str,
    content: &str,
) -> spindrift::Document {
    Html::from_string(format!(
        r#"<!DOCTYPE html>
<style>
  @page {{ size: 220px 160px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    width: 88px;
    color: rgb(255, 0, 0);
    font: 20px/24px sans-serif;
  }}
  .target::first-line {{ color: rgb(0, 128, 0) }}
  .decorated {{
    background: currentcolor;
    border-left: 4px solid currentcolor;
    border-right: 4px solid currentcolor;
    padding-left: 3px;
    padding-right: 3px;
    box-decoration-break: {box_decoration_break};
  }}
</style>
<div class="target"><span class="decorated">{content}</span></div>"#,
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap()
}

fn inline_decoration_rects(
    document: &spindrift::Document,
    color: CssColor,
) -> Vec<(f32, f32, f32, f32)> {
    document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(color))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect()
}

#[tokio::test]
async fn inline_box_decoration_clone_repeats_edges_across_forced_and_soft_breaks() {
    let green = CssColor::new(0, 128, 0);
    let red = CssColor::new(255, 0, 0);
    for content in ["A<br>B", "A A A A A"] {
        let cloned = render_inline_decoration_break_case("clone", content).await;
        let sliced = render_inline_decoration_break_case("slice", content).await;

        assert!(
            rendered_text_lines(&cloned).len() >= 2,
            "fixture must select more than one line: {:?}",
            rendered_text_lines(&cloned)
        );
        assert_eq!(
            rendered_text_lines(&cloned),
            rendered_text_lines(&sliced),
            "box-decoration-break must not change text selection"
        );

        let red_line = cloned.pages[0]
            .lines()
            .iter()
            .filter(|line| !line.text.trim().is_empty())
            .nth(1)
            .expect("second formatted line should paint");
        let red_text_left = first_visible_glyph_x(red_line);
        let continuation_has_left_border = |document: &spindrift::Document| {
            inline_decoration_rects(document, red)
                .iter()
                .any(|(x, _, width, _)| {
                    // CSS px → PDF pt turns the authored 4px border into 3pt.
                    // A sliced fragment still paints its background from the
                    // content edge, so test the distinct border primitive rather
                    // than treating the background's left edge as a border edge.
                    *x < red_text_left - 0.1 && *width <= 3.1
                })
        };

        assert!(
            continuation_has_left_border(&cloned),
            "clone continuation must paint a fragment-local inline start border"
        );
        assert!(
            !continuation_has_left_border(&sliced),
            "slice continuation must not gain an inline start border"
        );
        assert!(
            !inline_decoration_rects(&cloned, green).is_empty(),
            "first-line currentcolor must resolve cloned source decoration"
        );
    }
}

#[tokio::test]
async fn text_box_trim_end_shortens_last_line_after_block_in_inline() {
    let body = "<span><div>A<br>B</div></span>C";
    let untrimmed = render_text_box_trim_case("", "", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "", body).await;

    assert_eq!(rendered_text_lines(&trimmed), ["A", "B", "C"]);
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-end should move following block up by one half-leading: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_reaches_ordered_mixed_trailing_inline_run() {
    let body = "<div class=\"child\">A</div>C";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "margin: 0", body).await;

    assert_eq!(rendered_text_lines(&trimmed), ["A", "C"]);
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "ordered mixed-flow trim-end should apply to the trailing inline run: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_shortens_direct_inline_last_line() {
    let body = "A<br>B<br>C";
    let untrimmed = render_text_box_trim_case("", "", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "", body).await;

    assert_eq!(rendered_text_lines(&trimmed), ["A", "B", "C"]);
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "direct trim-end should shorten the block by one half-leading: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_does_not_interfere_with_positive_clearance() {
    let render = |target_extra: &str, float_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 400px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 100px }}
  .target, .float {{
    font: 50px/1 sans-serif;
  }}
  .target {{
    {target_extra}
  }}
  .float {{
    float: left;
    width: 100px;
    background: yellow;
    {float_extra}
  }}
  .clear {{ clear: both }}
</style>
<div class="float">F<br>F</div>
<div class="target">ApEx<br class="clear"></div>
<div class="spacer"></div>"#
        ))
    };
    let trimmed = render("text-box-trim: trim-end; text-box-edge: text;", "")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let reference = render("", "height: 100px;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&reference);
    assert!(
        delta.abs() < 0.5,
        "text-box-trim should preserve positive clearance before the spacer: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_start_reaches_ordered_mixed_leading_inline_run() {
    let body = "A<div class=\"child\">B</div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-start;", "margin: 0", body).await;

    assert_eq!(rendered_text_lines(&trimmed), ["A", "B"]);
    let untrimmed_first_y = untrimmed.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "A")
        .expect("untrimmed first line should render")
        .y();
    let trimmed_first_y = trimmed.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "A")
        .expect("trimmed first line should render")
        .y();
    let baseline_delta = trimmed_first_y - untrimmed_first_y;
    assert!(
        (baseline_delta - 18.75).abs() < 0.5,
        "ordered mixed-flow trim-start should apply to the leading inline run: delta={baseline_delta}"
    );
}

#[tokio::test]
async fn text_box_trim_start_moves_first_line_into_trimmed_leading() {
    let body = "A<br>B";
    let untrimmed = render_text_box_trim_case("", "", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-start;", "", body).await;

    let untrimmed_first_y = untrimmed.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "A")
        .expect("untrimmed first line should render")
        .y();
    let trimmed_first_y = trimmed.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "A")
        .expect("trimmed first line should render")
        .y();
    let baseline_delta = trimmed_first_y - untrimmed_first_y;
    assert!(
        (baseline_delta - 18.75).abs() < 0.5,
        "trim-start should move first-line paint into removed leading: delta={baseline_delta}"
    );
}

#[tokio::test]
async fn text_box_trim_does_not_cross_child_padding() {
    let body = "<div class=\"child\">C</div>";
    let untrimmed = render_text_box_trim_case("", "padding-bottom: 1px", body).await;
    let trimmed =
        render_text_box_trim_case("text-box-trim: trim-end;", "padding-bottom: 1px", body).await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not propagate through child block-end padding: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_start_does_not_skip_padded_first_child() {
    let body = "<div style=\"padding-top: 1px\">A</div><div>B</div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-start;", "margin: 0", body).await;

    let line_y = |document: &spindrift::Document, text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .expect("line should render")
            .y()
    };
    let delta = line_y(&trimmed, "B") - line_y(&untrimmed, "B");
    assert!(
        delta.abs() < 0.5,
        "trim-start should not skip a padded first child to trim a later line: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_does_not_skip_padded_last_child() {
    let body = "<div>A</div><div style=\"padding-bottom: 1px\">B</div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "margin: 0", body).await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not skip a padded last child to trim an earlier line: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_does_not_propagate_through_flex_child() {
    let body = "<div style=\"display: flex\"><span>C</span></div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "margin: 0", body).await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not propagate through a flex formatting context: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_does_not_propagate_through_grid_child() {
    let body = "<div style=\"display: grid\"><span>C</span></div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "margin: 0", body).await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not propagate through a grid formatting context: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_does_not_propagate_through_table_child() {
    let body =
        "<table style=\"border-spacing: 0\"><tr><td style=\"padding: 0\">C</td></tr></table>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case("text-box-trim: trim-end;", "margin: 0", body).await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not propagate through a table formatting context: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_edge_auto_uses_line_fit_edge_for_trim() {
    let body = "C";
    let untrimmed = render_text_box_trim_case("", "", body).await;
    let trimmed = render_text_box_trim_case(
        "text-box-trim: trim-end; text-box-edge: auto; line-fit-edge: alphabetic;",
        "",
        body,
    )
    .await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta > 25.0,
        "alphabetic line-fit-edge should trim more than text half-leading: delta={delta}"
    );
}

#[tokio::test]
async fn inherited_text_box_edge_auto_uses_affected_line_fit_edge_for_trim() {
    let body = "<div class=\"child\">C</div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case(
        "text-box-trim: trim-end; text-box-edge: auto;",
        "margin: 0; line-fit-edge: alphabetic;",
        body,
    )
    .await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta > 25.0,
        "inherited text-box-edge:auto should resolve against the affected line's line-fit-edge: delta={delta}"
    );
}

#[tokio::test]
async fn propagated_text_box_trim_uses_affected_block_text_box_edge() {
    let body = "<div class=\"child\">C</div>";
    let untrimmed = render_text_box_trim_case("", "margin: 0", body).await;
    let trimmed = render_text_box_trim_case(
        "text-box-trim: trim-end; text-box-edge: text;",
        "margin: 0; text-box-edge: alphabetic;",
        body,
    )
    .await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta > 25.0,
        "propagated trim should use the affected block's text-box-edge override: delta={delta}"
    );
}

#[tokio::test]
async fn line_fit_edge_layout_ignores_explicit_text_box_edge() {
    let render = |text_box_edge: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 240px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 0;
    line-height: 0;
    font-family: sans-serif;
  }}
  .fit {{
    font-size: 50px;
    line-height: 2;
    line-fit-edge: text;
    text-box-edge: {text_box_edge};
  }}
  .spacer {{ background: lightgray; block-size: 20px }}
</style>
<div class="target"><span class="fit">A</span></div>
<div class="spacer"></div>"#
        ))
    };
    let text_edge = render("text")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let alphabetic_edge = render("alphabetic")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let delta = bottom_lightgray_spacer_y(&alphabetic_edge) - bottom_lightgray_spacer_y(&text_edge);
    assert!(
        delta.abs() < 0.5,
        "line-fit-edge layout should not be affected by text-box-edge: delta={delta}"
    );
}

#[tokio::test]
async fn line_fit_edge_layout_includes_inline_block_axis_margins() {
    let render = |fit_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 260px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 1px;
    line-height: 1;
    font-family: sans-serif;
  }}
  .fit {{
    font-size: 50px;
    line-height: 2;
    line-fit-edge: text;
    {fit_extra}
  }}
  .spacer {{ background: lightgray; block-size: 20px }}
</style>
<div class="target"><span class="fit">A</span></div>
<div class="spacer"></div>"#
        ))
    };
    let without_margin = render("").render(&RenderOptions::default()).await.unwrap();
    let with_margin = render("margin-bottom: 40px;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let delta =
        bottom_lightgray_spacer_y(&with_margin) - bottom_lightgray_spacer_y(&without_margin);
    assert!(
        delta < -25.0,
        "line-fit-edge should include inline block-end margin in layout bounds: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_uses_innermost_block_container_metric() {
    let body = "<div class=\"child\">C</div>";
    let untrimmed = render_text_box_trim_case("", "", body).await;
    let trimmed = render_text_box_trim_case(
        "text-box-trim: trim-end; text-box-edge: alphabetic;",
        "text-box-trim: trim-end; text-box-edge: text;",
        body,
    )
    .await;

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "child trim metric should override ancestor metric: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_applies_per_multicol_column() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body, dl, dt, dd {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 50px }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 100px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<div class="spacer"></div>
<dl class="target">
  <dt>A</dt><dd>B</dd>
  <dt>C</dt><dd>D</dd>
  <dt>E</dt><dd>F</dd>
  <dt>G</dt><dd>H</dd>
</dl>
<div class="spacer"></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(
        rendered_text_lines(&trimmed),
        ["A", "B", "C", "D", "E", "F", "G", "H"]
    );
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-end should shorten every multicol column's last formatted line: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_fits_direct_inline_multicol_columns() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 220px;
    height: 190px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<div class="target">AAAA BBBB CCCC DDDD</div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(rendered_text_lines(&untrimmed), ["AAAA", "BBBB"]);
    assert_eq!(
        rendered_text_lines(&trimmed),
        ["AAAA", "BBBB", "CCCC", "DDDD"]
    );

    let column_index = |document: &spindrift::Document, text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"));
        usize::from(line.x() > 45.0)
    };
    assert_eq!(column_index(&trimmed, "BBBB"), 0);
    assert_eq!(column_index(&untrimmed, "BBBB"), 1);
    assert_eq!(column_index(&trimmed, "DDDD"), 1);
}

#[tokio::test]
async fn text_box_trim_end_fits_normalized_inline_multicol_columns() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 220px;
    height: 190px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<div class="target"><span>AAAA<br>BBBB<br>CCCC<br>DDDD</span></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(rendered_text_lines(&untrimmed), ["AAAA", "BBBB"]);
    assert_eq!(
        rendered_text_lines(&trimmed),
        ["AAAA", "BBBB", "CCCC", "DDDD"]
    );

    let column_index = |document: &spindrift::Document, text: &str| {
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"));
        usize::from(line.x() > 80.0)
    };
    assert_eq!(column_index(&trimmed, "BBBB"), 0);
    assert_eq!(column_index(&untrimmed, "BBBB"), 1);
    assert_eq!(column_index(&trimmed, "DDDD"), 1);
}

#[tokio::test]
async fn text_box_trim_end_applies_per_block_child_multicol_column() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 50px }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
  .target > div {{ margin: 0 }}
</style>
<div class="spacer"></div>
<div class="target">
  <div>A</div><div>B</div><div>C</div><div>D</div>
</div>
<div class="spacer"></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(rendered_text_lines(&trimmed), ["A", "B", "C", "D"]);
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-end should shorten every block-child multicol column's last formatted line: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_block_child_multicol_does_not_skip_padded_last_column_items() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 50px }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
  .target > div {{ margin: 0 }}
</style>
<div class="spacer"></div>
<div class="target">
  <div>A</div><div>B</div>
  <div style="padding-bottom: 1px">C</div>
  <div style="padding-bottom: 1px">D</div>
</div>
<div class="spacer"></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not skip padded block-child column items to trim earlier column lines: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_start_applies_per_block_child_multicol_column() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    column-count: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
  .target > div {{ margin: 0 }}
</style>
<div class="target">
  <div>A</div><div>B</div><div>C</div><div>D</div>
</div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line_y = |document: &spindrift::Document, text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .expect("line should render")
            .y()
    };
    for text in ["A", "B"] {
        let delta = line_y(&trimmed, text) - line_y(&untrimmed, text);
        assert!(
            (delta - 18.75).abs() < 0.5,
            "trim-start should move block-child column {text}'s first line into removed leading: delta={delta}"
        );
    }
}

#[tokio::test]
async fn text_box_trim_start_applies_per_multicol_column() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body, dl, dt, dd {{ margin: 0; padding: 0 }}
  .target {{
    columns: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<dl class="target">
  <dt>A</dt><dd>B</dd>
  <dt>C</dt><dd>D</dd>
  <dt>E</dt><dd>F</dd>
  <dt>G</dt><dd>H</dd>
</dl>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line_y = |document: &spindrift::Document, text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .expect("line should render")
            .y()
    };
    for text in ["A", "C"] {
        let delta = line_y(&trimmed, text) - line_y(&untrimmed, text);
        assert!(
            (delta - 18.75).abs() < 0.5,
            "trim-start should move column {text}'s first line into removed leading: delta={delta}"
        );
    }
}

#[tokio::test]
async fn text_box_trim_start_multicol_does_not_skip_padded_first_column_items() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body, dl, dt, dd {{ margin: 0; padding: 0 }}
  .target {{
    columns: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<dl class="target">
  <dt style="padding-top: 1px">A</dt><dd>B</dd>
  <dt style="padding-top: 1px">C</dt><dd>D</dd>
  <dt>E</dt><dd>F</dd>
  <dt>G</dt><dd>H</dd>
</dl>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-start;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let line_y = |document: &spindrift::Document, text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .expect("line should render")
            .y()
    };
    for text in ["E", "G"] {
        let delta = line_y(&trimmed, text) - line_y(&untrimmed, text);
        assert!(
            delta.abs() < 0.5,
            "trim-start should not skip a padded first column item to trim later column item {text}: delta={delta}"
        );
    }
}

#[tokio::test]
async fn text_box_trim_end_multicol_does_not_skip_padded_last_column_items() {
    let render = |target_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 500px 700px; margin: 0 }}
  html, body, dl, dt, dd {{ margin: 0; padding: 0 }}
  .spacer {{ background: lightgray; block-size: 50px }}
  .target {{
    columns: 2;
    column-gap: 20px;
    width: 220px;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-edge: text;
    {target_extra}
  }}
</style>
<div class="spacer"></div>
<dl class="target">
  <dt>A</dt><dd>B</dd>
  <dt>C</dt><dd>D</dd>
  <dt>E</dt><dd style="padding-bottom: 1px">F</dd>
  <dt>G</dt><dd style="padding-bottom: 1px">H</dd>
</dl>
<div class="spacer"></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-end;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta.abs() < 0.5,
        "trim-end should not skip padded last column items to trim earlier column lines: delta={delta}"
    );
}

#[tokio::test]
async fn text_box_trim_end_clones_per_page_fragment() {
    let render = |box_decoration_break: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 140pt; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-trim: trim-end;
    text-box-edge: text;
    box-decoration-break: {box_decoration_break};
  }}
</style>
<div class="target">A<br>B<br>C<br>D</div>"#
        ))
    };
    let sliced = render("slice")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let cloned = render("clone")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let page_texts = |document: &spindrift::Document| {
        document
            .pages
            .iter()
            .map(|page| {
                page.lines()
                    .iter()
                    .map(|line| line.text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|texts| !texts.is_empty())
            .collect::<Vec<_>>()
    };
    assert_eq!(page_texts(&sliced), [vec!["A"], vec!["B"], vec!["C", "D"]]);
    assert_eq!(page_texts(&cloned), [vec!["A", "B"], vec!["C", "D"]]);
}

#[tokio::test]
async fn text_box_trim_start_clones_paint_origin_per_page_fragment() {
    let render = |box_decoration_break: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 400px 120px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
    text-box-trim: trim-start;
    text-box-edge: text;
    box-decoration-break: {box_decoration_break};
  }}
</style>
<div class="target">A<br>B<br>C<br>D</div>"#
        ))
    };
    let sliced = render("slice")
        .render(&RenderOptions::default())
        .await
        .unwrap();
    let cloned = render("clone")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let first_line_y = |document: &spindrift::Document, page_index: usize| {
        document.pages[page_index]
            .lines()
            .iter()
            .find(|line| !line.text.trim().is_empty())
            .expect("page should have a visible line")
            .y()
    };

    let slice_y = first_line_y(&sliced, 1);
    let clone_y = first_line_y(&cloned, 1);
    let delta = clone_y - slice_y;
    assert!(
        (delta - 18.75).abs() < 0.5,
        "trim-start clone should move each page fragment's first line into removed leading: delta={delta}, slice_y={slice_y}, clone_y={clone_y}"
    );
}

#[tokio::test]
async fn inline_box_text_box_trim_shrinks_background_content_box() {
    let body = "A<span class=\"trimmed\">B</span>C";
    let untrimmed = render_inline_box_text_box_trim_case("", body).await;
    let trimmed = render_inline_box_text_box_trim_case("text-box-trim: trim-both;", body).await;

    assert_eq!(rendered_text_lines(&trimmed).join(""), "ABC");
    let (_, _, untrimmed_width, untrimmed_height) = inline_trim_background_rect(&untrimmed);
    let (_, _, trimmed_width, trimmed_height) = inline_trim_background_rect(&trimmed);
    assert!(
        (trimmed_width - untrimmed_width).abs() < 0.5,
        "inline trim should not change inline advance: untrimmed={untrimmed_width}, trimmed={trimmed_width}"
    );
    assert!(
        trimmed_height < untrimmed_height - 2.0,
        "inline trim should shrink the painted content box: untrimmed={untrimmed_height}, trimmed={trimmed_height}"
    );
}

#[tokio::test]
async fn split_inline_box_text_box_trim_shrinks_each_line_fragment_background() {
    let render = |span_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 220px 260px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    width: 110px;
    font-size: 20px;
    line-height: 2;
    font-family: sans-serif;
  }}
  .trimmed {{
    background: rgb(10, 20, 30);
    text-box-edge: cap alphabetic;
    {span_extra}
  }}
</style>
<div class="target"><span class="trimmed">alpha beta gamma delta</span></div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-both;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let untrimmed_rects = inline_trim_background_rects(&untrimmed);
    let trimmed_rects = inline_trim_background_rects(&trimmed);
    assert!(
        untrimmed_rects.len() > 1,
        "fixture should split the inline background across lines: {untrimmed_rects:?}"
    );
    assert_eq!(
        trimmed_rects.len(),
        untrimmed_rects.len(),
        "trim should preserve the split fragment count"
    );
    for ((_, _, untrimmed_width, untrimmed_height), (_, _, trimmed_width, trimmed_height)) in
        untrimmed_rects
            .iter()
            .copied()
            .zip(trimmed_rects.iter().copied())
    {
        assert!(
            (trimmed_width - untrimmed_width).abs() < 0.5,
            "split inline trim should not change fragment inline advance: untrimmed={untrimmed_width}, trimmed={trimmed_width}"
        );
        assert!(
            trimmed_height < untrimmed_height - 2.0,
            "split inline trim should shrink each fragment block size: untrimmed={untrimmed_height}, trimmed={trimmed_height}"
        );
    }
}

#[tokio::test]
async fn inline_box_text_box_trim_uses_ideographic_ink_metric() {
    let body = "A<span class=\"trimmed\">水</span>C";
    let untrimmed =
        render_inline_box_text_box_trim_case("text-box-edge: ideographic-ink alphabetic;", body)
            .await;
    let trimmed = render_inline_box_text_box_trim_case(
        "text-box-edge: ideographic-ink alphabetic; text-box-trim: trim-both;",
        body,
    )
    .await;

    assert_eq!(rendered_text_lines(&trimmed).join(""), "A水C");
    let (_, _, untrimmed_width, untrimmed_height) = inline_trim_background_rect(&untrimmed);
    let (_, _, trimmed_width, trimmed_height) = inline_trim_background_rect(&trimmed);
    assert!(
        (trimmed_width - untrimmed_width).abs() < 0.5,
        "ideographic-ink trim should not change inline advance"
    );
    assert!(
        trimmed_height < untrimmed_height - 2.0,
        "ideographic-ink should trim to glyph ink bounds: untrimmed={untrimmed_height}, trimmed={trimmed_height}"
    );
}

#[tokio::test]
async fn inline_box_text_box_trim_shrinks_link_rectangle() {
    let body = "<a class=\"trimmed\" href=\"https://example.com\">B</a>";
    let untrimmed = render_inline_box_text_box_trim_case("", body).await;
    let trimmed = render_inline_box_text_box_trim_case("text-box-trim: trim-both;", body).await;

    assert_eq!(untrimmed.pages[0].links().len(), 1);
    assert_eq!(trimmed.pages[0].links().len(), 1);
    let untrimmed_link = &untrimmed.pages[0].links()[0];
    let trimmed_link = &trimmed.pages[0].links()[0];
    assert_eq!(trimmed_link.target.as_ref(), "https://example.com");
    assert!(
        (trimmed_link.width() - untrimmed_link.width()).abs() < 0.5,
        "inline trim should not change link inline width"
    );
    assert!(
        trimmed_link.height() < untrimmed_link.height() - 2.0,
        "inline trim should shrink link block size: untrimmed={}, trimmed={}",
        untrimmed_link.height(),
        trimmed_link.height()
    );
}

#[tokio::test]
async fn vertical_inline_box_text_box_trim_shrinks_link_rectangle() {
    let body = "<a class=\"trimmed\" href=\"https://example.com\">B</a>";
    let untrimmed = render_vertical_inline_box_text_box_trim_case("", body).await;
    let trimmed =
        render_vertical_inline_box_text_box_trim_case("text-box-trim: trim-both;", body).await;

    assert_eq!(untrimmed.pages[0].links().len(), 1);
    assert_eq!(trimmed.pages[0].links().len(), 1);
    let untrimmed_link = &untrimmed.pages[0].links()[0];
    let trimmed_link = &trimmed.pages[0].links()[0];
    assert_eq!(trimmed_link.target.as_ref(), "https://example.com");
    let (background_x, background_y, background_width, background_height) =
        inline_trim_background_rect(&trimmed);
    assert!(
        (trimmed_link.x() - background_x).abs() < 0.5
            && (trimmed_link.y() - background_y).abs() < 0.5
            && (trimmed_link.width() - background_width).abs() < 0.5
            && (trimmed_link.height() - background_height).abs() < 0.5,
        "vertical trimmed link should use the trimmed content rect: link=({}, {}, {}, {}), background=({background_x}, {background_y}, {background_width}, {background_height}), old_width={}",
        trimmed_link.x(),
        trimmed_link.y(),
        trimmed_link.width(),
        trimmed_link.height(),
        untrimmed_link.width()
    );
}

#[tokio::test]
async fn bidi_inline_box_text_box_trim_keeps_link_rect_on_trimmed_fragment() {
    let render = |span_extra: &str| {
        Html::from_string(format!(
            r#"<!DOCTYPE html>
<meta charset="utf-8">
<style>
  @page {{ size: 360px 180px; margin: 0 }}
  html, body {{ margin: 0; padding: 0 }}
  .target {{
    direction: rtl;
    font-size: 50px;
    line-height: 2;
    font-family: sans-serif;
  }}
  .trimmed {{
    background: rgb(10, 20, 30);
    text-box-edge: cap alphabetic;
    {span_extra}
  }}
</style>
<div class="target">אב <a class="trimmed" href="https://example.com">ABC</a> גד</div>"#
        ))
    };
    let untrimmed = render("").render(&RenderOptions::default()).await.unwrap();
    let trimmed = render("text-box-trim: trim-both;")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let (_, _, _, untrimmed_background_height) = inline_trim_background_rect(&untrimmed);
    let (background_x, background_y, background_width, background_height) =
        inline_trim_background_rect(&trimmed);
    assert!(
        background_height < untrimmed_background_height - 2.0,
        "bidi inline trim should shrink the visual fragment background: untrimmed={untrimmed_background_height}, trimmed={background_height}"
    );
    assert_eq!(trimmed.pages[0].links().len(), 1);
    let link = &trimmed.pages[0].links()[0];
    assert_eq!(link.target.as_ref(), "https://example.com");
    assert!(
        (link.x() - background_x).abs() < 0.5
            && (link.y() - background_y).abs() < 0.5
            && (link.width() - background_width).abs() < 0.5
            && (link.height() - background_height).abs() < 0.5,
        "bidi visual ordering should keep the link rect on the trimmed background: link=({}, {}, {}, {}), background=({background_x}, {background_y}, {background_width}, {background_height})",
        link.x(),
        link.y(),
        link.width(),
        link.height()
    );
}

#[tokio::test]
async fn inline_box_text_box_trim_reduces_line_layout_bounds() {
    let untrimmed = render_inline_box_text_box_trim_layout_case("").await;
    let trimmed = render_inline_box_text_box_trim_layout_case("text-box-trim: trim-both;").await;

    assert_eq!(rendered_text_lines(&trimmed).join(""), "ABC");
    let delta = bottom_lightgray_spacer_y(&trimmed) - bottom_lightgray_spacer_y(&untrimmed);
    assert!(
        delta > 20.0,
        "trimmed inline box should reduce the consumed line block-size: delta={delta}"
    );
}

#[tokio::test]
async fn inline_box_text_box_trim_adjusts_decoration_geometry() {
    let body = "A<span class=\"trimmed\">B</span>C";
    let decoration = "text-decoration: green overline; text-decoration-skip-ink: none; text-decoration-thickness: 2px;";
    let untrimmed = render_inline_box_text_box_trim_case(decoration, body).await;
    let trimmed = render_inline_box_text_box_trim_case(
        &format!("{decoration} text-box-trim: trim-both;"),
        body,
    )
    .await;

    assert_eq!(rendered_text_lines(&trimmed).join(""), "ABC");
    let (_, untrimmed_y, untrimmed_width, _) = inline_trim_decoration_rect(&untrimmed);
    let (_, trimmed_y, trimmed_width, _) = inline_trim_decoration_rect(&trimmed);
    assert!(
        (trimmed_width - untrimmed_width).abs() < 0.5,
        "inline trim should not change decoration inline width"
    );
    assert!(
        (trimmed_y - untrimmed_y).abs() < 0.5,
        "text-box trimming changes box edges without moving the text decoration baseline: untrimmed={untrimmed_y}, trimmed={trimmed_y}"
    );
}

#[tokio::test]
async fn vertical_inline_box_text_box_trim_adjusts_decoration_geometry() {
    let body = "A<span class=\"trimmed\">B</span>C";
    let decoration = "text-decoration: green underline; text-decoration-skip-ink: none; text-decoration-thickness: 2px;";
    let untrimmed = render_vertical_inline_box_text_box_trim_case(decoration, body).await;
    let trimmed = render_vertical_inline_box_text_box_trim_case(
        &format!("{decoration} text-box-trim: trim-both;"),
        body,
    )
    .await;

    assert_eq!(rendered_text_lines(&trimmed).join(""), "ABC");
    let (untrimmed_x, _, untrimmed_width, untrimmed_height) =
        inline_trim_decoration_rect(&untrimmed);
    let (trimmed_x, _, trimmed_width, trimmed_height) = inline_trim_decoration_rect(&trimmed);
    assert!(
        (trimmed_height - untrimmed_height).abs() < 0.5,
        "vertical inline trim should not change decoration inline length"
    );
    assert!(
        trimmed_x < untrimmed_x - 1.0,
        "vertical decoration should move to the trimmed content edge: x=({untrimmed_x},{trimmed_x}) width=({untrimmed_width},{trimmed_width}) height=({untrimmed_height},{trimmed_height})"
    );
}

#[tokio::test]
async fn split_inline_after_block_omits_inline_start_border_for_wpt_case() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<meta charset="utf-8">
<title>CSS 2.1 Test Suite: handling of blocks inside inlines</title>
<style>
  body > span { border: 3px solid blue }
</style>
<body>
  <span
  ><div>One</div>
    Two
  </span>
</body>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let all_blue_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();
    let blue_rects = all_blue_rects
        .iter()
        .copied()
        .filter(|rect| rect.y() < 760.0)
        .collect::<Vec<_>>();
    assert_eq!(
        blue_rects.len(),
        3,
        "split inline final fragment should paint top, bottom, and inline-end edges only: all={all_blue_rects:?} final={blue_rects:?}"
    );

    let vertical_edges = blue_rects
        .iter()
        .copied()
        .filter(|rect| rect.width() < rect.height())
        .collect::<Vec<_>>();
    assert_eq!(
        vertical_edges.len(),
        1,
        "only the inline-end border should remain vertical on the final fragment: {blue_rects:?}"
    );

    let two = page
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Two")
        .expect("post-block inline text should render");
    let two_start = first_visible_glyph_x(two);
    assert!(
        vertical_edges[0].x() > two_start,
        "vertical split-inline border should be at inline end, not inline start: two={two:?}, border={:?}",
        vertical_edges[0]
    );
}

#[tokio::test]
async fn split_inline_empty_end_edge_paints_border_despite_negative_margin() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 240pt 240pt; margin: 0 }
body { margin: 0 }
p { display: none }
</style>
<p>Test passes if there is a filled green square and no red.</p>
<div style="width: 200px; height: 200px; background: red">
  <span style="font: 200px/1 sans-serif; border-right: 200px solid green; margin-right: -200px"><div></div></span>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("split inline end border should paint green");
    let green = &page.rects()[green_index];

    assert!(
        green.x().abs() < 0.01
            && (green.width() - 150.0).abs() < 0.01
            && (green.height() - 150.0).abs() < 0.01,
        "200px split inline border should cover the 200px square in PDF points: {green:?}"
    );
}

#[tokio::test]
async fn negative_margin_overflow_bfc_preserves_own_border() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<title>Negative margins in LTR/RTL and BFC/non-BFC</title>
<style>
@page { size: 240pt 260pt; margin: 0 }
html, body {
  margin: 0;
}
html {
  margin-left: 10px;
}
outer {
  display: block;
  border: blue 10px solid;
  width: 100px;
}
inner {
  display: block;
  border: orange 10px solid;
  margin-left: -20px;
  margin-right: -50px;
  height: 10px;
}
inner.bfc {
  overflow: hidden;
}
</style>
<body>
  <outer>
    <inner></inner>
  </outer>
  <outer dir="rtl">
    <inner></inner>
  </outer>
  <outer>
    <inner class="bfc"></inner>
  </outer>
  <outer dir="rtl">
    <inner class="bfc"></inner>
  </outer>
</body>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let orange = CssColor::new(255, 165, 0);
    let orange_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(orange))
        .collect::<Vec<_>>();
    let horizontal_edges = orange_rects
        .iter()
        .copied()
        .filter(|rect| (rect.width() - 127.5).abs() < 0.01 && (rect.height() - 7.5).abs() < 0.01)
        .collect::<Vec<_>>();
    let vertical_edges = orange_rects
        .iter()
        .copied()
        .filter(|rect| (rect.width() - 7.5).abs() < 0.01 && (rect.height() - 22.5).abs() < 0.01)
        .collect::<Vec<_>>();

    assert_eq!(
        horizontal_edges.len(),
        8,
        "each of the four negative-margin boxes should paint full-width top and bottom borders: {orange_rects:?}"
    );
    assert!(
        horizontal_edges.iter().all(|rect| rect.x().abs() < 0.01),
        "all LTR/RTL negative-margin inner border boxes should start at the physical left edge: {horizontal_edges:?}"
    );
    assert_eq!(
        vertical_edges.len(),
        8,
        "overflow BFC boxes should preserve their own side borders instead of clipping them to the padding box: {orange_rects:?}"
    );
    assert_eq!(
        vertical_edges
            .iter()
            .filter(|rect| rect.x().abs() < 0.01)
            .count(),
        4,
        "each inner border box should preserve its physical left border: {vertical_edges:?}"
    );
    assert_eq!(
        vertical_edges
            .iter()
            .filter(|rect| (rect.x() - 120.0).abs() < 0.01)
            .count(),
        4,
        "each inner border box should preserve its physical right border: {vertical_edges:?}"
    );
}

#[tokio::test]
async fn supports_explicit_block_dimensions() {
    let document = Html::from_string(
        "<div style=\"margin: 0; width: 50pt; height: 20pt; padding: 2pt; border: 1pt solid black; background: red\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects()[0];
    assert_eq!(rect.width(), 56.0);
    assert_eq!(rect.height(), 26.0);
}

#[tokio::test]
async fn padded_block_text_uses_content_box_once() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"margin:0;padding-left:10pt;font-size:10pt;line-height:10pt\">Text</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].x(), 20.0);
}

#[tokio::test]
async fn supports_percentage_block_widths() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 }</style><div style=\"margin:0; width:50%; height:10pt; background:red\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects()[0];
    assert_eq!(rect.width(), 90.0);
}

#[tokio::test]
async fn supports_flex_row_space_between() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; justify-content:space-between; width:100pt; font-size:10pt; line-height:10pt\"><span>A</span><span>B</span></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "A");
    assert_eq!(document.pages[0].lines()[1].text, "B");
    assert_eq!(document.pages[0].lines()[0].x(), 10.0);
    assert!(document.pages[0].lines()[1].x() >= 100.0);
}

#[tokio::test]
async fn inline_flex_paints_gap_decorations() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .flex { display: inline-flex; width: 50pt; column-gap: 10pt; column-rule: 4pt solid red }\
         .flex > div { width: 20pt; height: 10pt }\
         </style>\
         <span class=\"flex\"><div></div><div></div></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
        }),
        "inline flex should paint a vertical red column rule: {:?}",
        document.pages[0].strokes()
    );
}

#[tokio::test]
async fn wrapped_flex_paints_row_and_column_gap_decorations_with_orthogonal_items() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 220pt 160pt; margin: 0 }
body { margin: 0 }
#flexbox {
  display: flex;
  column-gap: 10px;
  column-rule: 10px solid red;
  row-gap: 30px;
  row-rule: 30px solid blue;
  height: 130px;
  width: 230px;
  flex-wrap: wrap;
  align-content: center;
}
.items {
  width: 70px;
  height: 50px;
  writing-mode: vertical-rl;
}
</style>
<div id="flexbox">
  <div class="items">One</div>
  <div class="items">Two</div>
  <div class="items">Three</div>
  <div class="items">Four</div>
  <div class="items">Five</div>
  <div class="items">Six</div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    assert!(
        page.rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 7.5).abs() < 0.01
                && rect.height() > 0.0
        }),
        "wrapped flex should paint vertical red column rules: {:?}",
        page.strokes()
    );
    assert!(
        page.rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(0, 0, 255))
                && (rect.height() - 22.5).abs() < 0.01
                && rect.width() > 0.0
        }),
        "wrapped flex should paint horizontal blue row rules: {:?}",
        page.strokes()
    );
}

#[tokio::test]
async fn grid_places_children_in_explicit_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 40pt 50pt; grid-template-rows: 12pt; column-gap: 5pt; width: 100pt }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first grid item background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second grid item background should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "red item: {red:?}");
    assert!((red.width() - 40.0).abs() < 0.01, "red item: {red:?}");
    assert!((blue.x() - 55.0).abs() < 0.01, "blue item: {blue:?}");
    assert!((blue.width() - 50.0).abs() < 0.01, "blue item: {blue:?}");
}

#[tokio::test]
async fn inline_grid_paints_gap_decorations() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: inline-grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 10pt; column-rule: 4pt solid red; width: 50pt }\
         </style>\
         <span class=\"grid\"><div></div><div></div></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0].rects().iter().any(|rect| {
            rect.fill == Some(CssColor::new(255, 0, 0))
                && (rect.width() - 4.0).abs() < 0.01
                && rect.height() > 0.0
        }),
        "inline grid should paint a vertical red column rule: {:?}",
        document.pages[0].strokes()
    );
}

#[tokio::test]
async fn grid_template_areas_place_named_items() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-areas: \"left right\"; grid-template-columns: 30pt 20pt; grid-template-rows: 10pt; width: 50pt }\
         .left { grid-area: left; background: red }\
         .right { grid-area: right; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"right\"></div><div class=\"left\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("left named grid area background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("right named grid area background should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "left item: {red:?}");
    assert!((red.width() - 30.0).abs() < 0.01, "left item: {red:?}");
    assert!((blue.x() - 40.0).abs() < 0.01, "right item: {blue:?}");
    assert!((blue.width() - 20.0).abs() < 0.01, "right item: {blue:?}");
}

#[tokio::test]
async fn all_unnamed_grid_template_areas_extend_the_explicit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-areas: \". . .\"; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt; width: 80pt }\
         .item { height: 10pt }\
         .red { background: red }\
         .green { background: green }\
         .blue { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"item red\"></div><div class=\"item green\"></div><div class=\"item blue\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01,
        "first explicit template track: {red:?}"
    );
    assert!(
        (green.x() - 30.0).abs() < 0.01,
        "second explicit template track: {green:?}"
    );
    assert!(
        (blue.x() - 60.0).abs() < 0.01,
        "third explicit template track: {blue:?}"
    );
}

#[tokio::test]
async fn grid_template_area_generates_named_start_and_end_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-areas: \"left right\"; grid-template-columns: 30pt 20pt; grid-template-rows: 10pt; width: 50pt }\
         .hit { grid-column: right-start / right-end; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"hit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("template-area generated-line grid item should paint");

    assert!(
        (rect.x() - 40.0).abs() < 0.01,
        "generated right-start line should begin at the second area: {rect:?}"
    );
    assert!(
        (rect.width() - 20.0).abs() < 0.01,
        "generated right-end line should end at the second area: {rect:?}"
    );
}

#[tokio::test]
async fn grid_template_area_generated_lines_accept_escaped_names() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 80pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-areas: \"\\31st \\32nd\"; grid-template-columns: 20pt 30pt; grid-template-rows: 10pt; column-gap: 5pt; width: 55pt }\
         .target { grid-column: \\32nd-start / \\32nd-end; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"target\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("escaped template-area line grid item should paint");

    assert!(
        (red.x() - 35.0).abs() < 0.01,
        "escaped area line names should resolve to the second column: {red:?}"
    );
    assert!(
        (red.width() - 30.0).abs() < 0.01,
        "escaped area line names should span the named area column: {red:?}"
    );
}

#[tokio::test]
async fn grid_named_line_occurrence_places_item() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: [main] 30pt [main] 20pt; grid-template-rows: 10pt; width: 50pt }\
         .hit { grid-column: main 2 / span 1; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"hit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("named-line occurrence grid item background should paint");

    assert!((rect.x() - 40.0).abs() < 0.01, "item: {rect:?}");
    assert!((rect.width() - 20.0).abs() < 0.01, "item: {rect:?}");
}

#[tokio::test]
async fn grid_spanning_item_includes_track_gaps() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 20pt 20pt 20pt; grid-template-rows: 10pt 10pt; column-gap: 5pt; row-gap: 4pt; width: 70pt }\
         .span { grid-column: 1 / span 2; grid-row: 1 / span 2; background: red }\
         .cell { grid-column: 3; grid-row: 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"cell\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("spanning grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("single-cell grid item should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "spanning item: {red:?}");
    assert!(
        (red.width() - 45.0).abs() < 0.01,
        "spanning item should include the column gap: {red:?}"
    );
    assert!(
        (red.height() - 24.0).abs() < 0.01,
        "spanning item should include the row gap: {red:?}"
    );
    assert!((blue.x() - 60.0).abs() < 0.01, "single cell: {blue:?}");
    assert!((blue.width() - 20.0).abs() < 0.01, "single cell: {blue:?}");
}

#[tokio::test]
async fn grid_flexible_tracks_distribute_remaining_width() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 1fr 2fr; grid-template-rows: 10pt; width: 90pt }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first flexible-track grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second flexible-track grid item should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (red.width() - 30.0).abs() < 0.01,
        "1fr track should receive one third of the free width: {red:?}"
    );
    assert!((blue.x() - 40.0).abs() < 0.01, "second item: {blue:?}");
    assert!(
        (blue.width() - 60.0).abs() < 0.01,
        "2fr track should receive two thirds of the free width: {blue:?}"
    );
}

#[tokio::test]
async fn grid_auto_fill_repeats_fixed_tracks_to_fill_definite_width() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: repeat(auto-fill, 20pt); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((red.x() - 10.0).abs() < 0.01, "first track: {red:?}");
    assert!((red.width() - 20.0).abs() < 0.01, "first track: {red:?}");
    assert!(
        (green.x() - 35.0).abs() < 0.01,
        "second auto-filled track should include the column gap: {green:?}"
    );
    assert!(
        (blue.x() - 60.0).abs() < 0.01,
        "third auto-filled track should fit in the definite grid width: {blue:?}"
    );
}

#[tokio::test]
async fn grid_auto_fit_collapses_empty_fixed_repeat_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; column-gap: 5pt; justify-content: end; width: 70pt }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first auto-fit grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second auto-fit grid item should paint");

    assert!(
        (red.x() - 35.0).abs() < 0.01,
        "auto-fit should collapse the empty third 20pt track before end-aligning the occupied tracks: {red:?}"
    );
    assert!((red.width() - 20.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (blue.x() - 60.0).abs() < 0.01,
        "occupied auto-fit tracks should keep their column gap: {blue:?}"
    );
    assert!((blue.width() - 20.0).abs() < 0.01, "second item: {blue:?}");
}

#[tokio::test]
async fn grid_auto_fit_merges_gutters_around_an_empty_interior_track() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .a { grid-column: 1; background: red }\
         .b { grid-column: 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first auto-fit grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("third auto-fit grid item should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "first item: {red:?}");
    assert!((red.width() - 20.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (blue.x() - 35.0).abs() < 0.01,
        "the collapsed middle track should preserve one merged gutter: {blue:?}"
    );
    assert!((blue.width() - 20.0).abs() < 0.01, "third item: {blue:?}");
}

#[tokio::test]
async fn grid_auto_template_rows_and_columns_size_to_items() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: auto auto; grid-template-rows: auto auto; width: 40pt; height: 30pt }\
         .a { grid-column: 1; grid-row: 1; width: 15pt; height: 10pt; background: red }\
         .b { grid-column: 2; grid-row: 1; width: 25pt; height: 10pt; background: blue }\
         .c { grid-column: 1; grid-row: 2; width: 15pt; height: 20pt; background: green }\
         .d { grid-column: 2; grid-row: 2; width: 25pt; height: 20pt; background: yellow }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first auto-track grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second auto-track grid item should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("third auto-track grid item should paint");
    let yellow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("fourth auto-track grid item should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "first column: {red:?}");
    assert!((red.width() - 15.0).abs() < 0.01, "first column: {red:?}");
    assert!(
        (blue.x() - 25.0).abs() < 0.01,
        "second auto column should start after the 15pt first column: {blue:?}"
    );
    assert!(
        (blue.width() - 25.0).abs() < 0.01,
        "second column: {blue:?}"
    );
    assert!(
        (green.y() - 80.0).abs() < 0.01,
        "second auto row should start after the 10pt first row in paint space: {green:?}"
    );
    assert!(
        (green.height() - 20.0).abs() < 0.01,
        "second row: {green:?}"
    );
    assert!(
        (yellow.x() - 25.0).abs() < 0.01,
        "second column: {yellow:?}"
    );
    assert!((yellow.y() - 80.0).abs() < 0.01, "second row: {yellow:?}");
}

#[tokio::test]
async fn grid_justify_content_space_evenly_distributes_column_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 10pt 10pt; grid-template-rows: 10pt; width: 80pt; justify-content: space-evenly }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first space-evenly grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second space-evenly grid item should paint");

    assert!(
        (red.x() - 30.0).abs() < 0.01,
        "space-evenly should place the first 10pt track after one 20pt interval: {red:?}"
    );
    assert!((red.width() - 10.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (blue.x() - 60.0).abs() < 0.01,
        "space-evenly should leave equal intervals between and around tracks: {blue:?}"
    );
    assert!((blue.width() - 10.0).abs() < 0.01, "second item: {blue:?}");
}

#[tokio::test]
async fn grid_align_content_space_evenly_distributes_row_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 10pt; grid-template-rows: 10pt 10pt; width: 10pt; height: 80pt; align-content: space-evenly }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first space-evenly row item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second space-evenly row item should paint");

    assert!(
        (red.y() - 80.0).abs() < 0.01,
        "space-evenly should place the first 10pt row after one 20pt interval: {red:?}"
    );
    assert!((red.height() - 10.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (blue.y() - 50.0).abs() < 0.01,
        "space-evenly should leave equal intervals between and around rows: {blue:?}"
    );
    assert!((blue.height() - 10.0).abs() < 0.01, "second item: {blue:?}");
}

#[tokio::test]
async fn rtl_grid_auto_placement_starts_at_inline_start_column() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; grid-template-columns: 20pt 20pt 20pt; grid-template-rows: 10pt; width: 60pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 50.0).abs() < 0.01,
        "RTL grid auto-placement should start at the rightmost column: {red:?}"
    );
    assert!(
        (green.x() - 30.0).abs() < 0.01,
        "second RTL auto-placed item should move leftward: {green:?}"
    );
    assert!(
        (blue.x() - 10.0).abs() < 0.01,
        "third RTL auto-placed item should occupy the leftmost track: {blue:?}"
    );
}

#[tokio::test]
async fn rtl_grid_keeps_unequal_tracks_in_physical_order() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; grid-template-columns: [right] 10pt [middle] 20pt [left] 30pt [end]; grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .a { grid-column: right / middle; background: red }\
         .b { background: green }\
         .c { grid-column: left / end; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((red.x() - 70.0).abs() < 0.01 && (red.width() - 10.0).abs() < 0.01);
    assert!((green.x() - 45.0).abs() < 0.01 && (green.width() - 20.0).abs() < 0.01);
    assert!((blue.x() - 10.0).abs() < 0.01 && (blue.width() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn grid_justify_self_self_start_uses_rtl_item_inline_start() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { justify-self: self-start; direction: rtl; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl self-start grid item should paint");
    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "rtl grid item self-start should align its right edge to the grid area's right edge: {green:?}"
    );
}

#[tokio::test]
async fn grid_justify_self_self_end_uses_rtl_item_inline_end() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { justify-self: self-end; direction: rtl; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl self-end grid item should paint");
    assert!(
        (green.x() - 10.0).abs() < 0.01,
        "rtl grid item self-end should align its left edge to the grid area's left edge: {green:?}"
    );
}

#[tokio::test]
async fn grid_justify_items_self_start_uses_rtl_item_inline_start() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; justify-items: self-start; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { direction: rtl; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-items:self-start grid item should paint");
    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "justify-items:self-start should align an RTL item's right edge to the grid area's right edge: {green:?}"
    );
}

#[tokio::test]
async fn grid_justify_items_self_end_uses_rtl_item_inline_end() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; justify-items: self-end; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { direction: rtl; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-items:self-end grid item should paint");
    assert!(
        (green.x() - 10.0).abs() < 0.01,
        "justify-items:self-end should align an RTL item's left edge to the grid area's left edge: {green:?}"
    );
}

#[tokio::test]
async fn rtl_grid_justify_self_left_uses_physical_left_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { justify-self: left; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-self:left grid item should paint");
    assert!(
        (green.x() - 10.0).abs() < 0.01,
        "justify-self:left should align to the physical left edge even in an RTL grid: {green:?}"
    );
}

#[tokio::test]
async fn rtl_grid_justify_self_right_uses_physical_right_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { justify-self: right; width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-self:right grid item should paint");
    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "justify-self:right should align to the physical right edge even in an RTL grid: {green:?}"
    );
}

#[tokio::test]
async fn rtl_grid_justify_items_left_uses_physical_left_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; justify-items: left; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-items:left grid item should paint");
    assert!(
        (green.x() - 10.0).abs() < 0.01,
        "justify-items:left should align auto justify-self items to the physical left edge in an RTL grid: {green:?}"
    );
}

#[tokio::test]
async fn rtl_grid_justify_items_right_uses_physical_right_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; direction: rtl; justify-items: right; grid-template-columns: 60pt; grid-template-rows: 10pt; width: 60pt }\
         .item { width: 20pt; height: 10pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl justify-items:right grid item should paint");
    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "justify-items:right should align auto justify-self items to the physical right edge in an RTL grid: {green:?}"
    );
}

#[tokio::test]
async fn grid_align_self_self_start_uses_vertical_rtl_item_inline_start() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 40pt; grid-template-rows: 60pt; width: 40pt }\
         .item { align-self: self-start; writing-mode: vertical-lr; direction: rtl; width: 20pt; height: 20pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical rtl align-self:self-start grid item should paint");
    assert!(
        (green.y() - 30.0).abs() < 0.01,
        "self-start should align the vertical-rtl item's inline-start/bottom side to the grid area's bottom edge: {green:?}"
    );
}

#[tokio::test]
async fn grid_align_self_self_end_uses_vertical_rtl_item_inline_end() {
    let document = Html::from_string(
        "<style>\
         @page { size: 120pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 40pt; grid-template-rows: 60pt; width: 40pt }\
         .item { align-self: self-end; writing-mode: vertical-lr; direction: rtl; width: 20pt; height: 20pt; background: green }\
         </style><div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("vertical rtl align-self:self-end grid item should paint");
    assert!(
        ((green.y() + green.height()) - 90.0).abs() < 0.01,
        "self-end should align the vertical-rtl item's inline-end/top side to the grid area's top edge: {green:?}"
    );
}

#[tokio::test]
async fn grid_auto_flow_column_places_items_down_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: column; grid-template-rows: 10pt 10pt; grid-auto-columns: 15pt; width: 45pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((red.x() - 10.0).abs() < 0.01, "first item: {red:?}");
    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "column auto-flow should place the second item in the next row: red={red:?}, green={green:?}"
    );
    assert!(
        (blue.x() - 25.0).abs() < 0.01,
        "column auto-flow should create the next implicit column: {blue:?}"
    );
    assert!(
        (blue.y() - red.y()).abs() < 0.01,
        "third item should return to the first row of the next column: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_auto_rows_size_implicit_row_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; grid-auto-rows: 14pt; row-gap: 3pt; width: 40pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.y() - green.y()).abs() < 0.01,
        "first two items should share the explicit row: red={red:?}, green={green:?}"
    );
    assert!(
        (red.height() - 10.0).abs() < 0.01,
        "explicit row should be 10pt tall: {red:?}"
    );
    assert!(
        (blue.height() - 14.0).abs() < 0.01,
        "implicit row should use grid-auto-rows: {blue:?}"
    );
    assert!(
        (blue.y() + blue.height() + 3.0 - red.y()).abs() < 0.01,
        "implicit row should be separated from the explicit row by row-gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_auto_rows_cycles_implicit_track_list() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; grid-auto-rows: 12pt 18pt; width: 40pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         .d { background: yellow }\
         .e { background: magenta }\
         .f { background: cyan }\
         .g { background: black }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div><div class=\"e\"></div><div class=\"f\"></div><div class=\"g\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let magenta = rect(CssColor::new(255, 0, 255));
    let black = rect(CssColor::new(0, 0, 0));

    assert!(
        (red.height() - 10.0).abs() < 0.01,
        "explicit row should be 10pt tall: {red:?}"
    );
    assert!(
        (blue.height() - 12.0).abs() < 0.01,
        "first implicit row should use the first grid-auto-rows track: {blue:?}"
    );
    assert!(
        (magenta.height() - 18.0).abs() < 0.01,
        "second implicit row should use the second grid-auto-rows track: {magenta:?}"
    );
    assert!(
        (black.height() - 12.0).abs() < 0.01,
        "third implicit row should cycle back to the first grid-auto-rows track: {black:?}"
    );
}

#[tokio::test]
async fn grid_auto_columns_cycles_implicit_track_list() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: column; grid-template-rows: 10pt 10pt; grid-auto-columns: 12pt 18pt; width: 80pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         .d { background: yellow }\
         .e { background: magenta }\
         .f { background: cyan }\
         .g { background: black }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div><div class=\"e\"></div><div class=\"f\"></div><div class=\"g\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let magenta = rect(CssColor::new(255, 0, 255));
    let black = rect(CssColor::new(0, 0, 0));

    assert!(
        (red.width() - 12.0).abs() < 0.01,
        "first implicit column should use the first grid-auto-columns track: {red:?}"
    );
    assert!(
        (blue.width() - 18.0).abs() < 0.01,
        "second implicit column should use the second grid-auto-columns track: {blue:?}"
    );
    assert!(
        (magenta.width() - 12.0).abs() < 0.01,
        "third implicit column should cycle back to the first grid-auto-columns track: {magenta:?}"
    );
    assert!(
        (black.width() - 18.0).abs() < 0.01,
        "fourth implicit column should cycle back to the second grid-auto-columns track: {black:?}"
    );
}

#[tokio::test]
async fn grid_auto_flow_dense_backfills_earlier_row_holes() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: row dense; grid-template-columns: 20pt 20pt 20pt; grid-auto-rows: 10pt; width: 60pt }\
         .wide { grid-column: span 2; background: red }\
         .wide2 { grid-column: span 2; background: green }\
         .single { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"wide\"></div><div class=\"wide2\"></div><div class=\"single\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((red.x() - 10.0).abs() < 0.01, "first wide item: {red:?}");
    assert!(
        (green.y() + 10.0 - red.y()).abs() < 0.01,
        "second wide item should move to the next row: red={red:?}, green={green:?}"
    );
    assert!(
        (blue.x() - 50.0).abs() < 0.01,
        "dense auto-placement should backfill the first-row hole: {blue:?}"
    );
    assert!(
        (blue.y() - red.y()).abs() < 0.01,
        "dense item should share the first row with the first wide item: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_creates_anonymous_items_for_non_whitespace_text_runs() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .item { background: red }\
         </style>\
         <div class=\"grid\">A<div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = page
        .lines()
        .iter()
        .find(|line| line.text.trim() == "A")
        .expect("anonymous grid text should render");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("grid item after anonymous text should paint");

    assert!(
        (first_visible_glyph_x(line) - 10.0).abs() < 0.01,
        "anonymous text should occupy the first grid cell: {line:?}"
    );
    assert!(
        (red.x() - 30.0).abs() < 0.01,
        "following grid item should occupy the second grid cell: {red:?}"
    );
}

#[tokio::test]
async fn grid_item_blocks_descendant_margin_escape_and_container_first_letter() {
    let grid_document = Html::from_string(
        "<!DOCTYPE html>\
         <meta charset=\"utf-8\">\
         <style>\
         @page { size: 800px 220px; margin: 0 }\
         .spacer { height: 1px }\
         .grid { display: grid; color: green }\
         .grid::first-letter { color: red }\
         </style>\
         <div class=\"spacer\"></div><div class=\"grid\"><div><p>This text should be <strong>green</strong> and body and paragraph margins should <strong>not collapse</strong>.</p></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference_document = Html::from_string(
        "<!DOCTYPE html>\
         <meta charset=\"utf-8\">\
         <style>\
         @page { size: 800px 220px; margin: 0 }\
         .spacer { height: 1px }\
         p { color: green; float: left }\
         </style>\
         <div class=\"spacer\"></div><p>This text should be <strong>green</strong> and body and paragraph margins should <strong>not collapse</strong>.</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let grid_lines = grid_document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let reference_line = reference_document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("This text"))
        .unwrap_or_else(|| {
            panic!(
                "reference text should render: {:?}",
                reference_document.pages[0].lines()
            )
        });
    let grid_line = grid_lines
        .iter()
        .find(|line| line.text.contains("This text"))
        .unwrap_or_else(|| {
            panic!(
                "grid text should render: {:?}",
                grid_document.pages[0].lines()
            )
        });

    assert!(
        (grid_line.y() - reference_line.y()).abs() < 0.01,
        "grid item content should keep paragraph margin inside the item: grid={grid_line:?}, reference={reference_line:?}"
    );
    assert!(
        grid_lines
            .iter()
            .all(|line| line.color == CssColor::new(0, 128, 0)),
        "grid container ::first-letter should not style grid item text: {grid_lines:?}"
    );
}

#[tokio::test]
async fn grid_outer_margins_adjoin_block_siblings_while_item_margins_stay_contained() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         p { line-height: 10pt }\
         .before { margin: 0 0 20pt }\
         .grid { display: grid; margin: 10pt 0 0 }\
         .grid > p { margin: 15pt 0 0 }\
         .after { margin: 0 }\
         </style>\
         <p class=\"before\">Before</p><div class=\"grid\"><p>Inside</p></div><p class=\"after\">After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text| {
        page.lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("expected {text:?} line: {:?}", page.lines()))
    };
    let before = line("Before");
    let inside = line("Inside");
    let after = line("After");

    assert!(
        ((inside.y() - before.y()).abs() - 45.0).abs() < 0.01,
        "the outer 20pt/10pt sibling margins should collapse to 20pt and the grid item's 15pt margin should remain inside the grid: before={before:?}, inside={inside:?}"
    );
    assert!(
        ((after.y() - inside.y()).abs() - 10.0).abs() < 0.01,
        "the following block should start after the grid item's contained line box: inside={inside:?}, after={after:?}"
    );
}

#[tokio::test]
async fn document_canvas_top_inset_is_invariant_to_anonymous_grid_item_splitting() {
    let grid = Html::from_string(
        "<!DOCTYPE html><style>@page { margin: 0 }</style><meta charset=\"utf-8\"><title>CSS Grid Test: Anonymous grid items - non-contiguous text runs - position:absolute</title>\
         <link rel=\"author\" title=\"Rune Lillesveen\" href=\"mailto:futhark@chromium.org\">\
         <p>The words \"Two\" and \"lines\" should not be on the same line.</p>\
         <div style=\"display:grid\">Two <span style=\"position:absolute\"></span>lines</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<!DOCTYPE html><style>@page { margin: 0 }</style><meta charset=\"utf-8\"><title>CSS Reftest Reference</title>\
         <link rel=\"author\" title=\"Rune Lillesveen\" href=\"mailto:futhark@chromium.org\">\
         <p>The words \"Two\" and \"lines\" should not be on the same line.</p>\
         Two<br>lines",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line_positions = |document: &spindrift::Document| {
        document.pages[0]
            .lines()
            .iter()
            .map(|line| (line.text.trim().to_owned(), line.x(), line.y()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        line_positions(&grid),
        line_positions(&reference),
        "a document-canvas inset must not depend on whether following text is normalized into anonymous grid items"
    );
}

#[tokio::test]
async fn document_canvas_body_start_margin_collapses_with_first_child() {
    let with_canvas_inset = Html::from_string(
        "<style>@page { margin: 0 } body { margin: 8px } p { margin: 12pt 0 0; font-size: 10pt; line-height: 10pt }</style><p>Inset</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let without_canvas_inset = Html::from_string(
        "<style>@page { margin: 0 } body { margin: 0 } p { margin: 12pt 0 0; font-size: 10pt; line-height: 10pt }</style><p>Inset</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let with_inset = with_canvas_inset.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Inset")
        .expect("document with a body canvas inset should render its child");
    let without_inset = without_canvas_inset.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Inset")
        .expect("document without a body canvas inset should render its child");

    assert!(
        (with_inset.y() - without_inset.y()).abs() < 0.01,
        "the body and its first child's adjoining block-start margins must collapse: with={with_inset:?}, without={without_inset:?}"
    );
    assert!(
        ((with_inset.x() - without_inset.x()).abs() - 6.0).abs() < 0.01,
        "the body canvas inset must still offset the inline position: with={with_inset:?}, without={without_inset:?}"
    );
}

#[tokio::test]
async fn grid_ignores_whitespace_only_anonymous_text_runs() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\">   <div class=\"a\"></div>   <div class=\"b\"></div>   </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first non-whitespace grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second non-whitespace grid item should paint");

    assert!(
        (red.x() - 10.0).abs() < 0.01,
        "collapsible whitespace should not consume the first grid cell: {red:?}"
    );
    assert!(
        (blue.x() - 30.0).abs() < 0.01,
        "second item should occupy the second grid cell: {blue:?}"
    );
}

#[tokio::test]
async fn grid_ignores_preserved_document_whitespace_text_runs() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; white-space: pre; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .item { background: red }\
         </style>\
         <div class=\"grid\">\n\n<div class=\"item\"></div>\t\n</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("non-whitespace grid item should paint");

    assert!(
        (red.x() - 10.0).abs() < 0.01,
        "preserved document whitespace should not consume a grid cell: {red:?}"
    );
}

#[tokio::test]
async fn grid_creates_anonymous_items_for_nbsp_text_runs() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .item { background: red }\
         </style>\
         <div class=\"grid\">&nbsp;<div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("grid item after anonymous NBSP text should paint");

    assert!(
        (red.x() - 30.0).abs() < 0.01,
        "NBSP should create an anonymous grid item before the real item: {red:?}"
    );
}

#[tokio::test]
async fn grid_display_contents_children_participate_as_grid_items() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .contents { display: contents }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"contents\"><div class=\"a\"></div><div class=\"b\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first display: contents child should paint as a grid item");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second display: contents child should paint as a grid item");

    assert!((red.x() - 10.0).abs() < 0.01, "first child: {red:?}");
    assert!((blue.x() - 30.0).abs() < 0.01, "second child: {blue:?}");
}

#[tokio::test]
async fn generated_after_pseudo_participates_as_grid_item() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; width: 40pt }\
         .grid::after { content: 'B'; display: block; background: blue; width: 20pt; height: 10pt }\
         .item { background: red; width: 20pt; height: 10pt }\
         </style>\
         <div class=\"grid\"><div class=\"item\">A</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("real grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("generated ::after grid item should paint");

    assert!((red.x() - 10.0).abs() < 0.01, "real item: {red:?}");
    assert!(
        (blue.x() - 30.0).abs() < 0.01,
        "generated ::after should occupy the next grid cell: {blue:?}"
    );
    assert!(
        page.lines().iter().any(|line| line.text.trim() == "B"),
        "generated ::after text should render as grid item content: {:?}",
        page.lines()
    );
}

#[tokio::test]
async fn generated_pseudo_grid_items_participate_in_order_sorting() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt 20pt 20pt; grid-template-rows: 10pt; width: 60pt }\
         .grid::before, .grid::after, span { display: block; width: 20pt; height: 10pt }\
         .grid::before { content: 'A'; order: 3 }\
         span { order: 2 }\
         .grid::after { content: 'C'; order: 1 }\
         </style>\
         <div class=\"grid\"><span>B</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut line_text_by_x = document.pages[0]
        .lines()
        .iter()
        .map(|line| (line.x(), line.text.trim()))
        .collect::<Vec<_>>();
    line_text_by_x.sort_by(|left, right| left.0.total_cmp(&right.0));
    let text = line_text_by_x
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>();

    assert_eq!(
        text,
        vec!["C", "B", "A"],
        "pseudo and element grid items should share order-modified document order: {:?}",
        document.pages[0].lines()
    );
}

#[tokio::test]
async fn grid_blockified_inline_item_paints_source_background_once() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 40pt; grid-template-rows: 10pt; width: 40pt }\
         .item { display: inline; background: rgb(10, 20, 30); color: transparent }\
         </style>\
         <div class=\"grid\"><span class=\"item\">text</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let painted = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(10, 20, 30)))
        .collect::<Vec<_>>();
    assert_eq!(
        painted.len(),
        1,
        "blockified inline grid item should not also paint inline text-fragment background: {painted:?}"
    );
}

#[tokio::test]
async fn grid_min_content_track_uses_item_intrinsic_width() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: min-content 20pt; grid-template-rows: 10pt; width: 100pt; font-size: 10pt }\
         .a { background: red; white-space: nowrap }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("min-content grid item background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second grid item background should paint");

    assert!(red.width() > 40.0, "min-content item: {red:?}");
    assert!(
        blue.x() >= red.x() + red.width() - 0.01,
        "tracks should not overlap: red={red:?}, blue={blue:?}"
    );
    assert!((blue.width() - 20.0).abs() < 0.01, "fixed item: {blue:?}");
}

#[tokio::test]
async fn grid_fit_content_track_clamps_between_min_and_max_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 140pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; width: 100pt; grid-template-rows: 10pt; margin-bottom: 2pt }\
         .min { grid-template-columns: min-content }\
         .fit { grid-template-columns: fit-content(30pt) }\
         .max { grid-template-columns: max-content }\
         .min > div { background: red }\
         .fit > div { background: green }\
         .max > div { background: blue }\
         </style>\
         <div class=\"grid min\"><div>Hi there friend</div></div>\
         <div class=\"grid fit\"><div>Hi there friend</div></div>\
         <div class=\"grid max\"><div>Hi there friend</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let min = rect(CssColor::new(255, 0, 0));
    let fit = rect(CssColor::new(0, 128, 0));
    let max = rect(CssColor::new(0, 0, 255));

    assert!(
        min.width() < fit.width(),
        "fit-content track should be wider than min-content: min={min:?}, fit={fit:?}"
    );
    assert!(
        fit.width() < max.width(),
        "fit-content track should be narrower than max-content: fit={fit:?}, max={max:?}"
    );
    assert!(
        (fit.width() - 30.0).abs() < 0.5,
        "fit-content(30pt) should use the argument between intrinsic bounds: {fit:?}"
    );
}

#[tokio::test]
async fn grid_fit_content_intrinsic_width_caps_at_max_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; width: max-content; grid-template-columns: fit-content(100pt); grid-template-rows: 10pt; background: yellow }\
         .item { background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"item\">Hi</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("fit-content intrinsic grid item should paint");

    assert!(
        red.width() > 5.0,
        "test fixture should have a visible max-content contribution: {red:?}"
    );
    assert!(
        red.width() < 40.0,
        "fit-content(100pt) should cap at the item's max-content contribution when sizing a max-content grid: {red:?}"
    );
}

#[tokio::test]
async fn grid_min_content_row_uses_item_intrinsic_height() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt; grid-template-rows: min-content 10pt; width: 20pt; row-gap: 5pt }\
         .a { height: 30pt; background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("min-content row grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second row grid item should paint");

    assert!(
        (red.height() - 30.0).abs() < 0.01,
        "min-content row: {red:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "second row should start after the first row's intrinsic height and the row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_min_content_row_treats_indefinite_percentage_item_height_as_auto() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 40pt; grid-template-rows: min-content 10pt; width: 40pt; row-gap: 5pt }\
         .a { height: 50%; min-height: 10pt; background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("percentage-height min-content row grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second row grid item should paint");

    assert!(
        (red.height() - 10.0).abs() < 0.01,
        "indefinite percentage item height should behave as auto and leave min-height to size the min-content row: {red:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "second row should follow the auto-height first row and row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_min_content_rows_distribute_spanning_item_intrinsic_height() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 140pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; grid-template-columns: 20pt; grid-template-rows: min-content min-content 10pt; width: 20pt; row-gap: 5pt }\
         .span { grid-row: 1 / 3; height: 45pt; background: red }\
         .after { grid-row: 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"after\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("spanning min-content row grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("post-span row grid item should paint");

    assert!(
        (red.height() - 45.0).abs() < 0.01,
        "spanning item should preserve its intrinsic content height: {red:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "post-span row should start after the spanning contribution and only the crossed row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_min_content_height_treats_percentage_row_gap_as_cyclic() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt 10pt; row-gap: 50%; background: yellow }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("percentage-row-gap min-content grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first percentage-row-gap grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second percentage-row-gap grid item should paint");

    assert!(
        (grid.height() - 20.0).abs() < 0.01,
        "cyclic percentage row gaps should resolve to zero for grid intrinsic height: {grid:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height()) - 10.0).abs() < 0.01,
        "percentage row gap should resolve against the intrinsic grid height for final item layout: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_percentage_gap_final_layout_resolves_against_content_box() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320pt 260pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 200pt; gap: 10%; grid-template-columns: 90pt 90pt; grid-template-rows: 90pt 90pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         .c { background: cyan }\
         .d { background: magenta }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected rect for color {color:?}: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(0, 128, 0));
    let first = rect(CssColor::new(255, 0, 0));
    let second = rect(CssColor::new(0, 0, 255));
    let third = rect(CssColor::new(0, 255, 255));

    assert!(
        (grid.height() - 180.0).abs() < 0.01,
        "cyclic percentage row gap should resolve to zero for the auto grid height: {grid:?}"
    );
    assert!(
        (second.x() - (first.x() + first.width()) - 20.0).abs() < 0.01,
        "column gap should resolve against the 200pt content width: first={first:?}, second={second:?}"
    );
    assert!(
        (first.y() - (third.y() + third.height()) - 18.0).abs() < 0.01,
        "row gap should resolve against the 180pt content height for final item layout: first={first:?}, third={third:?}"
    );
}

#[tokio::test]
async fn grid_final_item_sizing_excludes_the_following_gutter() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 100pt 40pt; grid-template-rows: 20pt 20pt; column-gap: 20pt; row-gap: 5pt; width: 160pt }\
         .percentage { grid-column: 1 / 2; grid-row: 1; width: calc(10pt + 50%); background: red }\
         .ratio { grid-column: 1 / 2; grid-row: 2; width: auto; height: 20pt; aspect-ratio: 2; justify-self: stretch; background: green }\
         </style>\
         <div class=\"grid\"><div class=\"percentage\"></div><div class=\"ratio\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("mixed-percentage Grid item should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("aspect-ratio Grid item should paint");

    assert!(
        (red.width() - 60.0).abs() < 0.01,
        "percentage item: {red:?}"
    );
    assert!(
        (green.width() - 100.0).abs() < 0.01,
        "ratio item: {green:?}"
    );
}

#[tokio::test]
async fn inline_grid_percentage_gap_overflows_intrinsic_width() {
    let document = Html::from_string(
        "<style>\
         @page { size: 360pt 220pt; margin: 10pt }\
         body { margin: 0; font-size: 0; line-height: 0 }\
         .grid { display: inline-grid; width: auto; gap: calc(20pt + 5%); grid-template-columns: 90pt 90pt; grid-template-rows: 90pt 90pt; background: black }\
         .a { background: red }\
         .b { background: blue }\
         .c { background: cyan }\
         .d { background: magenta }\
         </style>\
         <span class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div><div class=\"d\"></div></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("expected rect for color {color:?}: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(0, 0, 0));
    let first = rect(CssColor::new(255, 0, 0));
    let second = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 200.0).abs() < 0.01,
        "inline-grid intrinsic width should include only the fixed gap component: {grid:?}"
    );
    assert!(
        (second.x() - (first.x() + first.width()) - 30.0).abs() < 0.01,
        "final column gap should resolve calc(20pt + 5%) against the 200pt content width: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content 20pt; grid-template-rows: 10pt; font-size: 10pt; background: yellow }\
         .a { background: red; white-space: nowrap }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("min-content grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first grid item background should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second grid item background should paint");

    assert!(
        grid.width() < 100.0,
        "grid should shrink-wrap tracks: {grid:?}"
    );
    assert!(
        grid.width() >= red.width() + blue.width() - 0.01,
        "grid should contain tracks: grid={grid:?}, red={red:?}, blue={blue:?}"
    );
    assert!((blue.width() - 20.0).abs() < 0.01, "fixed item: {blue:?}");
}

#[tokio::test]
async fn grid_container_min_content_width_uses_per_track_item_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { grid-column: 1; background: red; white-space: nowrap }\
         .b { grid-column: 2; background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\">i</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("min-content grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second intrinsic grid item should paint");

    assert!(
        red.width() > blue.width() * 4.0,
        "test fixture should have meaningfully different item contributions: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < red.width() * 1.5,
        "grid min-content width should not apply the first column contribution to every intrinsic track: grid={grid:?}, red={red:?}, blue={blue:?}"
    );
    assert!(
        (blue.x() - (red.x() + red.width() + 5.0)).abs() < 0.01,
        "second track should start after the first track's own contribution and the gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_treats_percentage_gap_as_cyclic() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 50%; background: yellow }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("percentage-gap intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first percentage-gap intrinsic grid item should paint");

    assert!(
        (red.width() - 20.0).abs() < 0.01,
        "fixed track should establish the test fixture width: {red:?}"
    );
    assert!(
        (grid.width() - 40.0).abs() < 0.01,
        "cyclic percentage gaps should resolve to zero for grid intrinsic width contributions: {grid:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_treats_percentage_tracks_as_auto() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; width: min-content; grid-template-columns: 50% 20pt; grid-template-rows: 10pt; background: yellow }\
         .a { background: red; white-space: nowrap }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("percentage-track intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first percentage-track intrinsic grid item should paint");

    assert!(
        grid.width() > 60.0,
        "percentage tracks should behave as auto and include item intrinsic contributions: grid={grid:?}, red={red:?}"
    );
    assert!(
        grid.width() < 120.0,
        "percentage tracks should not resolve against the containing block for grid intrinsic width contributions: {grid:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_places_mixed_auto_and_explicit_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { background: red; white-space: nowrap }\
         .b { grid-column: 2; background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\">i</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("mixed auto/explicit intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("auto intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("explicit intrinsic grid item should paint");

    assert!(
        red.width() > blue.width() * 4.0,
        "test fixture should have meaningfully different mixed item contributions: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < red.width() * 1.5,
        "mixed auto/explicit contribution assignment should not apply the auto item contribution to every intrinsic track: grid={grid:?}, red={red:?}"
    );
    assert!(
        (blue.x() - (red.x() + red.width() + 5.0)).abs() < 0.01,
        "explicit second column should start after the auto-placed first column and the gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_dense_auto_placement_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: row dense; width: min-content; grid-template-columns: min-content min-content min-content; grid-template-rows: 10pt 10pt; column-gap: 5pt; row-gap: 5pt; font-size: 10pt; background: yellow }\
         .blocker { grid-column: 2; background: blue; white-space: nowrap }\
         .span { grid-column: span 2; background: red; white-space: nowrap }\
         .late { background: green; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"blocker\">i</div><div class=\"span\">MMMMMMMM</div><div class=\"late\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("dense intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("spanning dense intrinsic grid item should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("dense backfilled intrinsic grid item should paint");

    assert!(
        green.x() < red.x() + 0.01,
        "dense auto-placement should backfill the later item into the first column: red={red:?}, green={green:?}"
    );
    assert!(
        grid.width() < green.width() + red.width(),
        "dense intrinsic contribution assignment should reuse the first column instead of adding the late item to a later empty column: grid={grid:?}, red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_column_auto_flow_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: column; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt 10pt; column-gap: 5pt; row-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { background: red; white-space: nowrap }\
         .b { background: green; white-space: nowrap }\
         .c { background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">MMMMMMMM</div><div class=\"b\">MMMMMMMM</div><div class=\"c\">i</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("column-flow intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first column-flow intrinsic grid item should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("second column-flow intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("third column-flow intrinsic grid item should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "column auto-flow should place the first two items in the same intrinsic column: red={red:?}, green={green:?}"
    );
    assert!(
        blue.x() > red.x() + red.width(),
        "third column-flow item should advance to the next intrinsic column: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < red.width() * 1.5,
        "column auto-flow intrinsic contribution assignment should not put both wide items in separate intrinsic columns: grid={grid:?}, red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_column_dense_auto_placement_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: column dense; width: min-content; grid-template-columns: min-content min-content min-content; grid-template-rows: 10pt 10pt; column-gap: 5pt; row-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-row: 1; grid-column: span 2; background: red; white-space: nowrap }\
         .late { grid-row: 2; background: green; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div><div class=\"late\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("column-dense intrinsic grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("spanning column-dense intrinsic grid item should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("column-dense backfilled intrinsic grid item should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "column-dense auto-placement should backfill the later item into the first column: red={red:?}, green={green:?}"
    );
    assert!(
        grid.width() < red.width() + green.width(),
        "column-dense intrinsic contribution assignment should reuse the first column instead of adding the late item to a later empty column: grid={grid:?}, red={red:?}, green={green:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_column_auto_flow_honors_definite_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: column; width: min-content; grid-template-rows: 10pt 10pt; grid-auto-columns: 20pt 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a, .b, .c { grid-row: 2; height: 10pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "column auto-flow min-content width should include one implicit column per same-row item plus gaps: {grid:?}"
    );
    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 20.0).abs() < 0.01,
        "first same-row column-flow item should use the first auto column: {red:?}"
    );
    assert!(
        (green.x() - 35.0).abs() < 0.01 && (green.width() - 30.0).abs() < 0.01,
        "second same-row column-flow item should use the second auto column: {green:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "third same-row column-flow item should use the third auto column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_row_auto_flow_honors_definite_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: row; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a, .b, .c { grid-row: 2; height: 10pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "row auto-flow min-content width should include implicit columns for same-row items plus gaps: {grid:?}"
    );
    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 20.0).abs() < 0.01,
        "first same-row row-flow item should use the authored column: {red:?}"
    );
    assert!(
        (green.x() - 35.0).abs() < 0.01 && (green.width() - 30.0).abs() < 0.01,
        "second same-row row-flow item should use the first implicit auto column: {green:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "third same-row row-flow item should use the second implicit auto column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_row_auto_flow_honors_area_row_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: row; width: min-content; grid-template-areas: \"top\" \"slot\"; grid-template-columns: 20pt; grid-auto-columns: 30pt 40pt; grid-template-rows: 10pt 10pt; column-gap: 5pt; background: yellow }\
         .a, .b, .c { grid-row: slot-start / slot-end; height: 10pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "area row-line min-content width should include implicit columns for same-row items plus gaps: {grid:?}"
    );
    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 20.0).abs() < 0.01,
        "first generated-row-line item should use the authored column: {red:?}"
    );
    assert!(
        (green.x() - 35.0).abs() < 0.01 && (green.width() - 30.0).abs() < 0.01,
        "second generated-row-line item should use the first implicit auto column: {green:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "third generated-row-line item should use the second implicit auto column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_row_auto_flow_honors_named_row_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-auto-flow: row; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt [slot-start] 10pt [slot-end]; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a, .b, .c { grid-row: slot-start / slot-end; height: 10pt }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "authored row-line min-content width should include implicit columns for same-row items plus gaps: {grid:?}"
    );
    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 20.0).abs() < 0.01,
        "first authored-row-line item should use the authored column: {red:?}"
    );
    assert!(
        (green.x() - 35.0).abs() < 0.01 && (green.width() - 30.0).abs() < 0.01,
        "second authored-row-line item should use the first implicit auto column: {green:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "third authored-row-line item should use the second implicit auto column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_honors_positive_named_implicit_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column-start: slot 1; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "positive named implicit line should size explicit and preceding implicit columns: {grid:?}"
    );
    assert!(
        (red.x() - 70.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "positive named implicit line should place the item after the explicit and first implicit column: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_column_after_explicit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column-start: slot 1; height: 10pt; background: red }\
         .explicit { grid-column: 1; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "positive named implicit line should expand the same-page grid through the requested line: {grid:?}"
    );
    assert!(
        (blue.x() - 10.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "explicit column should remain at the authored grid start: {blue:?}"
    );
    assert!(
        (red.x() - 70.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "positive named implicit line should place the item after the explicit and first implicit column: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_forward_named_implicit_column_span_after_explicit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: 1 / span slot 1; height: 10pt; background: red }\
         .after { grid-column: 3; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"after\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 55.0).abs() < 0.01,
        "forward named implicit span should cover the explicit and first implicit column: {red:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "later implicit column should use the cycled grid-auto-columns track after the span: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_row_after_explicit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .a { grid-row-start: slot 1; background: red }\
         .explicit { grid-row: 1; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.y() + blue.height() - 170.0).abs() < 0.01 && (blue.height() - 10.0).abs() < 0.01,
        "explicit row should stay at the authored grid start: {blue:?}"
    );
    assert!(
        (red.height() - 40.0).abs() < 0.01 && (red.y() + red.height() - 120.0).abs() < 0.01,
        "positive named implicit row should place the item after the explicit and first implicit row: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_column_after_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fill, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column-start: slot 4; height: 10pt; background: red }\
         .explicit { grid-column: slot 1; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.x() - 10.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "explicit auto-fill slot should stay in the repeated grid: {blue:?}"
    );
    assert!(
        (red.x() - 120.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "positive named implicit auto-fill line should use cycled grid-auto-columns after the frozen repeat: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_row_after_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 220pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: 70pt; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fill, [slot] 20pt [end]); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .a { grid-row-start: slot 4; background: red }\
         .explicit { grid-row: slot 1; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.y() + blue.height() - 210.0).abs() < 0.01 && (blue.height() - 20.0).abs() < 0.01,
        "explicit auto-fill row slot should stay in the repeated grid: {blue:?}"
    );
    assert!(
        (red.y() + red.height() - 100.0).abs() < 0.01 && (red.height() - 40.0).abs() < 0.01,
        "positive named implicit auto-fill row should use cycled grid-auto-rows after the frozen repeat: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_column_after_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fit, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column-start: slot 4; height: 10pt; background: red }\
         .explicit { grid-column: slot 1; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.x() - 10.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "explicit auto-fit slot should stay in the first occupied repeated track: {blue:?}"
    );
    assert!(
        (red.x() - 70.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "positive named implicit auto-fit line should use cycled grid-auto-columns after the frozen repeat while empty repeated tracks collapse: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_row_after_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 220pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: 70pt; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, [slot] 20pt [end]); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .a { grid-row-start: slot 4; background: red }\
         .explicit { grid-row: slot 1; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.y() + blue.height() - 210.0).abs() < 0.01 && (blue.height() - 20.0).abs() < 0.01,
        "explicit auto-fit row slot should stay in the first occupied repeated track: {blue:?}"
    );
    assert!(
        (red.y() + red.height() - 150.0).abs() < 0.01 && (red.height() - 40.0).abs() < 0.01,
        "positive named implicit auto-fit row should use cycled grid-auto-rows after the frozen repeat while empty repeated tracks collapse: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_forward_named_implicit_column_span_after_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fill, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: slot 1 / span target 1; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap_or_else(|| panic!("red rect should paint: {:?}", page.rects()));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 105.0).abs() < 0.01,
        "forward named implicit auto-fill span should cross the repeated tracks and first cycled implicit column: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_forward_named_implicit_row_span_after_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 220pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: 70pt; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fill, [slot] 20pt [end]); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .span { grid-row: slot 1 / span target 1; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap_or_else(|| panic!("red rect should paint: {:?}", page.rects()));

    assert!(
        (red.y() + red.height() - 210.0).abs() < 0.01 && (red.height() - 105.0).abs() < 0.01,
        "forward named implicit auto-fill row span should cross the repeated tracks and first cycled implicit row: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_column_after_multi_track_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 300pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 75pt; grid-template-columns: repeat(auto-fill, [slot] 10pt [mid] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column-start: slot 3; height: 10pt; background: red }\
         .explicit { grid-column: slot 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.x() - 50.0).abs() < 0.01 && (blue.width() - 10.0).abs() < 0.01,
        "second explicit slot in a multi-track auto-fill repeat should keep its repeated track size: {blue:?}"
    );
    assert!(
        (red.x() - 125.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "positive named implicit line after multi-track auto-fill should count repeated line names and cycle auto columns beyond the explicit grid: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_positive_named_implicit_row_after_multi_track_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 260pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 20pt; height: 75pt; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fill, [slot] 10pt [mid] 20pt [end]); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .a { grid-row-start: slot 3; background: red }\
         .explicit { grid-row: slot 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (blue.y() + blue.height() - 210.0).abs() < 0.01 && (blue.height() - 10.0).abs() < 0.01,
        "second explicit slot in a multi-track auto-fill repeat should keep its repeated row size: {blue:?}"
    );
    assert!(
        (red.y() + red.height() - 135.0).abs() < 0.01 && (red.height() - 40.0).abs() < 0.01,
        "positive named implicit line after multi-track auto-fill should count repeated row line names and cycle auto rows beyond the explicit grid: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_honors_forward_named_implicit_spans() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column: 1 / span slot 1; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 55.0).abs() < 0.01,
        "forward named implicit span should cover the explicit and first implicit column: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_honors_backward_named_implicit_spans() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column: span slot / 2; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("min-content backward named implicit grid item should paint");

    assert!(
        (red.width() - 65.0).abs() < 0.01,
        "backward named implicit span should size one startward implicit column using the last grid-auto-columns track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_backward_named_implicit_spans_cycle_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column: span slot 2 / 2; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("min-content cycled backward named implicit grid item should paint");

    assert!(
        (red.width() - 100.0).abs() < 0.01,
        "backward named implicit span should cycle grid-auto-columns before the explicit grid: {red:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_explicit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .explicit { grid-column: 1; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"explicit\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 65.0).abs() < 0.01,
        "backward named implicit span should cover the startward implicit column plus the explicit column: {red:?}"
    );
    assert!(
        (blue.x() - 55.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "explicit column should be offset after the synthesized startward implicit column and gap: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_template_area_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-areas: \"main side\"; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: span slot / side-end; height: 10pt; background: red }\
         .side { grid-column: side-start / side-end; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"side\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 100.0).abs() < 0.01,
        "backward named implicit span should cover the startward implicit column plus the area-created explicit grid: {red:?}"
    );
    assert!(
        (blue.x() - 80.0).abs() < 0.01 && (blue.width() - 30.0).abs() < 0.01,
        "area-created column should be shifted after the synthesized startward implicit column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_row_span_before_template_area_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 180pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 10pt; height: min-content; grid-template-areas: \"main\" \"foot\"; grid-template-columns: 10pt; grid-template-rows: 20pt; grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .span { grid-row: span slot / foot-end; width: 10pt; background: red }\
         .foot { grid-row: foot-start / foot-end; width: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"foot\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 100.0).abs() < 0.01,
        "backward named implicit row span should cover the backward-cycled startward implicit row plus area-created explicit rows: {red:?}"
    );
    assert!(
        (blue.height() - 30.0).abs() < 0.01,
        "area-created row should be shifted after the synthesized startward implicit row and use grid-auto-rows: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_numbered_repeat_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: repeat(2, 20pt); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 65.0).abs() < 0.01,
        "backward named implicit span should synthesize a startward implicit column before a numbered repeat: {red:?}"
    );
    assert!(
        (blue.x() - 80.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "positive explicit line references should shift after the prepended implicit column before a numbered repeat: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_row_span_before_numbered_repeat_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 10pt; height: min-content; grid-template-columns: 10pt; grid-template-rows: repeat(2, 20pt); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .span { grid-row: span slot / 2; width: 10pt; background: red }\
         .second { grid-row: 2; width: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 65.0).abs() < 0.01,
        "backward named implicit row span should cover the startward implicit row plus the first explicit row: {red:?}"
    );
    assert!(
        (blue.height() - 20.0).abs() < 0.01,
        "positive explicit row references should keep the repeated explicit row size after shifting: {blue:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "second explicit row should be below the synthesized startward row span with the authored row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_row_span_before_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 10pt; height: 70pt; grid-template-columns: 10pt; grid-template-rows: repeat(auto-fill, 20pt); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .span { grid-row: span slot / 2; width: 10pt; background: red }\
         .second { grid-row: 2; width: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 65.0).abs() < 0.01,
        "backward named implicit row span should synthesize a startward implicit row before auto-fill rows: {red:?}"
    );
    assert!(
        (blue.height() - 20.0).abs() < 0.01,
        "positive explicit auto-fill row references should shift after the prepended implicit row: {blue:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "second auto-fill row should remain below the synthesized startward row span with the authored row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_auto_fill_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 280pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fill, 20pt); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 65.0).abs() < 0.01,
        "backward named implicit span should synthesize a startward implicit column before auto-fill tracks: {red:?}"
    );
    assert!(
        (blue.x() - 80.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "positive explicit auto-fill line references should shift after the prepended implicit column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_row_span_before_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 10pt; height: 70pt; grid-template-columns: 10pt; grid-template-rows: repeat(auto-fit, 20pt); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .span { grid-row: span slot / 2; width: 10pt; background: red }\
         .second { grid-row: 2; width: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 65.0).abs() < 0.01,
        "backward named implicit row span should synthesize a startward implicit row before auto-fit rows: {red:?}"
    );
    assert!(
        (blue.height() - 20.0).abs() < 0.01,
        "positive explicit auto-fit row references should shift after the prepended implicit row: {blue:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height() + 5.0)).abs() < 0.01,
        "second auto-fit row should remain below the synthesized startward row span with the authored row gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 280pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 65.0).abs() < 0.01,
        "backward named implicit span should synthesize a startward implicit column before auto-fit tracks: {red:?}"
    );
    assert!(
        (blue.x() - 80.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "positive explicit auto-fit line references should shift after the prepended implicit column: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_end_aligned_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 280pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; justify-content: end; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() + 10.0).abs() < 0.01 && (red.width() - 65.0).abs() < 0.01,
        "startward implicit span should be end-aligned after the trailing empty auto-fit track collapses: {red:?}"
    );
    assert!(
        (blue.x() - 60.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "occupied auto-fit track should align after trailing collapsed repeated tracks stop contributing to used width: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_backward_named_implicit_span_before_distributed_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 280pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; justify-content: space-evenly; background: yellow }\
         .span { grid-column: span slot / 2; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"span\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 5.0).abs() < 0.01 && (red.width() - 60.0).abs() < 0.01,
        "startward implicit span should use distribution after trailing empty auto-fit tracks collapse: {red:?}"
    );
    assert!(
        (blue.x() - 65.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "occupied auto-fit track should use the distributed collapsed-track geometry: {blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_negative_named_implicit_row_before_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 10pt; height: 70pt; grid-template-columns: 10pt; grid-template-rows: repeat(auto-fit, [slot] 20pt [end]); grid-auto-rows: 30pt 40pt; row-gap: 5pt; background: yellow }\
         .line { grid-row: slot -4; width: 10pt; background: red }\
         .second { grid-row: 2; width: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"line\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.height() - 40.0).abs() < 0.01,
        "negative named implicit row should use the backward-cycled startward implicit row before auto-fit rows: {red:?}"
    );
    assert!(
        (blue.height() - 20.0).abs() < 0.01,
        "positive explicit auto-fit row references should shift after the prepended implicit row: {blue:?}"
    );
    assert!(
        (red.y() - (blue.y() + blue.height()) - 5.0).abs() < 0.01,
        "line 2 should remain below the synthesized startward implicit row by the merged auto-fit gutter: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_layout_places_negative_named_implicit_column_before_auto_fit_grid() {
    let document = Html::from_string(
        "<style>\
         @page { size: 280pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: 70pt; grid-template-columns: repeat(auto-fit, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .line { grid-column: slot -4; height: 10pt; background: red }\
         .second { grid-column: 2; height: 10pt; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"line\"></div><div class=\"second\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 10.0).abs() < 0.01 && (red.width() - 40.0).abs() < 0.01,
        "negative named implicit column should use the backward-cycled startward implicit column before auto-fit tracks: {red:?}"
    );
    assert!(
        (blue.x() - 55.0).abs() < 0.01 && (blue.width() - 20.0).abs() < 0.01,
        "positive explicit auto-fit line references should shift after the prepended implicit column and trailing empty auto-fit tracks collapse: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_named_line_item_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: [main] min-content [main] min-content [main]; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { grid-column: main 1; background: red; white-space: nowrap }\
         .b { grid-column: main 2; background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">i</div><div class=\"b\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("min-content named-line grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first named-line intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second named-line intrinsic grid item should paint");

    assert!(
        blue.width() > red.width() * 4.0,
        "test fixture should have meaningfully different named-line contributions: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < blue.width() * 1.5,
        "named-line contribution assignment should not apply the second column contribution to every intrinsic track: grid={grid:?}, blue={blue:?}"
    );
    assert!(
        (blue.x() - (red.x() + red.width() + 5.0)).abs() < 0.01,
        "second named line track should start after the first track's contribution and gap: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_negative_line_item_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { grid-column: 1; background: red; white-space: nowrap }\
         .b { grid-column: -2; background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">i</div><div class=\"b\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("min-content negative-line grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first negative-line intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second negative-line intrinsic grid item should paint");

    assert!(
        blue.width() > red.width() * 4.0,
        "test fixture should have meaningfully different negative-line contributions: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < blue.width() * 1.5,
        "negative numeric line contribution assignment should not apply the second column contribution to every intrinsic track: grid={grid:?}, blue={blue:?}"
    );
    assert!(
        (blue.x() - (red.x() + red.width() + 5.0)).abs() < 0.01,
        "negative numeric line should resolve to the second intrinsic track: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_negative_named_line_item_contributions() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: [main] min-content [main] min-content [main]; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .a { grid-column: main 1; background: red; white-space: nowrap }\
         .b { grid-column: main -2; background: blue; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"a\">i</div><div class=\"b\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("min-content negative named-line grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first negative named-line intrinsic grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second negative named-line intrinsic grid item should paint");

    assert!(
        blue.width() > red.width() * 4.0,
        "test fixture should have meaningfully different negative named-line contributions: red={red:?}, blue={blue:?}"
    );
    assert!(
        grid.width() < blue.width() * 1.5,
        "negative named-line contribution assignment should not apply the second column contribution to every intrinsic track: grid={grid:?}, blue={blue:?}"
    );
    assert!(
        (blue.x() - (red.x() + red.width() + 5.0)).abs() < 0.01,
        "negative named line should resolve to the second intrinsic track: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_distributes_simple_spanning_contribution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: 1 / span 2; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("spanning intrinsic grid item should paint");

    assert!(
        red.width() > 40.0,
        "test fixture should have a meaningful spanning contribution: {red:?}"
    );
    assert!(
        red.width() < 100.0,
        "spanning contribution should be distributed across the two intrinsic tracks, not applied in full to each track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_distributes_backward_spanning_contribution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: span 2 / 3; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("backward spanning intrinsic grid item should paint");

    assert!(
        red.width() > 40.0,
        "test fixture should have a meaningful backward spanning contribution: {red:?}"
    );
    assert!(
        red.width() < 100.0,
        "backward spanning contribution should be distributed across the two intrinsic tracks, not applied in full to each track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_distributes_named_spanning_contribution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: [main] min-content [main] min-content [main]; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: 1 / span main 2; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("named spanning intrinsic grid item should paint");

    assert!(
        red.width() > 40.0,
        "test fixture should have a meaningful named spanning contribution: {red:?}"
    );
    assert!(
        red.width() < 100.0,
        "named spanning contribution should be distributed across the spanned intrinsic tracks, not applied in full to each track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_distributes_backward_named_spanning_contribution() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: [main] min-content [main] min-content [main]; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: span main 2 / main 3; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("backward named spanning intrinsic grid item should paint");

    assert!(
        red.width() > 40.0,
        "test fixture should have a meaningful backward named spanning contribution: {red:?}"
    );
    assert!(
        red.width() < 100.0,
        "backward named spanning contribution should be distributed across the spanned intrinsic tracks, not applied in full to each track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_template_area_generated_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-areas: \"main main\"; grid-template-columns: min-content min-content; grid-template-rows: 10pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: main-start / main-end; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("area-line intrinsic grid item should paint");

    assert!(
        red.width() > 40.0,
        "test fixture should have a meaningful generated area-line contribution: {red:?}"
    );
    assert!(
        red.width() < 100.0,
        "generated area-line contribution should be distributed across the spanned intrinsic tracks, not applied in full to each track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_template_area_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-areas: \"main main\"; grid-template-rows: 10pt; grid-auto-columns: 20pt 30pt; column-gap: 5pt; font-size: 10pt; background: yellow }\
         .span { grid-column: main-start / main-end; background: red; white-space: nowrap }\
         </style>\
         <div class=\"grid\"><div class=\"span\">MMMMMMMM</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("template-area auto-column grid item should paint");

    assert!(
        (red.width() - 55.0).abs() < 0.01,
        "area-created explicit columns should use cycled grid-auto-columns, and generated area lines should resolve across them: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_template_area_extra_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 260pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-areas: \"left right\"; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt; column-gap: 5pt; background: yellow }\
         .left { grid-column: left-start / left-end; background: red }\
         .right { grid-column: right-start / right-end; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"left\"></div><div class=\"right\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("template-area extra auto-column grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("authored template-area column item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("area-created auto-column item should paint");

    assert!(
        (grid.width() - 55.0).abs() < 0.01,
        "area-created columns beyond the authored track list should contribute grid-auto-columns to intrinsic width: {grid:?}"
    );
    assert!(
        (red.width() - 20.0).abs() < 0.01,
        "authored column should keep its explicit track size: {red:?}"
    );
    assert!(
        (blue.width() - 30.0).abs() < 0.01,
        "area-created column should use grid-auto-columns: {blue:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_one_fixed_auto_fill_repetition() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: repeat(auto-fill, 20pt); grid-template-rows: 10pt; column-gap: 5pt; background: yellow }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first auto-fill grid item should paint");

    assert!(
        (red.width() - 20.0).abs() < 0.01,
        "indefinite min-content sizing should use one fixed auto-fill repetition: {red:?}"
    );
    assert!(
        (red.x() - 10.0).abs() < 0.01,
        "first item should occupy the single intrinsic auto-fill track: {red:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_uses_one_implicit_auto_column() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-rows: 10pt; grid-auto-columns: 20pt 30pt; column-gap: 5pt; background: yellow }\
         .a { background: red }\
         .b { background: green }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));

    assert!(
        (grid.width() - 20.0).abs() < 0.01,
        "row auto-flow should create rows rather than extra implicit columns: {grid:?}"
    );
    assert!((red.width() - 20.0).abs() < 0.01, "first item: {red:?}");
}

#[tokio::test]
async fn grid_container_min_content_width_uses_positive_implicit_column_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-rows: 10pt; grid-auto-columns: 20pt 30pt; column-gap: 5pt; background: yellow }\
         .a { grid-column: 2; background: red }\
         .b { grid-column: 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let grid = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("positive implicit-line intrinsic grid background should paint");

    assert!(
        (grid.width() - 80.0).abs() < 0.01,
        "min-content grid should include implicit columns before positive numeric line placements: {grid:?}"
    );
}

#[tokio::test]
async fn grid_container_min_content_width_extends_explicit_columns_for_positive_implicit_lines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; width: min-content; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; background: yellow }\
         .a { grid-column: 2; background: red }\
         .b { grid-column: 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rect = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap_or_else(|| panic!("rect {color:?} should paint: {:?}", page.rects()))
    };
    let grid = rect(CssColor::new(255, 255, 0));
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (grid.width() - 100.0).abs() < 0.01,
        "min-content grid should include explicit and implicit columns plus gaps: {grid:?}"
    );
    assert!(
        (red.x() - 35.0).abs() < 0.01 && (red.width() - 30.0).abs() < 0.01,
        "first implicit column should use the first grid-auto-columns track: {red:?}"
    );
    assert!(
        (blue.x() - 70.0).abs() < 0.01 && (blue.width() - 40.0).abs() < 0.01,
        "second implicit column should use the second grid-auto-columns track: {blue:?}"
    );
}

#[tokio::test]
async fn inline_grid_paints_atomically_and_exports_item_baseline() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 20pt }\
         .grid { display: inline-grid; grid-template-columns: 24pt; grid-template-rows: 20pt; background: yellow }\
         .item { font-size: 20pt; line-height: 20pt; background: red }\
         </style>\
         a <span class=\"grid\"><span class=\"item\">b</span></span> c",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", page.lines()))
    };
    let a = line("a");
    let b = line("b");
    let yellow = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .expect("inline-grid background should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("inline-grid item background should paint");

    assert!(
        yellow.x() > first_visible_glyph_x(a),
        "inline-grid should participate at an inline position: a={a:?}, grid={yellow:?}"
    );
    assert!(
        (red.x() - yellow.x()).abs() < 0.01,
        "grid item should paint inside the inline-grid atom: grid={yellow:?}, item={red:?}"
    );
    assert!(
        (b.y() - a.y()).abs() < 0.01,
        "inline-grid should export the grid item baseline: a={}, b={}",
        a.y(),
        b.y()
    );
}

#[tokio::test]
async fn inline_grid_exports_first_occupied_row_baseline_not_paint_order() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 20pt }\
         .grid { display: inline-grid; grid-template-columns: 40pt; grid-template-rows: 20pt 20pt; background: yellow }\
         .low { grid-row: 2; font-size: 20pt; line-height: 20pt }\
         .top { grid-row: 1; font-size: 10pt; line-height: 10pt }\
         </style>\
         a <span class=\"grid\"><span class=\"low\">Low</span><span class=\"top\">Top</span></span> z",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", page.lines()))
    };
    let parent = line("a");
    let top = line("Top");
    let low = line("Low");

    assert!(
        (top.y() - parent.y()).abs() < 0.01,
        "inline-grid should export the first occupied grid row baseline: parent={}, top={}",
        parent.y(),
        top.y()
    );
    assert!(
        (low.y() - parent.y()).abs() > 5.0,
        "inline-grid baseline should not come from the first painted/source item: parent={}, low={}",
        parent.y(),
        low.y()
    );
}

#[tokio::test]
async fn grid_justify_items_aligns_item_inside_grid_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; grid-template-columns: 40pt; grid-template-rows: 10pt; width: 40pt; justify-items: end }\
         .item { width: 10pt; height: 10pt; background: red }\
         </style>\
         <div class=\"grid\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("aligned grid item background should paint");

    assert!((rect.x() - 40.0).abs() < 0.01, "item: {rect:?}");
    assert!((rect.width() - 10.0).abs() < 0.01, "item: {rect:?}");
}

#[tokio::test]
async fn grid_align_items_baseline_aligns_same_row_text_baselines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; align-items: baseline; grid-template-columns: 70pt 70pt; grid-template-rows: 40pt }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt; align-self: first baseline }\
         </style>\
         <div class=\"grid\"><p class=\"big\">Big</p><p class=\"small\">Small</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines()
        .iter()
        .find(|line| line.text == "Big")
        .unwrap_or_else(|| panic!("Big should render: {:?}", page.lines()));
    let small = page
        .lines()
        .iter()
        .find(|line| line.text == "Small")
        .unwrap_or_else(|| panic!("Small should render: {:?}", page.lines()));

    assert!(
        (big.y() - small.y()).abs() < 0.01,
        "grid baseline-aligned items should share measured text baselines: big={}, small={}",
        big.y(),
        small.y()
    );
}

#[tokio::test]
async fn grid_align_items_last_baseline_aligns_same_row_text_baselines() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 120pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; align-items: last baseline; grid-template-columns: 70pt 70pt; grid-template-rows: 50pt }\
         p { margin: 0 }\
         .big { font-size: 30pt; line-height: 30pt }\
         .small { font-size: 10pt; line-height: 10pt; align-self: last baseline }\
         </style>\
         <div class=\"grid\"><p class=\"big\">Big</p><p class=\"small\">First<br>Last</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let big = page
        .lines()
        .iter()
        .find(|line| line.text == "Big")
        .unwrap_or_else(|| panic!("Big should render: {:?}", page.lines()));
    let first = page
        .lines()
        .iter()
        .find(|line| line.text == "First")
        .unwrap_or_else(|| panic!("First should render: {:?}", page.lines()));
    let last = page
        .lines()
        .iter()
        .find(|line| line.text == "Last")
        .unwrap_or_else(|| panic!("Last should render: {:?}", page.lines()));

    assert!(
        (big.y() - last.y()).abs() < 0.01,
        "grid last-baseline items should share measured last text baselines: big={}, last={}",
        big.y(),
        last.y()
    );
    assert!(
        (first.y() - big.y()).abs() > 5.0,
        "grid last-baseline alignment should use the final line, not the first: first={}, big={}",
        first.y(),
        big.y()
    );
}

#[tokio::test]
async fn grid_baseline_alignment_groups_items_sharing_start_row_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; align-items: baseline; grid-template-columns: 80pt 80pt; grid-template-rows: 20pt 30pt }\
         p { margin: 0 }\
         .span { grid-row: 1 / 3; font-size: 30pt; line-height: 30pt }\
         .peer { grid-row: 1; align-self: first baseline; font-size: 10pt; line-height: 10pt }\
         </style>\
         <div class=\"grid\"><p class=\"span\">Span</p><p class=\"peer\">Peer</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let span = page
        .lines()
        .iter()
        .find(|line| line.text == "Span")
        .unwrap_or_else(|| panic!("Span should render: {:?}", page.lines()));
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap_or_else(|| panic!("Peer should render: {:?}", page.lines()));

    assert!(
        (span.y() - peer.y()).abs() < 0.01,
        "spanning and non-spanning grid items with the same start row edge should share first baselines: span={}, peer={}",
        span.y(),
        peer.y()
    );
}

#[tokio::test]
async fn grid_last_baseline_alignment_groups_items_sharing_end_row_edge() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; align-items: last baseline; grid-template-columns: 80pt 80pt; grid-template-rows: 20pt 30pt }\
         p { margin: 0 }\
         .span { grid-row: 1 / 3; font-size: 10pt; line-height: 10pt }\
         .peer { grid-row: 2 / 3; align-self: last baseline; font-size: 30pt; line-height: 30pt }\
         </style>\
         <div class=\"grid\"><p class=\"span\">Top<br>Last</p><p class=\"peer\">Peer</p></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let top = page
        .lines()
        .iter()
        .find(|line| line.text == "Top")
        .unwrap_or_else(|| panic!("Top should render: {:?}", page.lines()));
    let last = page
        .lines()
        .iter()
        .find(|line| line.text == "Last")
        .unwrap_or_else(|| panic!("Last should render: {:?}", page.lines()));
    let peer = page
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap_or_else(|| panic!("Peer should render: {:?}", page.lines()));

    assert!(
        (last.y() - peer.y()).abs() < 0.01,
        "spanning and non-spanning grid items with the same end row edge should share last baselines: last={}, peer={}",
        last.y(),
        peer.y()
    );
    assert!(
        (top.y() - peer.y()).abs() > 5.0,
        "last-baseline spanning group should use the spanning item's last line, not first: top={}, peer={}",
        top.y(),
        peer.y()
    );
}

#[tokio::test]
async fn grid_last_baseline_alignment_uses_nested_grid_last_row_baseline() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 140pt; margin: 10pt }\
         body { margin: 0 }\
         .outer { display: grid; align-items: last baseline; grid-template-columns: 80pt 80pt; grid-template-rows: 70pt }\
         .nested { display: grid; align-self: last baseline; grid-template-columns: 60pt; grid-template-rows: 20pt 30pt; background: yellow }\
         .top { grid-row: 1; font-size: 10pt; line-height: 10pt }\
         .last { grid-row: 2; font-size: 20pt; line-height: 20pt }\
         .peer { align-self: last baseline; font-size: 20pt; line-height: 20pt; background: blue }\
         </style>\
         <div class=\"outer\"><div class=\"nested\"><span class=\"top\">Top</span><span class=\"last\">Last</span></div><div class=\"peer\">Peer</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let line = |text: &str| {
        page.lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render: {:?}", page.lines()))
    };
    let top = line("Top");
    let last = line("Last");
    let peer = line("Peer");

    assert!(
        (last.y() - peer.y()).abs() < 0.01,
        "parent grid last-baseline alignment should use the nested grid's last row baseline: last={}, peer={}",
        last.y(),
        peer.y()
    );
    assert!(
        (top.y() - peer.y()).abs() > 10.0,
        "nested grid should not export its first row baseline for last-baseline alignment: top={}, peer={}",
        top.y(),
        peer.y()
    );
}

#[tokio::test]
async fn absolute_grid_child_uses_grid_static_position_without_participating() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned grid child should paint");
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("first normal grid item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal grid item should paint");

    assert!((red.x() - 25.0).abs() < 0.01, "red item: {red:?}");
    assert!((blue.x() - 50.0).abs() < 0.01, "blue item: {blue:?}");
    assert!((green.x() - 50.0).abs() < 0.01, "abspos item: {green:?}");
    assert!((green.width() - 8.0).abs() < 0.01, "abspos item: {green:?}");
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_auto_fill_track() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fill, 20pt); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: yellow }\
         .c { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div><div class=\"c\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned auto-fill grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("third normal auto-fill grid item should paint");

    assert!(
        (blue.x() - 75.0).abs() < 0.01,
        "third auto-fill track should start after two fixed tracks and gaps: {blue:?}"
    );
    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the third auto-filled track start: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_auto_fill_column_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fill, [slot] 20pt [end]); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: slot 3; width: 8pt; height: 8pt; background: green }\
         .peer { grid-column: slot 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned named auto-fill grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("named auto-fill peer should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the third named auto-fill column line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_multi_track_auto_fill_column_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fill, [slot] 10pt [middle] 10pt [end]); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: middle 2; width: 8pt; height: 8pt; background: green }\
         .peer { grid-column: middle 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned multi-track auto-fill grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("multi-track named auto-fill peer should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use a named line inside a multi-track auto-fill fragment: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_auto_fill_after_implicit_column_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fill, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: slot 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned after-implicit auto-fill grid child should paint");

    assert!(
        (green.x() - 130.0).abs() < 0.01,
        "abspos item should use the first after-explicit implicit named line after auto-fill tracks: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_auto_fill_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fill, [slot] 20pt [end]); row-gap: 5pt; align-content: end; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 3; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: slot 3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned named auto-fill row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("named auto-fill row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos item should use the third named auto-fill row line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_auto_fill_after_implicit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 200pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fill, [slot] 20pt [end]); grid-auto-rows: 30pt; row-gap: 5pt; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned after-implicit auto-fill row grid child should paint");

    assert!(
        ((green.y() + green.height()) - 85.0).abs() < 0.01,
        "abspos item should use the first after-explicit implicit named row line after auto-fill tracks: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_collapsed_auto_fit_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fit, 20pt); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal auto-fit grid item should paint");

    assert!(
        (green.x() - (blue.x() + blue.width())).abs() < 0.01,
        "abspos item should use the collapsed auto-fit line after the last occupied track: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_collapsed_auto_fit_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fit, [slot] 20pt [end]); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned named auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal named auto-fit grid item should paint");

    assert!(
        (green.x() - (blue.x() + blue.width())).abs() < 0.01,
        "abspos item should use the third named auto-fit line after empty-track collapse: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_multi_track_collapsed_auto_fit_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fit, [slot] 10pt [middle] 10pt [end]); grid-template-rows: 10pt; column-gap: 5pt; width: 70pt }\
         .abs { position: absolute; grid-column: middle 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned multi-track auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second occupied multi-track auto-fit item should paint");

    assert!(
        (green.x() - (blue.x() + blue.width())).abs() < 0.01,
        "abspos item should use the collapsed named line inside a multi-track auto-fit fragment: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_collapsed_auto_fit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, 20pt); row-gap: 5pt; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned row auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal row auto-fit grid item should paint");

    assert!(
        ((green.y() + green.height()) - blue.y()).abs() < 0.01,
        "abspos item should use the collapsed auto-fit row line after the last occupied track: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_multi_track_collapsed_auto_fit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, [slot] 10pt [middle] 10pt [end]); row-gap: 5pt; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: middle 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned multi-track row auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second occupied multi-track row auto-fit item should paint");

    assert!(
        ((green.y() + green.height()) - blue.y()).abs() < 0.01,
        "abspos item should use the collapsed named row line inside a multi-track auto-fit fragment: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_collapsed_auto_fit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, [slot] 20pt [end]); row-gap: 5pt; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned named row auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal named row auto-fit grid item should paint");

    assert!(
        ((green.y() + green.height()) - blue.y()).abs() < 0.01,
        "abspos item should use the third named auto-fit row line after empty-track collapse: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_content_aligned_auto_fit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, [slot] 20pt [end]); row-gap: 5pt; align-content: end; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned content-aligned row auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal content-aligned row auto-fit grid item should paint");

    assert!(
        (green.y() - (blue.y() - green.height())).abs() < 0.01,
        "abspos item should include align-content:end alignment when using the collapsed named auto-fit row line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_distributed_auto_fit_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(auto-fit, [slot] 20pt [end]); row-gap: 5pt; align-content: space-evenly; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned distributed row auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal distributed row auto-fit grid item should paint");

    assert!(
        (green.y() - (blue.y() - green.height())).abs() < 0.01,
        "abspos item should include distributed align-content when using the collapsed named auto-fit row line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_content_aligned_auto_fit_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fit, [slot] 20pt [end]); grid-template-rows: 10pt; column-gap: 5pt; justify-content: end; width: 70pt }\
         .abs { position: absolute; grid-column: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned content-aligned auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal content-aligned auto-fit grid item should paint");

    assert!(
        (green.x() - (blue.x() + blue.width())).abs() < 0.01,
        "abspos item should include justify-content:end alignment when using the collapsed named auto-fit line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_distributed_auto_fit_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fit, [slot] 20pt [end]); grid-template-rows: 10pt; column-gap: 5pt; justify-content: space-evenly; width: 70pt }\
         .abs { position: absolute; grid-column: slot 3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned distributed auto-fit grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal distributed auto-fit grid item should paint");

    assert!(
        (green.x() - (blue.x() + blue.width())).abs() < 0.01,
        "abspos item should include distributed justify-content alignment when using the collapsed named auto-fit line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_content_aligned_fixed_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; justify-content: end; width: 70pt }\
         .abs { position: absolute; grid-column: 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { grid-column: 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned content-aligned fixed-line grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second content-aligned fixed-line grid item should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should include justify-content:end when resolving a fixed explicit grid line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_left_offset_resolves_against_grid_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 700pt 650pt; margin: 0 }\
         body { margin: 0 }\
         #grid { display: grid; grid: 150pt 100pt / 200pt 300pt; margin: 1pt 2pt 3pt 4pt; padding: 20pt 15pt 10pt 5pt; border-width: 9pt 3pt 12pt 6pt; border-style: solid; width: 550pt; height: 400pt; position: relative }\
         #grid > div { position: absolute; left: 0 }\
         #firstItem { background: magenta; grid-column: 1 / 2; grid-row: 1 / 2 }\
         #secondItem { background: cyan; grid-column: 2 / 3; grid-row: 1 / 2 }\
         #thirdItem { background: yellow; grid-column: 1 / 2; grid-row: 2 / 3 }\
         #fourthItem { background: lime; grid-column: 2 / 3; grid-row: 2 / 3 }\
         </style>\
         <div id=\"grid\"><div id=\"firstItem\">First item</div><div id=\"secondItem\">Second item</div><div id=\"thirdItem\">Third item</div><div id=\"fourthItem\">Fourth item</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let item = |color| {
        page.rects()
            .iter()
            .find(|rect| rect.fill == Some(color) && rect.width() > 0.0 && rect.height() > 0.0)
            .unwrap_or_else(|| panic!("expected visible rect for color {color:?}"))
    };
    let first = item(CssColor::new(255, 0, 255));
    let second = item(CssColor::new(0, 255, 255));
    let third = item(CssColor::new(255, 255, 0));
    let fourth = item(CssColor::new(0, 255, 0));

    assert!(
        (second.x() - first.x() - 200.0).abs() < 0.01,
        "left offset should resolve from each column grid area: first={first:?}, second={second:?}"
    );
    assert!(
        (fourth.x() - third.x() - 200.0).abs() < 0.01,
        "left offset should resolve from each column grid area in the second row: third={third:?}, fourth={fourth:?}"
    );
}

#[tokio::test]
async fn absolute_grid_area_excludes_the_following_gutter_in_ltr_and_rtl() {
    for (direction, expected_x) in [("ltr", 10.0), ("rtl", 70.0)] {
        let document = Html::from_string(format!(
            "<style>\
             @page {{ size: 220pt 100pt; margin: 10pt }}\
             body {{ margin: 0 }}\
             .grid {{ display: grid; position: relative; direction: {direction}; grid-template-columns: 100pt 40pt; grid-template-rows: 10pt; column-gap: 20pt; width: 160pt }}\
             .abs {{ position: absolute; grid-column: 1 / 2; left: 0; right: 0; height: 8pt; background: green }}\
             </style>\
             <div class=\"grid\"><div class=\"abs\"></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let green = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .expect("absolutely positioned grid child should paint");
        assert!(
            (green.x() - expected_x).abs() < 0.01,
            "{direction} area should start at its physical track edge: {green:?}"
        );
        assert!(
            (green.width() - 100.0).abs() < 0.01,
            "{direction} area must exclude its following gutter: {green:?}"
        );
    }
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_positive_implicit_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; justify-content: end; width: 120pt }\
         .abs { position: absolute; grid-column: 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned positive implicit-line grid child should paint");

    assert!(
        (green.x() - 145.0).abs() < 0.01,
        "abspos positive implicit line should use cycled grid-auto-columns and content alignment: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_after_explicit_line_omits_following_gutter() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; width: 120pt }\
         .abs { position: absolute; grid-column: 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned after-explicit grid child should paint");

    assert!(
        (green.x() - 125.0).abs() < 0.01,
        "after-explicit static line should be after the implicit track, not after the following gutter: {green:?}"
    );
}

#[tokio::test]
async fn rtl_absolute_grid_child_static_position_uses_grid_area_end_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 100pt; margin: 10pt }\
         body { margin: 0; direction: rtl }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(auto-fill, [slot] 20pt [end]); grid-template-rows: 10pt; grid-auto-columns: 30pt; column-gap: 5pt; width: 70pt; direction: rtl }\
         .abs { position: absolute; grid-column: slot 3 / slot 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("rtl absolutely positioned grid child should paint");

    assert!(
        (green.x() - 172.0).abs() < 0.01,
        "rtl abspos auto insets should anchor to the grid-area end line, not the start-line probe width: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_positive_named_implicit_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: [main] 20pt [main]; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; justify-content: end; width: 120pt }\
         .abs { position: absolute; grid-column: main 4; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned positive named implicit-line grid child should paint");

    assert!(
        (green.x() - 145.0).abs() < 0.01,
        "abspos positive named implicit line should assume after-explicit implicit lines have the requested name and use cycled grid-auto-columns: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_numbered_repeat_named_column_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: repeat(2, [slot] 20pt [end]); grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: slot 2; width: 8pt; height: 8pt; background: green }\
         .peer { grid-column: slot 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned numbered-repeat named-column grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("numbered-repeat named-column peer should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the named line inside a finite repeat for its column static position: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_numbered_repeat_named_row_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 160pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: repeat(2, [slot] 20pt [end]); row-gap: 5pt; align-content: end; width: 20pt; height: 70pt }\
         .abs { position: absolute; grid-row: slot 2; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: slot 2; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned numbered-repeat named-row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("numbered-repeat named-row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos item should use the named line inside a finite repeat for its row static position: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_positive_implicit_auto_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: 20pt; grid-auto-rows: 30pt 40pt; row-gap: 5pt; align-content: end; width: 20pt; height: 120pt }\
         .abs { position: absolute; grid-row: 4; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: 4; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned positive implicit-row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("positive implicit-row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos positive implicit row should use cycled grid-auto-rows and align-content:end: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_positive_named_implicit_auto_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: [main] 20pt [main]; grid-auto-rows: 30pt 40pt; row-gap: 5pt; align-content: end; width: 20pt; height: 120pt }\
         .abs { position: absolute; grid-row: main 4; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: 4; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned positive named implicit-row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("positive named implicit-row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos positive named implicit row should assume after-explicit implicit rows have the requested name and use cycled grid-auto-rows: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_negative_named_implicit_auto_columns() {
    let document = Html::from_string(
        "<style>\
         @page { size: 220pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: [main] 20pt [main]; grid-template-rows: 10pt; grid-auto-columns: 30pt 40pt; column-gap: 5pt; justify-content: end; width: 120pt }\
         .abs { position: absolute; grid-column: main -3; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned negative named implicit-line grid child should paint");

    assert!(
        (green.x() - 80.0).abs() < 0.01,
        "abspos negative named implicit line should assume before-explicit implicit lines have the requested name and use backward-cycled grid-auto-columns: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_negative_implicit_auto_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: 20pt; grid-auto-rows: 30pt 40pt; row-gap: 5pt; align-content: end; width: 20pt; height: 120pt }\
         .abs { position: absolute; grid-row: -3; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: -3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned negative implicit-row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("negative implicit-row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos negative implicit row should use backward-cycled grid-auto-rows and align-content:end: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_negative_named_implicit_auto_rows() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 180pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; grid-template-columns: 20pt; grid-template-rows: [main] 20pt [main]; grid-auto-rows: 30pt 40pt; row-gap: 5pt; align-content: end; width: 20pt; height: 120pt }\
         .abs { position: absolute; grid-row: main -3; width: 8pt; height: 8pt; background: green }\
         .peer { grid-row: main -3; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"peer\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned negative named implicit-row grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("negative named implicit-row peer should paint");

    assert!(
        ((green.y() + green.height()) - (blue.y() + blue.height())).abs() < 0.01,
        "abspos negative named implicit row should assume before-explicit implicit rows have the requested name and use backward-cycled grid-auto-rows: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_named_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: [main] 20pt [main] 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: main 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned named-line grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal grid item should paint");

    assert!((blue.x() - 50.0).abs() < 0.01, "blue item: {blue:?}");
    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use second named line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn inline_source_absolute_grid_child_uses_inline_static_position() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><span class=\"abs\"></span><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("inline-source absolutely positioned grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal grid item should paint");

    assert!((blue.x() - 50.0).abs() < 0.01, "blue item: {blue:?}");
    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "inline-source abspos grid child should use inline static grid replay: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_negative_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: -1; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned negative-line grid child should paint");

    assert!(
        (green.x() - 70.0).abs() < 0.01,
        "abspos item should use the explicit grid end line: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_negative_named_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: [main] 20pt [main] 20pt [main]; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: main -1; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned negative named-line grid child should paint");

    assert!(
        (green.x() - 70.0).abs() < 0.01,
        "abspos item should use the last named explicit grid line: {green:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_template_area() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-areas: \"left right\"; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-area: right; width: 8pt; height: 8pt; background: green }\
         .left { grid-area: left; background: red }\
         .right { grid-area: right; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"left\"></div><div class=\"right\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned template-area grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("normal grid area child should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the named template area's column start: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_template_area_generated_line() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-areas: \"left right\"; grid-template-columns: 20pt 20pt; grid-template-rows: 10pt; column-gap: 5pt; width: 45pt }\
         .abs { position: absolute; grid-column: right-start; width: 8pt; height: 8pt; background: green }\
         .left { grid-area: left; background: red }\
         .right { grid-area: right; background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"left\"></div><div class=\"right\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned generated-line grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("normal grid area child should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the generated template-area line: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_flexible_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 200pt 100pt; margin: 10pt }\
         body { margin: 0 }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: 1fr 1fr; grid-template-rows: 10pt; width: 100pt }\
         .abs { position: absolute; grid-column: 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned flexible-track grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal grid item should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the second flexible track start: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn absolute_grid_child_static_position_uses_intrinsic_tracks() {
    let document = Html::from_string(
        "<style>\
         @page { size: 240pt 120pt; margin: 10pt }\
         body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .grid { display: grid; position: relative; margin-left: 15pt; grid-template-columns: max-content max-content; grid-template-rows: 10pt; column-gap: 5pt; width: 120pt }\
         .abs { position: absolute; grid-column: 2; width: 8pt; height: 8pt; background: green }\
         .a { background: red }\
         .b { background: blue }\
         </style>\
         <div class=\"grid\"><div class=\"abs\"></div><div class=\"a\">wide text</div><div class=\"b\">xx</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolutely positioned intrinsic-track grid child should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second normal grid item should paint");

    assert!(
        (green.x() - blue.x()).abs() < 0.01,
        "abspos item should use the second intrinsic track start: green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn rtl_fixed_width_flex_container_uses_physical_right_edge() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 100pt; margin: 10pt } body { margin: 0; direction: rtl }\
         .flex { display: flex; width: 100pt; height: 10pt; margin: 0; background: red }\
         </style><div class=\"flex\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let flex_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex background should paint");
    assert!(
        (flex_background.x() - 90.0).abs() < 0.01,
        "fixed-width flex container in RTL should align to the containing block's right edge: {flex_background:?}"
    );
}

#[tokio::test]
async fn flex_row_space_between_single_item_falls_back_to_start() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 500pt 220pt; margin: 10pt } body { margin: 0 }\
         div { background: blue; margin: 1em 0; border: 1px solid black; height: 8em; width: 30em; display: flex; justify-content: space-between }\
         span { background: white; margin: 1em; width: 5em; max-width: 6em; display: inline-block; flex: 1 0 0% }</style>\
         <div><span>one</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let white = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 255)))
        .expect("single flex item background should paint");
    assert!(
        (white.x() - 22.75).abs() < 0.01,
        "space-between single item should use flex-start fallback: {white:?}"
    );
    assert!(
        (white.width() - 72.0).abs() < 0.01,
        "flex item should be clamped by max-width: {white:?}"
    );
}

#[tokio::test]
async fn flex_row_single_item_space_around_and_evenly_fall_back_to_center() {
    for justify_content in ["space-around", "space-evenly"] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 240pt 100pt; margin: 10pt }} body {{ margin: 0 }}\
             .row {{ display:flex; justify-content:{justify_content}; width:200pt }}\
             .item {{ width:40pt; height:10pt; background:green }}\
             </style><div class=\"row\"><div class=\"item\"></div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let green = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap();
        assert!(
            (green.x() - 90.0).abs() < 0.01,
            "{justify_content} single item should center: {green:?}"
        );
    }
}

#[tokio::test]
async fn flex_column_rejustifies_after_replaced_auto_minimum_growth() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 140pt 140pt; margin: 10pt }} body {{ margin: 0 }}\
         .col {{ display:flex; flex-direction:column; justify-content:space-around; width:75pt; height:100pt }}\
         img {{ width:75pt; flex:0 1 0% }}\
         </style><div class=\"col\"><img src=\"{image}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let image = document.pages[0]
        .images()
        .iter()
        .find(|image| !image.background)
        .expect("replaced flex item should paint");
    assert!((image.height() - 75.0).abs() < 0.01, "image={image:?}");
    assert!(
        (image.y() - 42.5).abs() < 0.01,
        "space-around should center the item after automatic minimum growth: {image:?}"
    );
}

#[tokio::test]
async fn flex_basis_min_content_counts_inline_atoms() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; width: 100pt; font-size: 10pt; line-height: 12pt }\
         .item { flex: 0 0 min-content; background: red }\
         .atom { display: inline-block; width: 34pt; height: 4pt; margin-left: 4pt }</style>\
         <div class=\"row\"><div class=\"item\">A<span class=\"atom\"></span></div><div>B</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.width() >= 37.5,
        "flex min-content basis should include the inline atom: {item:?}"
    );
}

#[tokio::test]
async fn flex_max_content_uses_graph_generated_inline_edges_and_atoms() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 180pt }\
         .item { flex: 0 0 max-content; background: red }\
         .item::before { content: 'XX' }\
         .edge { padding-left: 20pt; padding-right: 10pt; border-left: 5pt solid transparent; border-right: 5pt solid transparent; text-transform: uppercase }\
         .atom { display: inline-block; width: 30pt; height: 4pt }</style>\
         <div class=\"row\"><div class=\"item\"><span class=\"edge\">z</span><span class=\"atom\"></span></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.width() > 68.0,
        "flex max-content should include generated text, inline edges, and the atom: {item:?}"
    );
}

#[tokio::test]
async fn anonymous_flex_text_preserves_graph_measured_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt; white-space: break-spaces }\
         .row { display: flex; width: 200pt }\
         .marker { width: 20pt; height: 10pt; background: green }</style>\
         <div class=\"row\">A     B<div class=\"marker\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let marker = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    assert!(
        marker.x() > 35.0,
        "anonymous flex text should reserve preserved spaces before the marker: {marker:?}"
    );
}

#[tokio::test]
async fn column_flex_min_content_height_uses_graph_selected_atom_lines() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .column { display: flex; flex-direction: column; width: 25pt; height: 80pt }\
         .item { max-height: min-content; flex-basis: 80pt; background: green }\
         .atom { display: inline-block; width: 20pt; height: 10pt }</style>\
         <div class=\"column\"><div class=\"item\"><span class=\"atom\"></span><span class=\"atom\"></span></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    assert!(
        item.height() > 19.0 && item.height() < 31.0,
        "max-height:min-content should clamp to the two graph-selected atom lines: {item:?}"
    );
}

#[tokio::test]
async fn nested_flex_intrinsics_use_styled_inline_graph_contributions() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 220pt }\
         .nested { display: flex; flex: 0 0 auto; background: red }\
         .item { flex: 0 0 max-content }\
         .item::before { content: 'AA' }\
         .styled { letter-spacing: 4pt; padding-left: 12pt; border-left: 4pt solid transparent; text-transform: uppercase }\
         .atom { display: inline-block; width: 18pt; height: 6pt }</style>\
         <div class=\"row\"><div class=\"nested\"><div class=\"item\"><span class=\"styled\">bb</span><span class=\"atom\"></span></div></div><div>Tail</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let nested = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        nested.width() > 48.0,
        "nested flex intrinsic width should include generated text, styling, and atoms: {nested:?}"
    );
}

#[tokio::test]
async fn flex_min_content_block_size_uses_wrapped_graph_fragments() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt }\
         .column { display: flex; flex-direction: column; width: 31pt; height: 100pt }\
         .item { max-height: min-content; flex-basis: 100pt; background: green }\
         .word { letter-spacing: 2pt }</style>\
         <div class=\"column\"><div class=\"item\"><span class=\"word\">AB</span> <span class=\"word\">CD</span> <span class=\"word\">EF</span></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    assert!(
        item.height() > 23.0 && item.height() < 40.0,
        "min-content block-size should come from graph-selected wrapped line fragments: {item:?}"
    );
}

#[tokio::test]
async fn direct_inline_replaced_row_height_uses_graph_atomic_metrics() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt }\
         .row { display: flex; width: 120pt }\
         .item { flex: 0 0 auto; background: red }\
         .atom { display: inline-block; width: 8pt; height: 32pt }</style>\
         <div class=\"row\"><div class=\"item\"><svg width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\" fill=\"blue\" /></svg><span class=\"atom\"></span></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        item.height() > 31.0,
        "direct inline replaced rows should use graph atom metrics for height: {item:?}"
    );
}

#[tokio::test]
async fn justify_content_left_uses_physical_left_in_row_reverse() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; flex-direction:row-reverse; justify-content:left; width:200pt; height:30pt }\
         .item { width:30pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 10.0).abs() < 0.01, "blue={blue:?}");
    assert!((green.x() - 40.0).abs() < 0.01, "green={green:?}");
    assert!((red.x() - 70.0).abs() < 0.01, "red={red:?}");
}

#[tokio::test]
async fn justify_content_end_uses_logical_end_in_rtl_row_reverse() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; direction:rtl; flex-direction:row-reverse; justify-content:end; width:200pt; height:30pt }\
         .item { width:30pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((red.x() - 10.0).abs() < 0.01, "red={red:?}");
    assert!((green.x() - 40.0).abs() < 0.01, "green={green:?}");
    assert!((blue.x() - 70.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn justify_content_physical_left_right_fall_back_to_start_on_column_axis() {
    for justify_content in ["left", "right"] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 180pt 160pt; margin: 10pt }} body {{ margin:0 }}\
             .col {{ display:flex; flex-direction:column-reverse; justify-content:{justify_content}; width:100pt; height:80pt }}\
             .item {{ width:30pt; height:20pt }}\
             </style><div class=\"col\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:green\"></div><div class=\"item\" style=\"background:blue\"></div></div>",
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let red = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .unwrap();
        let green = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap();
        let blue = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .unwrap();

        assert!(
            (blue.y() - 130.0).abs() < 0.01,
            "{justify_content}: blue={blue:?}"
        );
        assert!(
            (green.y() - 110.0).abs() < 0.01,
            "{justify_content}: green={green:?}"
        );
        assert!(
            (red.y() - 90.0).abs() < 0.01,
            "{justify_content}: red={red:?}"
        );
    }
}

#[tokio::test]
async fn adjacent_flex_container_vertical_margins_collapse_as_block_siblings() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt } body { margin:0 }\
         .flex { display:flex; width:40pt; height:20pt; margin:10pt 0; background:blue }\
         </style><div class=\"flex\"></div><div class=\"flex\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .collect::<Vec<_>>();

    assert_eq!(blue_rects.len(), 2);
    let gap = blue_rects[0].y() - (blue_rects[1].y() + blue_rects[1].height());
    assert!(
        (gap - 10.0).abs() < 0.01,
        "adjacent sibling margins should collapse to 10pt, not add to 20pt: {blue_rects:?}"
    );
}

#[tokio::test]
async fn supports_flex_column_space_around() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } .col { display:flex; flex-direction:column; justify-content:space-around; height:100pt }</style><div class=\"col\"><span>A</span><span>B</span></div>",
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

    let a_top = rendered_line_baseline_top(&document, a);
    let b_top = rendered_line_baseline_top(&document, b);
    assert!((a_top - 130.0).abs() < 1.0, "A top={a_top}");
    assert!((b_top - 80.0).abs() < 1.0, "B top={b_top}");
}

#[tokio::test]
async fn column_flex_overflow_hidden_clips_centered_item_border_box() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } .flex { display:flex; flex-direction:column; align-items:center; overflow:hidden; width:70pt; height:70pt } .big { background:blue; width:10pt; border:solid coral; border-width:2pt 50pt; flex:3 } .small { background:teal; width:20pt; flex:1 }</style>\
         <div class=\"flex\"><div class=\"big\"></div><div class=\"small\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let coral = CssColor::new(255, 127, 80);
    let coral_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(coral))
        .collect::<Vec<_>>();

    assert!(!coral_rects.is_empty());
    assert!(
        coral_rects
            .iter()
            .all(|rect| rect.x() >= 10.0 && rect.x() + rect.width() <= 80.0)
    );
    assert!(
        coral_rects
            .iter()
            .any(|rect| (rect.x() - 10.0).abs() < 0.01 && (rect.width() - 30.0).abs() < 0.01)
    );
    assert!(
        coral_rects
            .iter()
            .any(|rect| (rect.x() - 50.0).abs() < 0.01 && (rect.width() - 30.0).abs() < 0.01)
    );
}

#[tokio::test]
async fn align_self_self_end_uses_item_writing_mode_on_row_cross_axis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:inline-flex; height:100pt; border:1pt dashed blue; vertical-align:top }\
         .item { width:30pt; height:20pt; margin:1pt 2pt 3pt 4pt; border:2pt dotted black; padding:3pt; }\
         .self-start { align-self:self-start; background:yellow }\
         .self-end { align-self:self-end; writing-mode:vertical-lr; direction:rtl; background:purple }\
         </style><div class=\"row\"><div class=\"item self-start\"></div><div class=\"item self-end\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .unwrap();
    let purple = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .unwrap();

    assert!(
        (yellow.y() - purple.y()).abs() < 0.01,
        "vertical-rl/rtl self-end should align its inline-end/top side to the flex row cross-start: yellow={yellow:?}, purple={purple:?}"
    );
}

#[tokio::test]
async fn align_self_self_end_can_target_row_cross_end_from_item_writing_mode() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; height:80pt; width:100pt }\
         .item { width:20pt; height:20pt; margin:0 }\
         .reference { align-self:flex-end; background:green }\
         .target { align-self:self-end; writing-mode:vertical-lr; direction:ltr; background:red }\
         </style><div class=\"row\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.y() - target.y()).abs() < 0.01,
        "vertical-lr/ltr self-end should align its inline-end/bottom side to row cross-end: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn align_self_self_end_uses_item_writing_mode_on_column_cross_axis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .column { display:flex; flex-direction:column; width:80pt; height:80pt }\
         .item { width:20pt; height:20pt; margin:0 }\
         .reference { align-self:flex-end; background:green }\
         .target { align-self:self-end; writing-mode:horizontal-tb; direction:ltr; background:red }\
         </style><div class=\"column\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.x() - target.x()).abs() < 0.01,
        "horizontal/ltr self-end should align its inline-end/right side to column cross-end: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn align_items_self_end_is_inherited_by_auto_align_self() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; align-items:self-end; height:80pt; width:100pt }\
         .item { width:20pt; height:20pt; margin:0; writing-mode:vertical-lr; direction:ltr }\
         .reference { align-self:flex-end; background:green }\
         .target { background:red }\
         </style><div class=\"row\"><div class=\"item reference\"></div><div class=\"item target\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let reference = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        (reference.y() - target.y()).abs() < 0.01,
        "align-self:auto should inherit align-items:self-end and align the vertical item's inline-end/bottom side: reference={reference:?}, target={target:?}"
    );
}

#[tokio::test]
async fn safe_self_end_falls_back_to_cross_start_when_item_overflows() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin:0 }\
         .row { display:flex; height:20pt; width:100pt }\
         .start { width:20pt; height:20pt; background:green }\
         .target { width:20pt; height:40pt; align-self:safe self-end; background:red }\
         </style><div class=\"row\"><div class=\"start\"></div><div class=\"target\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let start = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let target = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        ((start.y() + start.height()) - (target.y() + target.height())).abs() < 0.01,
        "safe self-end should fall back to row cross-start when the item overflows: start={start:?}, target={target:?}"
    );
}

#[tokio::test]
async fn shrink_to_fit_inline_block_includes_consecutive_float_row_width() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin:10pt } body { margin:0 }\
         .box { display:inline-block; background:red; vertical-align:top }\
         .box > div { float:left; width:25pt; height:10pt }\
         </style><div class=\"box\"><div></div><div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        (red.width() - 50.0).abs() < 0.01,
        "inline-block shrink-to-fit width should include both consecutive floats: {red:?}"
    );
}

#[tokio::test]
async fn shrink_to_fit_float_includes_same_line_float_and_inline_block() {
    let document = Html::from_string(
        "<style>@page { size: 320pt 260pt; margin:10pt } body { margin:0 }\
         .outer { float:left; min-width:150pt; background:red }\
         .right { float:right; width:100pt; height:200pt; background:green }\
         .inline { display:inline-block; vertical-align:top; width:100pt; height:200pt; background:green }\
         </style><div class=\"outer\"><div class=\"right\"></div><div class=\"inline\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        (red.width() - 200.0).abs() < 0.01,
        "float shrink-to-fit width should include the same-line float and inline-block: {red:?}"
    );

    let mut green = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    green.sort_by(|left, right| left.x().total_cmp(&right.x()));
    assert_eq!(green.len(), 2, "green rects={green:?}");

    let left = green[0];
    let right = green[1];
    for rect in [left, right] {
        assert!((rect.width() - 100.0).abs() < 0.01, "green={rect:?}");
        assert!((rect.height() - 200.0).abs() < 0.01, "green={rect:?}");
        assert!(
            (rect.y() - red.y()).abs() < 0.01,
            "red={red:?} green={rect:?}"
        );
    }
    assert!(
        (left.x() - red.x()).abs() < 0.01,
        "left={left:?} red={red:?}"
    );
    assert!(
        (right.x() - (red.x() + 100.0)).abs() < 0.01,
        "right={right:?} red={red:?}"
    );
    assert!(
        ((right.x() + right.width()) - (red.x() + red.width())).abs() < 0.01,
        "right={right:?} red={red:?}"
    );
}

#[tokio::test]
async fn inline_block_auto_height_expands_to_contain_internal_float() {
    let document = Html::from_string(
        "<style>\
         @page { size: 160pt 140pt; margin: 10pt }\
         body, div { margin: 0 }\
         .atom { display: inline-block; background: rgb(0 128 0) }\
         .float { float: left; width: 30pt; height: 40pt; background: rgb(0 0 255) }\
         </style>\
         <div><span class=\"atom\"><span class=\"float\"></span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let atom = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        atom.height() >= 39.99,
        "inline-block background should include its internal float: {atom:?}"
    );
}

#[tokio::test]
async fn flex_row_height_uses_pre_line_item_line_count() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 160pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } .row { display: flex; margin: 0 0 20pt } .item { white-space: pre-line; flex: 1 } p { margin: 0 }</style><div class=\"row\"><div class=\"item\">One\nTwo\nThree</div><div>Side</div></div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((one.y() - after.y() - 56.0).abs() < 0.01);
}

#[tokio::test]
async fn flex_row_height_uses_tallest_pre_line_item_line_count() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0; font-size: 11pt; line-height: 17.6pt } .row { display: flex; margin: 0 0 44pt } .from, .to { white-space: pre-line } .from { flex: 1 }</style><div class=\"row\"><address class=\"from\">One\nTwo\nThree\nFour</address><address class=\"to\">A\nB\nC</address></div><p style=\"margin:0\">After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((one.y() - after.y() - 114.4).abs() < 0.01);
}

#[tokio::test]
async fn flex_row_height_counts_preserved_leading_newline_in_pre_line_item() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 180pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 12pt } dt::before { content: ''; display: block } .row { display: flex; margin: 0 0 20pt } .item { white-space: pre-line; flex: 1 } p { margin: 0 }</style><div class=\"row\"><address class=\"item\">\nOne\nTwo</address></div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let one = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    // CSS Text preserves the leading segment break in `white-space: pre-line`,
    // so this item has three 12pt line boxes: an empty line, `One`, and `Two`.
    // The first visible line is one line below the item top; the following
    // paragraph is separated from it by the two remaining item lines plus the
    // row's 20pt bottom margin.
    assert!(
        (one.y() - after.y() - 44.0).abs() < 0.01,
        "one.y()={} after.y()={}",
        one.y(),
        after.y()
    );
}

#[tokio::test]
async fn supports_flex_grow() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; width:200pt\"><div style=\"flex-grow:1; height:10pt; background:red\"></div><div style=\"width:50pt; height:10pt; background:blue\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].rects()[0].width(), 150.0);
    assert_eq!(document.pages[0].rects()[1].width(), 50.0);
}

#[tokio::test]
async fn floats_after_a_block_start_below_that_block() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } p { margin: 0 }</style>\
         <p>Intro\
         <div style=\"float:left; width:25pt; height:20pt; background:green\"></div>\
         <div style=\"float:left; width:25pt; height:20pt; background:green\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let intro = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Intro")
        .expect("paragraph text should render");
    let green_tops = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .collect::<Vec<_>>();

    assert_eq!(green_tops.len(), 2);
    assert!(
        green_tops.iter().all(|top| *top <= intro.y() + 0.5),
        "floats should start after the preceding block line: line={intro:?}, tops={green_tops:?}"
    );
}

#[tokio::test]
async fn adjacent_left_floats_share_row_and_overflow_moves_down() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         div { float: left; width: 45pt; height: 20pt; background: green }</style>\
         <div></div><div></div><div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(rects.len(), 3);
    assert!((rects[0].x() - 10.0).abs() < 0.01, "rects={rects:?}");
    assert!((rects[1].x() - 55.0).abs() < 0.01, "rects={rects:?}");
    assert!((rects[2].x() - 10.0).abs() < 0.01, "rects={rects:?}");
    assert!(rects[2].y() < rects[0].y(), "rects={rects:?}");
}

#[tokio::test]
async fn mixed_left_and_right_floats_use_opposite_edges() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0 }\
         .left { float: left; width: 30pt; height: 20pt; background: green }\
         .right { float: right; width: 30pt; height: 20pt; background: blue }</style>\
         <div class=\"left\"></div><div class=\"right\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((green.x() - 10.0).abs() < 0.01, "green={green:?}");
    assert!((blue.x() - 100.0).abs() < 0.01, "blue={blue:?}");
    assert!(
        (green.y() - blue.y()).abs() < 0.01,
        "green={green:?} blue={blue:?}"
    );
}

#[tokio::test]
async fn clear_both_moves_block_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         .clear { clear: both; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"clear\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear block should start below float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn first_cleared_child_after_float_uses_parent_start_clearance_hypothesis() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>@page{size:140px 140px;margin:0}body{margin:0}p{display:none}</style>\
         <p>Test passes if there is a filled green square and no red.</p>\
         <div style=\"width:100px;background:red\">\
           <div style=\"float:left;width:100px;height:50px;background:green\"></div>\
           <div style=\"clear:left;margin-top:200px\"></div>\
         </div>\
         <div style=\"width:100px;height:50px;background:green\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = CssColor::new(0, 128, 0);
    let green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(green))
        .collect::<Vec<_>>();
    let left = green_rects
        .iter()
        .map(|rect| rect.x())
        .min_by(f32::total_cmp)
        .expect("the floated green region should paint");
    let top = green_rects
        .iter()
        .map(|rect| rect.y())
        .min_by(f32::total_cmp)
        .expect("the green square should have a top edge");
    let right = green_rects
        .iter()
        .map(|rect| rect.x() + rect.width())
        .max_by(f32::total_cmp)
        .expect("the green square should have a right edge");
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .max_by(f32::total_cmp)
        .expect("the green square should have a bottom edge");
    assert!((right - left - 75.0).abs() < 0.01, "green={green_rects:?}");
    assert!((bottom - top - 75.0).abs() < 0.01, "green={green_rects:?}");
    for y in [top + 18.75, top + 56.25] {
        assert_eq!(
            final_rect_fill_at(page, left + 37.5, y),
            Some(green),
            "the two 50px green regions should form one visible 100px square at y={y}"
        );
    }
    assert_ne!(
        final_rect_fill_at(page, left + 37.5, bottom + 18.75),
        Some(green)
    );
}

#[tokio::test]
async fn following_text_wraps_around_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <div class=\"float\"></div>one two three four",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("one"))
        .unwrap();

    assert!(
        first.x() >= 39.0,
        "first text line should be shortened by the float: {first:?}"
    );
}

#[tokio::test]
async fn inline_float_after_text_does_not_shift_previous_text() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         span { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <p style=\"margin:0\">Before <span></span>After after after</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Before"))
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();
    let green_top = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .unwrap();

    assert!(
        (before.x() - 10.0).abs() < 0.01,
        "text before the float should keep the original line start: {before:?}"
    );
    assert!(
        green_top > after.y() + 5.0,
        "inline float that fits after prefix text should be placed on the prefix line: before={before:?}, after={after:?}, green_top={green_top}"
    );
    assert!(
        (before.y() - after.y()).abs() < 0.01,
        "suffix text after a fitting inline float should remain on the prefix line: before={before:?}, after={after:?}"
    );
    assert!(
        after.x() >= 39.0,
        "text after the waiting inline float should avoid the float: {after:?}"
    );
}

#[tokio::test]
async fn inline_float_after_text_defers_when_remaining_band_is_too_narrow() {
    let document = Html::from_string(
        "<style>@page { size: 118pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         span { float: left; width: 80pt; height: 20pt; background: green }</style>\
         <p style=\"margin:0\">Before <span></span>After after after</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Before"))
        .unwrap();
    let green_top = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .map(|rect| rect.y() + rect.height())
        .unwrap();

    assert!(
        green_top < before.y(),
        "inline float should defer when it cannot fit after prefix text: before={before:?}, green_top={green_top}"
    );
}

#[tokio::test]
async fn inline_float_rolled_to_next_line_stays_below_earlier_line_box() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 240pt 120pt; margin: 10pt } body { margin: 0 }\
         div { font: 10pt/10pt monospace; width: 12ch; line-height: 1; background: yellow }\
         .float { float: left; width: 12ch; height: 1em; background: orange }</style>\
         <div>1111 <nobr>2222 <div class=\"float\"></div>3333</nobr></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first_line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("1111"))
        .unwrap();
    let first_x = first_line
        .runs
        .iter()
        .find(|run| run.text.contains("1111"))
        .map(|run| first_line.x() + run.x_offset)
        .unwrap();
    let second_x = first_line
        .runs
        .iter()
        .find(|run| run.text.contains("2222"))
        .map(|run| first_line.x() + run.x_offset)
        .unwrap();
    let text_ordered = first_line
        .text
        .find("1111")
        .zip(first_line.text.find("2222"))
        .is_some_and(|(first, second)| first < second);
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("3333"))
        .unwrap();
    let orange = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .unwrap();
    let orange_top = orange.y() + orange.height();

    assert!(
        second_x > first_x || (second_x - first_x).abs() < 0.01 && text_ordered,
        "1111 and 2222 should remain ordered on the first line: line={first_line:?}"
    );
    assert!(
        orange_top < first_line.y(),
        "rolled inline float must not be higher than the earlier line box: first={first_line:?}, orange={orange:?}"
    );
    assert!(
        after.x() >= orange.x() + orange.width() - 0.01,
        "3333 should sit to the right of the rolled left float: after={after:?}, orange={orange:?}"
    );
    assert!(
        orange_top > after.y() + 5.0,
        "3333 should share the rolled float's line band: after={after:?}, orange={orange:?}"
    );
}

#[tokio::test]
async fn inline_float_nowrap_does_not_break_before_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         span { float: right; width: 5ch; height: 5ch; background: blue }</style>\
         <div>Some text that <span></span> overflows my parent.</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "nowrap inline float should keep all text on one visual line: {text_lines:?}"
    );
    let text = text_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("Some text that") && text.contains("overflows my parent."),
        "nowrap line should contain prefix and suffix text: {:?}",
        text_lines
    );
    assert!(
        text.contains("that overflows"),
        "collapsible whitespace should collapse across the inline float marker: {text:?}"
    );
    assert!(
        !text.contains("that  overflows"),
        "inline float marker must not preserve both adjacent collapsible spaces: {text:?}"
    );

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();
    assert!(
        blue.x() > text_lines[0].x() + 20.0,
        "right float should be placed at the right side of the nowrap band: blue={blue:?}, line={:?}",
        text_lines[0]
    );
    assert!(
        ((blue.y() + blue.height()) - text_lines[0].y()).abs() < 3.0,
        "right float should share the nowrap line's top band: blue={blue:?}, line={:?}",
        text_lines[0]
    );
}

#[tokio::test]
async fn nested_block_float_in_nowrap_inline_retains_marker_and_continuation() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         .float { float: right; width: 5ch; height: 5ch; background: blue }</style>\
         <div><span>S<div class=\"float\"></div><span>ome</span> text that overflows my parent.</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        text_lines.len(),
        1,
        "nowrap text must remain one line: {text_lines:?}"
    );
    assert!(
        text_lines[0]
            .text
            .contains("Some text that overflows my parent."),
        "the float marker must retain the text on both sides: {:?}",
        text_lines[0]
    );

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("nested block float must produce a blue paint rect");
    assert!(
        blue.y() + blue.height() <= text_lines[0].y() + 0.01,
        "the deferred right float must occupy the preceding eligible row: blue={blue:?}, line={:?}",
        text_lines[0]
    );
}

#[tokio::test]
async fn inline_float_nowrap_collapses_space_after_earlier_marker() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         span { float: right; width: 5ch; height: 5ch; background: blue }</style>\
         <div>Some <span></span> text that overflows my parent.</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .map(|line| line.text.as_str())
        .collect::<String>();

    assert!(
        text.contains("Some text"),
        "collapsible whitespace should collapse across an earlier inline float marker: {text:?}"
    );
    assert!(
        !text.contains("Some  text"),
        "earlier inline float marker must not preserve both adjacent collapsible spaces: {text:?}"
    );
}

#[tokio::test]
async fn inline_left_float_nowrap_keeps_text_unbroken() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         span { float: left; width: 5ch; height: 5ch; background: green }</style>\
         <div>Some text that <span></span> overflows my parent.</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "left nowrap inline float should keep all text on one visual line: {text_lines:?}"
    );
    let text = text_lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("Some text that") && text.contains("overflows my parent."),
        "nowrap line should contain prefix and suffix text: {:?}",
        text_lines
    );

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    assert!(
        green.x() <= text_lines[0].x() + 0.01,
        "left float should be placed at the left side of the nowrap band: green={green:?}, line={:?}",
        text_lines[0]
    );
    // A float enters the inline formatting context at its source position:
    // text that precedes the marker remains at the line start, while only
    // following content wraps around the float.  The one-line assertion
    // above verifies that the entire `nowrap` source line remains intact.
}

#[tokio::test]
async fn zero_width_inline_float_does_not_break_an_unbreakable_word() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .test { width: 0; font: 10pt/10pt monospace } .oof { float: left }</style>\
         <div class=\"test\">un<span class=\"oof\"></span>bro<b class=\"oof\">float</b>ken</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "unbroken"),
        "an inline float in a zero-width band must not split the surrounding unbreakable word: {:?}",
        document.pages[0].lines(),
    );
}

#[tokio::test]
async fn nested_inline_floats_keep_nowrap_text_in_the_float_context() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .wrapper { white-space: nowrap; font: 10pt/10pt monospace } span { float: left }</style>\
         <div class=\"wrapper\"><span>X<span>X</span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("XX"),
        "a nested inline float must retain its nowrap continuation instead of dropping it: {text:?}",
    );
}

#[tokio::test]
async fn multiple_inline_floats_nowrap_preserve_same_side_order() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         div { width: 10ch; white-space: nowrap; font: 10pt/10pt monospace }\
         .first { float: right; width: 2ch; height: 4ch; background: blue }\
         .second { float: right; width: 2ch; height: 4ch; background: red }</style>\
         <div>Some text <span class=\"first\"></span><span class=\"second\"></span> overflows.</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let text_lines = document.pages[0]
        .lines()
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect::<Vec<_>>();
    let line_y = text_lines[0].y();
    assert!(
        text_lines
            .iter()
            .all(|line| (line.y() - line_y).abs() < 0.01),
        "multiple nowrap inline floats should keep all text on one visual line: {text_lines:?}"
    );

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    assert!(
        blue.x() > red.x(),
        "same-side right floats should keep source-order placement: blue={blue:?}, red={red:?}"
    );
}

#[tokio::test]
async fn flow_root_float_does_not_leak_to_following_text() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .root { display: flow-root } .float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <div class=\"root\"><div class=\"float\"></div></div><p style=\"margin:0\">After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((after.x() - 10.0).abs() < 0.01, "after={after:?}");
}

#[tokio::test]
async fn flex_container_avoids_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         .flex { display: flex; width: 60pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><div class=\"flex\"><span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!(blue.x() >= 39.0, "flex should avoid active float: {blue:?}");
}

#[tokio::test]
async fn table_wrapper_avoids_active_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         table { width: 60pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!(
        blue.x() >= 39.0,
        "table should avoid active float: {blue:?}"
    );
}

#[tokio::test]
async fn table_wrapper_moves_below_floats_when_band_is_too_narrow() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .left { float: left; width: 40pt; height: 20pt; background: green }\
         .right { float: right; width: 40pt; height: 20pt; background: blue }\
         table { width: 30pt; height: 10pt; background: red }</style>\
         <div class=\"left\"></div><div class=\"right\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "table should move below floats when no band is wide enough: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_both_moves_table_wrapper_below_active_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         table { clear: both; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear table should start below float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_left_moves_table_wrapper_below_left_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 40pt; height: 20pt; background: green }\
         table { clear: left; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear-left table should start below left float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn clear_right_moves_table_wrapper_below_right_float() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: right; width: 40pt; height: 20pt; background: green }\
         table { clear: right; width: 40pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "clear-right table should start below right float: green={green:?} red={red:?}"
    );
}

#[tokio::test]
async fn empty_table_wrapper_uses_float_avoidance() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table { margin: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 30pt; background: green }\
         table { width: 50pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><table></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= 39.0,
        "empty table should avoid active float: {red:?}"
    );
}

#[tokio::test]
async fn table_cell_float_does_not_leak_to_following_parent_text() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body, table, td, p { margin: 0; padding: 0; border-spacing: 0; font: 10pt/10pt monospace }\
         .cell-float { float: left; width: 30pt; height: 20pt; background: green }</style>\
         <table><tr><td><div class=\"cell-float\"></div></td></tr></table><p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert!((after.x() - 10.0).abs() < 0.01, "after={after:?}");
}

#[tokio::test]
async fn float_exclusions_do_not_leak_to_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         .break { break-before: page }</style>\
         <div class=\"float\"></div><div class=\"break\">Next</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let next = document.pages[1]
        .lines()
        .iter()
        .find(|line| line.text == "Next")
        .unwrap();

    assert!((next.x() - 10.0).abs() < 0.01, "next={next:?}");
}

#[tokio::test]
async fn fragmented_float_excludes_following_text_on_later_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let continued = document.pages[1]
        .lines()
        .iter()
        .find(|line| !line.text.trim().is_empty())
        .unwrap();

    assert!(
        continued.x() >= 49.0,
        "continued text should avoid the fragmented float on page 2: {continued:?}"
    );
}

#[tokio::test]
async fn clear_both_after_fragmented_float_starts_below_current_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, div { margin: 0 }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }\
         .after { clear: both; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <div class=\"after\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let (red_page_index, red) = document
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| page.rects().iter().map(move |rect| (page_index, rect)))
        .find(|(_, rect)| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let continued_green = document.pages[red_page_index]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .min_by(|left, right| {
            (left.y() + left.height())
                .partial_cmp(&(right.y() + right.height()))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    assert!(
        red.y() + red.height() <= continued_green.y() + 0.01,
        "clear after a fragmented float should start below the continued fragment on its page: red={red:?} green={continued_green:?}"
    );
}

#[tokio::test]
async fn clear_both_after_three_fragment_float_clears_final_continuation() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, div { margin: 0 }\
         .float { float: left; width: 40pt } .chunk { height: 45pt; background: green }\
         .after { clear: both; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <div class=\"after\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let (red_page, red) = document
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| page.rects().iter().map(move |rect| (page_index, rect)))
        .find(|(_, rect)| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let (green_page, last_green) = document
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page_index, page)| page.rects().iter().map(move |rect| (page_index, rect)))
        .filter(|(_, rect)| rect.fill == Some(CssColor::new(0, 128, 0)))
        .max_by(|(left_page, left), (right_page, right)| {
            left_page.cmp(right_page).then_with(|| {
                left.y()
                    .partial_cmp(&right.y())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .unwrap();

    assert_eq!(
        red_page, green_page,
        "clear should wait for the final continued float fragment before placing the following box"
    );
    assert!(
        red.y() + red.height() <= last_green.y() + 0.01,
        "clear after a three-fragment float should start below the final fragment: red={red:?} green={last_green:?}"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_bookmark_side_effects() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2>Float Mark</h2><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let bookmark = document
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.label == "Float Mark")
        .unwrap();

    assert_eq!(bookmark.page_index, 1, "bookmark={bookmark:?}");
}

#[tokio::test]
async fn fragmented_float_preserves_anchor_for_generated_page_reference() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @bottom-center { content: target-counter(url(#float-anchor), page); font-size: 8pt; height: 10pt } }\
         body, div, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2 id=\"float-anchor\">Float Anchor</h2><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .any(|line| line.text == "2"),
        "page-margin generated content should resolve the anchor inside the fragmented float"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_named_string_for_page_margin_content() {
    let document = Html::from_string(
        "<style>\
         @page { size: 140pt 80pt; margin: 10pt; @top-center { content: string(float_title); font-size: 8pt; height: 10pt } }\
         body, h2 { margin: 0; font: 10pt/10pt sans-serif }\
         h2 { string-set: float_title content(text) }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><h2>Float String</h2><div class=\"chunk\"></div></div>\
         <div>After<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line<br>Line</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document.pages[1]
            .lines()
            .iter()
            .any(|line| line.text == "Float String" && line.y() > 65.0),
        "page 2 top margin should use the named string captured inside the fragmented float"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_svg_replaced_descendant() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         .float { float: left; width: 60pt } .chunk { height: 45pt }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><svg width=\"10pt\" height=\"10pt\"><rect width=\"10pt\" height=\"10pt\" fill=\"blue\"/></svg><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.paths())
            .any(|path| path.fill == Some(CssColor::new(0, 0, 255))),
        "replaced SVG descendant inside a fragmented float should survive replay"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_generated_before_content() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt; @bottom-center { content: target-text(url(#generated), before) target-text(url(#generated), content); font: 8pt/8pt sans-serif; height: 10pt } }\
         body, div { margin: 0; font: 10pt/10pt sans-serif }\
         .float { float: left; width: 80pt } .chunk { height: 45pt }\
         .generated::before { content: 'Float Generated '; }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div id=\"generated\" class=\"generated\"> Body</div><div class=\"chunk\"></div></div>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rendered_text = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    assert!(
        ["Float", "Generated", "Body"]
            .iter()
            .all(|part| rendered_text.iter().any(|line| line.contains(part))),
        "generated pseudo text inside a fragmented float should survive anchor-text replay: {rendered_text:?}"
    );
}

#[tokio::test]
async fn fragmented_float_preserves_generated_image_content() {
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 140pt 80pt; margin: 10pt }} body, div {{ margin: 0; font: 10pt/10pt sans-serif }}\
         .float {{ float: left; width: 80pt }} .chunk {{ height: 45pt }}\
         .generated::before {{ content: url({png}) ' '; width: 8pt; height: 6pt }}</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"generated\">Icon</div><div class=\"chunk\"></div></div>\
         <p>After</p>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .map(|page| page.images().len())
            .sum::<usize>()
            >= 1,
        "generated image content inside a fragmented float should survive replay"
    );
}

#[tokio::test]
async fn vertical_writing_clear_left_matches_line_left_inline_start_float() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 100pt; margin: 10pt } body { margin: 0; writing-mode: vertical-rl; direction: ltr }\
         .float { float: inline-start; width: 30pt; height: 20pt; background: green }\
         .clear { clear: left; width: 20pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"clear\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.01,
        "line-relative clear:left should clear a vertical inline-start top-side float: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_text_avoids_inline_start_top_float() {
    let normal = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body, p { margin: 0; writing-mode: vertical-rl; direction: ltr; font: 10pt/12pt sans-serif }</style>\
         <p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body, p { margin: 0; writing-mode: vertical-rl; direction: ltr; font: 10pt/12pt sans-serif }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }</style>\
         <div class=\"float\"></div><p>After</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();
    let normal_after = normal.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("After"))
        .unwrap();

    assert!(
        after.y() < normal_after.y() - 25.0 && after.y() <= green.y() + 0.5,
        "vertical text should start below the top-side logical float: normal={normal_after:?}, green={green:?}, after={after:?}"
    );
}

#[tokio::test]
async fn vertical_logical_inline_floats_advance_to_unoccupied_columns() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
html { writing-mode: vertical-rl; }
body > div { height: 20em; margin: 1em; padding: 2px; border: 1px solid silver; }
div > div { margin: .5em; padding: .5em; background: yellow; }
.is { float: inline-start; }
.ie { float: inline-end; }
.ltr { direction: ltr; }
.rtl { direction: rtl; }
</style>
<div class="ltr">
 Lorem ipsum dolor sit amet, consectetur adipiscing elit.
 Phasellus efficitur nisi at sollicitudin eleifend.
 <div class="is">Inline-start</div>
 Vestibulum ac condimentum diam. Vivamus viverra iaculis mollis.
 Nam bibendum, dolor id porttitor egestas, metus sem pretium eros,
 ut mollis mauris ligula eu risus. Aenean eget vestibulum nunc.
 <div class="ie">Inline-end</div>
 Nam vitae eleifend tellus. Vestibulum ut accumsan lacus.
 Vivamus vitae eros hendrerit, tincidunt augue non, laoreet justo.
 Aliquam erat volutpat.
</div>
<div class="rtl">
 Lorem ipsum dolor sit amet, consectetur adipiscing elit.
 Phasellus efficitur nisi at sollicitudin eleifend.
 <div class="is">Inline-start</div>
 Vestibulum ac condimentum diam. Vivamus viverra iaculis mollis.
 Nam bibendum, dolor id porttitor egestas, metus sem pretium eros,
 ut mollis mauris ligula eu risus. Aenean eget vestibulum nunc.
 <div class="ie">Inline-end</div>
 Nam vitae eleifend tellus. Vestibulum ut accumsan lacus.
 Vivamus vitae eros hendrerit, tincidunt augue non, laoreet justo.
 Aliquam erat volutpat.
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .any(|line| line.text.contains("Lorem ipsum")),
        "both vertical writing directions should lay out their text"
    );
    assert_eq!(
        document
            .pages
            .iter()
            .flat_map(|page| page.rects())
            .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
            .count(),
        4,
        "each logical inline float should paint its yellow background"
    );
    let mut unmatched_yellow_backgrounds = document
        .pages
        .iter()
        .flat_map(|page| page.rects())
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    for label in ["Inline-start", "Inline-end"] {
        let matching_lines = document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .filter(|line| line.text.contains(label))
            .collect::<Vec<_>>();
        assert_eq!(matching_lines.len(), 2, "both directions paint {label}");
        for line in matching_lines {
            let matching_background = unmatched_yellow_backgrounds
                .iter()
                .position(|background| {
                    line.x() >= background.x() - 0.01
                        && line.x() <= background.x() + background.width() + 0.01
                        && line.y() >= background.y() - 0.01
                        && line.y() <= background.y() + background.height() + 0.01
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{label} line origin must lie inside its own unmatched yellow float background: line={line:?}, backgrounds={unmatched_yellow_backgrounds:?}"
                    )
                });
            unmatched_yellow_backgrounds.remove(matching_background);
        }
    }
    assert!(
        unmatched_yellow_backgrounds.is_empty(),
        "every yellow float background must own exactly one label"
    );
}

#[tokio::test]
async fn vertical_float_replay_projects_ordinary_and_positioned_contents_once() {
    for (writing_mode, direction, origin) in [
        ("vertical-rl", "rtl", "bottom"),
        ("vertical-lr", "rtl", "bottom"),
        ("sideways-rl", "rtl", "bottom"),
        ("sideways-lr", "ltr", "bottom"),
        ("vertical-rl", "ltr", "top"),
        ("vertical-lr", "ltr", "top"),
        ("sideways-rl", "ltr", "top"),
        ("sideways-lr", "rtl", "top"),
    ] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 180pt 180pt; margin: 10pt }} html, body {{ margin: 0; writing-mode: {writing_mode}; direction: {direction}; font: 10pt/12pt sans-serif }} .float {{ float: inline-start; position: relative; width: 70pt; height: 60pt; padding: 6pt; background: yellow }} .positioned {{ position: absolute; left: 4pt; bottom: 4pt; width: 8pt; height: 8pt; background: red }}</style><div class=\"float\">Float label<div class=\"positioned\"></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let yellow = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
            .expect("float background should paint");
        let red = document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .expect("positioned float child should paint");
        let line = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.contains("Float label"))
            .expect("ordinary float text should paint");

        assert!(
            line.x() >= yellow.x() - 0.01
                && line.x() <= yellow.x() + yellow.width() + 0.01
                && line.y() >= yellow.y() - 0.01
                && line.y() <= yellow.y() + yellow.height() + 0.01,
            "ordinary {origin}-origin {writing_mode} {direction} float content must share the projected background: yellow={yellow:?}, line={line:?}"
        );
        assert!(
            red.x() >= yellow.x() - 0.01
                && red.x() + red.width() <= yellow.x() + yellow.width() + 0.01
                && red.y() >= yellow.y() - 0.01
                && red.y() + red.height() <= yellow.y() + yellow.height() + 0.01,
            "positioned {origin}-origin {writing_mode} {direction} float content must receive the same single projection: yellow={yellow:?}, red={red:?}"
        );
    }
}

#[tokio::test]
async fn vertical_writing_over_tall_bfc_moves_past_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "over-tall vertical BFC should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn orthogonal_bfc_consumes_parent_vertical_float_band() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { writing-mode: horizontal-tb; overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "orthogonal horizontal BFC should consume the parent vertical float band: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_bfc_moves_past_bottom_side_insufficient_span() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-end; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical BFC should move past a bottom-side float when the remaining span is too small: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_bfc_root_avoids_inline_start_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-rl; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .bfc { overflow: hidden; width: 24pt; height: 10pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"bfc\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.y() + red.height() <= green.y() + 0.5,
        "vertical BFC root should be placed below the top-side logical float: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_table_wrapper_moves_past_over_tall_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         table { width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical table wrapper should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn vertical_writing_flex_container_moves_past_over_tall_top_float() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-start; width: 24pt; height: 30pt; background: green }\
         .flex { display: flex; width: 24pt; height: 100pt; background: red }</style>\
         <div class=\"float\"></div><div class=\"flex\"><span></span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert!(
        red.x() >= green.x() + green.width() - 0.01,
        "vertical flex container should move to the next block-axis slab: green={green:?}, red={red:?}"
    );
}

#[tokio::test]
async fn orthogonal_vertical_row_flex_auto_width_shrink_wraps_cross_size() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 160pt 140pt; margin: 10pt }
body { margin: 0 }
.container {
  display: flex;
  flex-flow: row;
  writing-mode: vertical-rl;
  border: 2pt solid black;
  height: 90pt;
}
.item {
  line-height: 0;
  float: right;
}
.color-block {
  display: inline-block;
  width: 15pt;
  height: 45pt;
}
</style>
<div class="container">
  <div class="item">
    <span class="color-block" style="background: orange"></span><br>
    <span class="color-block" style="background: grey"></span>
  </div>
  <div class="item">
    <span class="color-block" style="background: blue"></span><br>
    <span class="color-block" style="background: yellow"></span>
  </div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let black_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::BLACK))
        .collect::<Vec<_>>();
    let color_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill.is_some_and(|color| color != CssColor::BLACK))
        .collect::<Vec<_>>();
    assert_eq!(
        color_rects.len(),
        4,
        "fixture should paint the four flex item color blocks: {color_rects:?}"
    );
    let border_width = black_rects
        .iter()
        .map(|rect| rect.width())
        .fold(0.0f32, f32::max);

    assert!(
        (border_width - 34.0).abs() < 0.01,
        "vertical row flex auto width should shrink-wrap its 30pt physical cross-size plus 2pt borders, not fill the page: colors={color_rects:?}, borders={black_rects:?}"
    );
}

#[tokio::test]
async fn vertical_lr_inline_end_float_uses_bottom_side() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin: 0; writing-mode: vertical-lr; direction: ltr }\
         .float { float: inline-end; width: 24pt; height: 30pt; background: green }</style>\
         <div class=\"float\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 10.0).abs() < 0.5,
        "vertical-lr inline-end float should sit against the physical bottom side: green={green:?}"
    );
}

#[tokio::test]
async fn table_float_exclusions_do_not_leak_to_next_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, table, td { margin: 0; padding: 0; border-spacing: 0 }\
         .float { float: left; width: 30pt; height: 40pt; background: green }\
         table { break-before: page; width: 40pt; height: 10pt; background: blue }</style>\
         <div class=\"float\"></div><table><tr><td>A</td></tr></table>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let blue = document.pages[1]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 10.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn broken_left_float_excludes_lines_on_each_visible_fragment_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 40pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J<br>K<br>L</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(document.pages.len() >= 2);
    for page_index in 0..2 {
        assert!(
            document.pages[page_index].rects().iter().any(|rect| {
                rect.fill == Some(CssColor::new(0, 128, 0))
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            }),
            "float should paint on page {page_index}"
        );
        let line = document.pages[page_index]
            .lines()
            .iter()
            .find(|line| line.text.len() == 1)
            .expect("body text should share the float page");
        assert!(
            line.x() > 45.0,
            "left float should shorten lines on page {page_index}, line={line:?}"
        );
    }
}

#[tokio::test]
async fn broken_left_float_exclusion_ends_after_last_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt } .chunk { height: 30pt; background: green }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H<br>I<br>J<br>K<br>L<br>M<br>N</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line_after_float = document
        .pages
        .iter()
        .flat_map(|page| page.lines())
        .find(|line| line.text == "G")
        .expect("text should continue after the broken float");
    assert!(
        (line_after_float.x() - 10.0).abs() < 0.01,
        "float exclusion should end after the final fragment: {line_after_float:?}"
    );
}

#[tokio::test]
async fn positioned_descendant_stays_inside_broken_float_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body, p { margin: 0; font: 10pt/10pt monospace }\
         .float { float: left; width: 40pt }\
         .chunk { height: 40pt; background: blue; position: relative }\
         span { position: absolute; z-index: 1; left: 5pt; top: 0; width: 10pt; height: 10pt; background: red }</style>\
         <div class=\"float\"><div class=\"chunk\"></div><div class=\"chunk\"><span></span></div></div>\
         <p>A<br>B<br>C<br>D<br>E<br>F<br>G<br>H</p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[1];
    let float_background = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
    let positioned_child = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));

    assert!(
        float_background < positioned_child,
        "positioned child should paint inside the second float fragment stacking context"
    );
}

#[tokio::test]
async fn paginates_wpt_flex_reference_float_prefix_without_looping() {
    let row_widths = [
        "3ch", "3ch", "4ch", "3ch", "3ch", "4ch", "3ch", "0.4ch", "4ch", "3ch", "3ch", "4ch",
        "0.2ch", "0.2ch", "0.2ch", "3ch", "3ch", "4ch", "4.5ch", "4.5ch", "4.5ch", "3ch", "3ch",
        "4ch",
    ];
    let col_heights = ["1em", "1em", "1.5em", "1em", "1em", "1.5em", "1em"];
    let mut html = String::from(
        "<style>\
         body { display: grid; grid-template-columns: repeat(auto-fill, 66px 66px 66px); grid-auto-rows: 50px; font: 10px/1 monospace }\
         .wrap { counter-increment: test }\
         .row, .col { background: blue; padding: 5px; float: left }\
         .item { padding: 3px; border: 2px solid aqua; color: orange }\
         </style>",
    );

    for width in row_widths {
        html.push_str(&format!(
            "<div class=\"wrap\"><div class=\"row\"><div class=\"item\" style=\"width:{width}\">X X</div></div></div>"
        ));
    }
    for (index, height) in col_heights.iter().enumerate() {
        let grid_column = if index == 0 {
            " style=\"counter-reset:test; grid-column:1\""
        } else {
            ""
        };
        html.push_str(&format!(
            "<div class=\"wrap\"{grid_column}><div class=\"col\"><div class=\"item\" style=\"height:{height}\">X</div></div></div>"
        ));
    }

    let document = Html::from_string(html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(!document.pages.is_empty());
    assert!(
        document.pages.len() < 20,
        "float pagination should make progress, pages={}",
        document.pages.len()
    );
}

#[tokio::test]
async fn local_wpt_flex_intrinsic_reference_floats_finish_if_available() {
    let wpt_root = std::path::Path::new("../spindrift-wpt/third_party/wpt");
    if !wpt_root.exists() {
        return;
    }

    for reference in [
        "css/css-flexbox/flex-container-max-content-001-ref.html",
        "css/css-flexbox/flex-container-min-content-001-ref.html",
    ] {
        let path = wpt_root.join(reference);
        let document = Html::from_file(&path)
            .await
            .unwrap()
            .with_base_path(wpt_root)
            .unwrap()
            .render(&RenderOptions::default())
            .await
            .unwrap();
        assert!(
            !document.pages.is_empty() && document.pages.len() < 20,
            "{reference} should render with a finite page count, pages={}",
            document.pages.len()
        );
    }
}

async fn assert_column_wrap_intrinsic_flex_covers_reference(
    flex_flow: &str,
    first_order: Option<i32>,
    second_order: Option<i32>,
) {
    let first_order = first_order
        .map(|order| format!(" order: {order};"))
        .unwrap_or_default();
    let second_order = second_order
        .map(|order| format!(" order: {order};"))
        .unwrap_or_default();
    let document = Html::from_string(format!(
        "<!DOCTYPE html>\
         <link rel=\"author\" title=\"David Grogan\" href=\"mailto:dgrogan@chromium.org\">\
         <link rel=\"help\" href=\"https://drafts.csswg.org/css-flexbox/#intrinsic-sizes\">\
         <meta name=\"assert\" content=\"During the container's intrinsic sizing pass, the item has the correct available size when it is laid out during flex basis calculation.\">\
         <style>\
           #reference-overlapped-red {{ position: absolute; background-color: red; width: 100px; height: 100px; z-index: -1 }}\
           .grandchild {{ float: left; width: 50px; height: 50px }}\
         </style>\
         <p>Test passes if there is a filled green square and <strong>no red</strong>.</p>\
         <div id=\"reference-overlapped-red\"></div>\
         <div style=\"display: flex; flex-flow: {flex_flow}; width: max-content; height: 100px; background: green\">\
           <div style=\"width: 100%; flex: 0 0 auto; min-height: 0px;{first_order}\">\
             <div class=\"grandchild\"></div><div class=\"grandchild\"></div>\
           </div>\
           <div style=\"width: 90px; height: 50px; flex: 0 0 auto; min-height: 0px;{second_order}\"></div>\
         </div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap_or_else(|| panic!("reference red square should paint: {:?}", page.rects()));
    let green = page
        .rects()
        .iter()
        .find(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && rect.width() > 50.0
                && rect.height() > 50.0
        })
        .unwrap_or_else(|| panic!("green flex container should paint: {:?}", page.rects()));

    assert!((red.width() - 75.0).abs() < 0.01, "red={red:?}");
    assert!((red.height() - 75.0).abs() < 0.01, "red={red:?}");
    assert!(
        (green.x() - red.x()).abs() < 0.01,
        "green={green:?}, red={red:?}"
    );
    assert!(
        (green.y() - red.y()).abs() < 0.01,
        "green={green:?}, red={red:?}"
    );
    assert!(
        (green.width() - red.width()).abs() < 0.01,
        "green should cover the full reference width: green={green:?}, red={red:?}"
    );
    assert!(
        (green.height() - red.height()).abs() < 0.01,
        "green should cover the full reference height: green={green:?}, red={red:?}"
    );

    let expected = Some(CssColor::new(0, 128, 0));
    for x in [
        red.x() + 1.0,
        red.x() + red.width() / 2.0,
        red.x() + red.width() - 1.0,
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, red.y() + red.height() / 2.0),
            expected,
            "reference square should be fully covered at x={x}: green={green:?}, red={red:?}"
        );
    }
}

#[tokio::test]
async fn column_wrap_max_content_flex_basis_uses_max_cross_available_width() {
    assert_column_wrap_intrinsic_flex_covers_reference("column wrap", None, None).await;
}

#[tokio::test]
async fn ordered_column_wrap_max_content_flex_basis_uses_max_cross_available_width() {
    assert_column_wrap_intrinsic_flex_covers_reference("column wrap", Some(2), Some(1)).await;
}

#[tokio::test]
async fn column_reverse_wrap_max_content_flex_basis_uses_max_cross_available_width() {
    assert_column_wrap_intrinsic_flex_covers_reference("column-reverse wrap", None, None).await;
}

#[tokio::test]
async fn flex_order_sorts_items_and_preserves_source_order_ties() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:90pt }\
         .item { width:30pt; height:10pt }\
         .a { background:red; order:2 }\
         .b { background:green; order:-1 }\
         .c { background:blue; order:2 }\
         </style><div class=\"row\"><div class=\"item a\"></div><div class=\"item b\"></div><div class=\"item c\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((green.x() - 10.0).abs() < 0.01, "green={green:?}");
    assert!((red.x() - 40.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 70.0).abs() < 0.01, "blue={blue:?}");
}

#[tokio::test]
async fn flex_auto_minimum_is_capped_by_definite_width() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 100pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; flex-flow:wrap; width:300pt }\
         .item { width:75pt; height:10pt; background:green }\
         .wide { width:80pt; height:1pt }\
         </style>\
         <div class=\"row\">\
           <div class=\"item\"><div class=\"wide\"></div></div>\
           <div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0)) && (rect.height() - 10.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(item_rects.len(), 5);
    let first_line_count = item_rects
        .iter()
        .filter(|rect| (rect.y() - 80.0).abs() < 0.01)
        .count();
    assert_eq!(
        first_line_count, 4,
        "definite width should cap flex auto minimums so four 75pt items fit in 300pt: {item_rects:?}"
    );
    assert!(
        item_rects
            .iter()
            .all(|rect| (rect.width() - 75.0).abs() < 0.01),
        "flex item backgrounds should use the definite item width: {item_rects:?}"
    );
}

#[tokio::test]
async fn flex_baseline_alignment_aligns_item_text_baselines() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; align-items:baseline; width:160pt }\
         .big { font-size:30pt; line-height:30pt }\
         .small { font-size:10pt; line-height:10pt; align-self:first baseline }\
         p { margin:0 }\
         </style><div class=\"row\"><p class=\"big\">Big</p><p class=\"small\">Small</p></div>",
    )
    .render(&RenderOptions::default())
    .await
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
        "expected flex item text baselines to align: big={}, small={}",
        big.y(),
        small.y()
    );
}

#[tokio::test]
async fn flex_baseline_alignment_reserves_largest_top_margin() {
    let document = Html::from_string(
        "<style>@page { size: 420pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; align-items:baseline; width:300pt; height:60pt; background:blue }\
         .row span { display:inline-block; flex:none; width:80pt; margin:0 10pt; height:20pt }\
         .row span:nth-child(1) { background:yellow }\
         .row span:nth-child(2) { background:pink; margin-top:10pt; height:30pt }\
         .row span:nth-child(3) { background:lightblue; height:40pt }</style>\
         <div class=\"row\"><span>one</span><span>two</span><span>three</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let yellow = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .unwrap();
    let pink = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 192, 203)))
        .unwrap();
    let lightblue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(173, 216, 230)))
        .unwrap();

    let yellow_top = yellow.y() + yellow.height();
    let pink_top = pink.y() + pink.height();
    let lightblue_top = lightblue.y() + lightblue.height();
    assert!(
        (yellow_top - pink_top).abs() < 0.01 && (yellow_top - lightblue_top).abs() < 0.01,
        "baseline-aligned flex item border boxes should share the top offset reserved by the largest top margin: yellow={yellow:?}, pink={pink:?}, lightblue={lightblue:?}"
    );
}

#[tokio::test]
async fn flex_last_baseline_alignment_uses_last_text_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; align-items:last baseline; width:160pt }\
         .multi, .peer { font-size:10pt; line-height:12pt; margin:0 }\
         .multi { white-space:pre-line }\
         </style><div class=\"row\"><p class=\"multi\">One\nTwo</p><p class=\"peer\">Peer</p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let two = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Two")
        .unwrap();
    let peer = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Peer")
        .unwrap();
    assert!(
        (two.y() - peer.y()).abs() < 0.01,
        "expected flex item last text baselines to align: two={}, peer={}",
        two.y(),
        peer.y()
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_column_flex_item_and_abspos_bottom_inset_use_physical_edges() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flexbox { display:flex; width:100pt; height:100pt; align-items:baseline;\
           flex-direction:column; writing-mode:vertical-lr; direction:rtl;\
           flex-wrap:wrap-reverse; position:relative }\
         .item { width:100pt; height:50pt; background:green }\
         .abspos { position:absolute; bottom:50pt }</style>\
         <div class=\"flexbox\"><div class=\"abspos item\"></div><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let mut green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let positioned = &green_rects[0];
    let in_flow = &green_rects[1];
    assert!(
        (positioned.x() - in_flow.x()).abs() < 0.01
            && (positioned.y() - in_flow.y()).abs() < 0.01
            && (positioned.width() - 100.0).abs() < 0.01
            && (in_flow.width() - 100.0).abs() < 0.01
            && (positioned.height() - 50.0).abs() < 0.01
            && (in_flow.height() - 50.0).abs() < 0.01,
        "the wrap-reverse baseline fallback and physical bottom inset should both select the physical top half: {green_rects:?}"
    );
}

#[tokio::test]
async fn column_flex_baseline_items_fall_back_to_inline_start() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display:flex; flex-direction:column; align-items:baseline; width:100pt; height:100pt; background:red }\
         .item { width:40pt; height:20pt; background:green }\
         .wide { width:70pt; background:blue }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item wide\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first baseline item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second baseline item should paint");

    assert!(
        (green.x() - red.x()).abs() < 0.01 && (blue.x() - red.x()).abs() < 0.01,
        "column flex first-baseline self-alignment fallback should align inline-start edges: red={red:?}, green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn column_flex_baseline_wrap_reverse_aligns_synthesized_baselines_to_cross_start() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display:flex; flex-direction:column; flex-wrap:wrap-reverse; align-items:baseline; width:100pt; height:100pt; background:red }\
         .item { width:40pt; height:20pt; background:green }\
         .wide { width:70pt; background:blue }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item wide\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first baseline item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second baseline item should paint");

    assert!(
        (green.x() - red.x() - 30.0).abs() < 0.01 && (blue.x() - red.x() - 30.0).abs() < 0.01,
        "column flex wrap-reverse first-baseline fallback should align inline-start edges in the reversed line slot: red={red:?}, green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn column_flex_last_baseline_items_fall_back_to_inline_end() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display:flex; flex-direction:column; align-items:last baseline; width:100pt; height:100pt; background:red }\
         .item { width:40pt; height:20pt; background:green }\
         .wide { width:70pt; background:blue }</style>\
         <div class=\"flex\"><div class=\"item\"></div><div class=\"item wide\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("first last-baseline item should paint");
    let blue = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .expect("second last-baseline item should paint");

    assert!(
        (green.x() + green.width() - blue.x() - blue.width()).abs() < 0.01,
        "column flex last-baseline self-alignment fallback should align inline-end edges: red={red:?}, green={green:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn align_content_last_baseline_single_line_falls_back_to_logical_end() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; flex-wrap:wrap; align-content:last baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "last-baseline content fallback should pack the sole line at logical end: {green:?}"
    );
}

#[tokio::test]
async fn align_content_baseline_wrap_reverse_single_line_falls_back_to_logical_start() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; flex-wrap:wrap-reverse; align-content:baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 50.0).abs() < 0.01,
        "first-baseline content fallback should use logical start, not wrap-reverse flex-start: {green:?}"
    );
}

#[tokio::test]
async fn baseline_aligned_vertical_row_flex_item_falls_back_to_block_start() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:baseline; writing-mode:vertical-rl;\
           direction:ltr; flex-direction:row; width:100pt; height:100pt }\
         .item { width:50pt; height:100pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.x() - 50.0).abs() < 0.01,
        "vertical row first-baseline fallback should align to block-start/right: {green:?}"
    );
}

#[tokio::test]
async fn vertical_lr_row_flex_synthesizes_missing_baseline_from_line_under() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 120px 140px; margin: 0 }
body { margin: 0 }
p { display: none }
</style>
<p>Test passes if there is a filled green square and no red.</p>
<div style="display: flex; align-items: baseline; writing-mode: vertical-lr; text-orientation: sideways; background: red;">
  <div style="height: 50px; width: 100px; background: green;"></div>
  <div style="height: 50px; width: 100px; background: green; line-height: 0;">
    <span style="width: 10px; height: 10px; display: inline-block;"></span>
  </div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let mut green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let lower = &green_rects[0];
    let upper = &green_rects[1];
    assert!(
        ((upper.x() - lower.x()) - 7.5).abs() < 0.01
            && (lower.width() - upper.width()).abs() < 0.01
            && (lower.height() - upper.height()).abs() < 0.01
            && (lower.width() - lower.height() * 2.0).abs() < 0.01
            && (lower.y() + lower.height() - upper.y()).abs() < 0.01,
        "vertical-lr flex items should align the missing baseline to the inline-block baseline: {green_rects:?}"
    );
}

#[tokio::test]
async fn vertical_rl_row_flex_sideways_baseline_synthesis() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 120px 140px; margin: 0 }
body { margin: 0 }
p { display: none }
</style>
<p>Test passes if there is a filled green square and no red.</p>
<div style="display: flex; writing-mode: vertical-rl; align-items: baseline; text-orientation: sideways; position: relative; background: red;">
  <div style="background: green; line-height: 100px; font-size: 0; height: 50px;"><div style="display: inline-block;"></div></div>
  <div style="background: green; width: 50px; height: 50px;"></div>
  <div style="background: green; position: absolute; left: 0; bottom: 0; width: 50px; height: 50px;"></div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("flex background should render");

    for (x, y) in [
        (red.x() + red.width() * 0.25, red.y() + red.height() * 0.25),
        (red.x() + red.width() * 0.75, red.y() + red.height() * 0.25),
        (red.x() + red.width() * 0.25, red.y() + red.height() * 0.75),
        (red.x() + red.width() * 0.75, red.y() + red.height() * 0.75),
    ] {
        assert_eq!(
            final_rect_fill_at(page, x, y),
            Some(CssColor::new(0, 128, 0)),
            "sideways vertical-rl flex baseline synthesis should cover red at ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn vertical_rl_row_flex_mixed_missing_baseline_synthesizes_central_baseline() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 140px 140px; margin: 0 }
body { margin: 0 }
p { display: none }
</style>
<p>Test passes if there is a filled green square and no red.</p>
<div style="display: flex; writing-mode: vertical-rl; align-items: baseline; text-orientation: mixed;">
  <div style="background: green; line-height: 100px; height: 50px; color: transparent;">text</div>
  <div style="background: green; width: 100px; height: 50px;"></div>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let mut green_rects = page
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().total_cmp(&b.y()));

    let px = 0.75;
    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let top = &green_rects[0];
    let bottom = &green_rects[1];
    assert!(
        top.x().abs() < 0.01
            && bottom.x().abs() < 0.01
            && ((bottom.y() - top.y()) - 50.0 * px).abs() < 0.01
            && (top.width() - 100.0 * px).abs() < 0.01
            && (bottom.width() - 100.0 * px).abs() < 0.01
            && (top.height() - 50.0 * px).abs() < 0.01
            && (bottom.height() - 50.0 * px).abs() < 0.01,
        "vertical-rl text-orientation:mixed missing baseline should synthesize at the border-box center: {green_rects:?}"
    );
}

#[tokio::test]
async fn last_baseline_single_item_falls_back_to_self_end() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:last baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "last-baseline self-alignment fallback should align to self-end/bottom: {green:?}"
    );
}

#[tokio::test]
async fn explicit_baseline_align_self_uses_same_fallback_sides() {
    let first = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:flex-start; writing-mode:vertical-rl;\
           direction:ltr; flex-direction:row; width:100pt; height:100pt }\
         .item { align-self:first baseline; width:50pt; height:100pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let last = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:flex-start; width:100pt; height:100pt }\
         .item { align-self:last baseline; width:100pt; height:50pt; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first_green = first.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let last_green = last.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (first_green.x() - 50.0).abs() < 0.01 && (last_green.y() - 0.0).abs() < 0.01,
        "explicit baseline align-self should use the same fallback sides: first={first_green:?}, last={last_green:?}"
    );
}

#[tokio::test]
async fn baseline_fallback_does_not_override_auto_cross_margin() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 }\
         .flex { display:flex; align-items:baseline; width:100pt; height:100pt }\
         .item { width:100pt; height:50pt; margin-top:auto; background:green }</style>\
         <div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.y() - 0.0).abs() < 0.01,
        "baseline fallback must not override cross-axis auto margin placement: {green:?}"
    );
}

#[tokio::test]
async fn zero_percent_flex_basis_overrides_authored_main_size_for_empty_item() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .container { background: green; display: flex; height: 75pt; width: 75pt }\
         .item { background: red; flex-basis: 0%; height: 75pt; width: 75pt }\
         </style><div class=\"container\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("container background should paint");

    assert!((green.width() - 75.0).abs() < 0.01);
    assert!((green.height() - 75.0).abs() < 0.01);
    assert!(
        !document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0)) && rect.width() > 0.01)
    );
}

#[tokio::test]
async fn flex_basis_content_ignores_authored_main_size_for_base_size() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 140pt; margin: 10pt } body { margin: 0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:120pt; margin:0 0 10pt }\
         .content { flex:0 0 content; width:80pt; height:10pt; background:red }\
         .auto { flex:0 0 auto; width:80pt; height:10pt; background:blue }\
         </style><div class=\"row\"><div class=\"content\">Hi</div></div><div class=\"row\"><div class=\"auto\">Hi</div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let content = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let auto = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!(
        content.width() < 30.0,
        "content flex-basis should use intrinsic text width: {content:?}"
    );
    assert!((auto.width() - 80.0).abs() < 0.01, "auto={auto:?}");
}

#[tokio::test]
async fn flex_shorthand_accepts_unitless_zero_basis() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:200pt }\
         .item { flex:4 1 0; width:25pt; height:20pt }\
         </style><div class=\"row\"><div class=\"item\" style=\"background:yellow\"></div><div class=\"item\" style=\"background:pink\"></div><div class=\"item\" style=\"background:lightblue\"></div><div class=\"item\" style=\"background:gray\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let item_widths = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| (rect.height() - 20.0).abs() < 0.01)
        .map(|rect| rect.width())
        .collect::<Vec<_>>();

    assert_eq!(item_widths.len(), 4);
    assert!(
        item_widths.iter().all(|width| (*width - 50.0).abs() < 0.01),
        "flex:4 1 0 should use zero flex-basis and distribute the 200pt row equally: {item_widths:?}"
    );
}

#[tokio::test]
async fn column_flex_item_max_height_min_content_clamps_flex_basis() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 220pt; margin: 10pt } body { margin:0 }\
         .container { display:flex; flex-direction:column; width:75pt; height:150pt }\
         .item { max-height:min-content; flex-basis:150pt; background:green }\
         .child { height:75pt }\
         </style><div class=\"container\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| (rect.width() - 75.0).abs() < 0.01 && (rect.height() - 75.0).abs() < 0.01)
        .is_some();

    assert!(
        green,
        "max-height:min-content should clamp the column flex item to its 75pt child block-size: {:?}",
        document.pages[0].rects()
    );
}

#[tokio::test]
async fn column_flex_replaced_item_auto_min_height_uses_transferred_size() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .before {{ width:75pt; height:37.5pt; background:green }}\
         .flex {{ display:flex; flex-direction:column; width:75pt; height:0 }}\
         img {{ width:75pt }}\
         </style><div class=\"before\"></div><div class=\"flex\"><img src=\"{image}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    assert!((image.width() - 75.0).abs() < 0.01, "image={image:?}");
    assert!(
        (image.height() - 75.0).abs() < 0.01,
        "the image's transferred automatic minimum height should overflow the zero-height column flex container: {image:?}"
    );
}

#[tokio::test]
async fn nested_abspos_flex_image_uses_stretched_cross_size_as_flex_basis() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<!DOCTYPE html><style>@page {{ size: 240pt 240pt; margin: 0 }} body {{ margin:0 }}\
         .outer-flex {{ display:flex; height:200px }} .flex-item {{ width:100% }}\
         .intermediate {{ position:relative; height:100% }}\
         .inner-flex {{ display:flex; position:absolute; top:0; bottom:0 }}\
         img {{ display:block }}\
         </style><div class=\"outer-flex\"><div class=\"flex-item\"><div class=\"intermediate\"><div class=\"inner-flex\"><img src=\"{image}\"></div></div></div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    assert!((image.width() - 150.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 150.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn non_stretched_nested_abspos_flex_image_keeps_intrinsic_flex_basis() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<!DOCTYPE html><style>@page {{ size: 240pt 240pt; margin: 0 }} body {{ margin:0 }}\
         .outer-flex {{ display:flex; height:200px }} .flex-item {{ width:100% }}\
         .intermediate {{ position:relative; height:100% }}\
         .inner-flex {{ display:flex; align-items:flex-start; position:absolute; top:0; bottom:0 }}\
         img {{ display:block }}\
         </style><div class=\"outer-flex\"><div class=\"flex-item\"><div class=\"intermediate\"><div class=\"inner-flex\"><img src=\"{image}\"></div></div></div></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    assert!((image.width() - 1.0).abs() < 0.01, "image={image:?}");
    assert!((image.height() - 1.0).abs() < 0.01, "image={image:?}");
}

#[tokio::test]
async fn collapsed_flex_item_before_replaced_item_keeps_source_indexed_auto_minimum() {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 180pt 180pt; margin: 10pt }} body {{ margin:0 }}\
         .flex {{ display:flex; flex-direction:column; width:75pt; height:0 }}\
         .collapsed {{ visibility:collapse; width:75pt; height:20pt; background:red }}\
         img {{ width:75pt }}\
         </style><div class=\"flex\"><div class=\"collapsed\"></div><img src=\"{image}\"></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0))),
        "collapsed flex item must not paint"
    );
    assert_eq!(document.pages[0].images().len(), 1);
    let image = &document.pages[0].images()[0];
    assert!(
        (image.height() - 75.0).abs() < 0.01,
        "source-indexed estimates should preserve the image auto minimum after a collapsed sibling: {image:?}"
    );
}

#[tokio::test]
async fn flex_basis_intrinsic_keywords_use_min_and_max_content_sizes() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:220pt; margin:0 0 10pt }\
         .min { flex:0 0 min-content; width:120pt; height:10pt; background:red }\
         .max { flex:0 0 max-content; width:120pt; height:10pt; background:blue }\
         </style><div class=\"row\"><div class=\"min\">WWWW WWWW</div></div><div class=\"row\"><div class=\"max\">WWWW WWWW</div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!(
        min.width() < max.width(),
        "min-content should be narrower than max-content: min={min:?}, max={max:?}"
    );
    assert!(
        max.width() < 120.0,
        "max-content flex-basis should ignore authored width: {max:?}"
    );
}

#[tokio::test]
async fn flex_basis_fit_content_clamps_between_min_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:220pt; margin:0 0 10pt }\
         .min { flex:0 0 min-content; height:10pt; background:red }\
         .fit { flex:0 0 fit-content(30pt); height:10pt; background:green }\
         .max { flex:0 0 max-content; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"min\">Hi there friend</div></div>\
         <div class=\"row\"><div class=\"fit\">Hi there friend</div></div>\
         <div class=\"row\"><div class=\"max\">Hi there friend</div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let fit = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!(
        min.width() < fit.width() && fit.width() < max.width(),
        "fit-content should clamp between min/max content: min={min:?}, fit={fit:?}, max={max:?}"
    );
    assert!(
        (fit.width() - 30.0).abs() < 0.01,
        "fit-content(30pt) should use the argument when it is between intrinsic bounds: {fit:?}"
    );
}

#[tokio::test]
async fn flex_item_mixed_percentage_max_width_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 auto; width:180pt; max-width:calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn flex_item_mixed_percentage_min_width_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 auto; width:20pt; min-width:calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn flex_basis_mixed_percentage_resolves_against_definite_main_size() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display:flex; width:200pt }\
         .item { flex:0 0 calc(50% + 10pt); height:10pt; background:green }\
         </style><div class=\"row\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((item.width() - 110.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn column_flex_basis_mixed_percentage_resolves_against_definite_main_size() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 160pt; margin: 10pt } body { margin: 0 }\
         .col { display:flex; flex-direction:column; width:80pt; height:100pt }\
         .item { flex:0 0 calc(50% + 10pt); width:20pt; background:green }\
         </style><div class=\"col\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let item = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((item.height() - 60.0).abs() < 0.1, "item={item:?}");
}

#[tokio::test]
async fn column_flex_item_definite_flex_basis_resolves_child_percentage_height() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 10pt } body { margin:0 }\
         .col { display:flex; flex-direction:column }\
         .item { height:0; flex:0 0 100pt }\
         .item > div { width:100pt; height:100%; background:green }</style>\
         <div class=\"col\"><div class=\"item\"><div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!(
        (green.width() - 100.0).abs() < 0.01 && (green.height() - 100.0).abs() < 0.01,
        "percentage-height child should resolve against the flex item's definite flex-basis height: {green:?}"
    );
}

#[tokio::test]
async fn column_flex_item_mixed_percentage_min_max_height_resolves_against_container() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 260pt; margin: 10pt } body { margin: 0 }\
         .col { display:flex; flex-direction:column; width:80pt; height:100pt; margin-bottom:10pt }\
         .min { flex:0 0 auto; width:20pt; height:10pt; min-height:calc(50% + 10pt); background:green }\
         .max { flex:0 0 auto; width:20pt; height:90pt; max-height:calc(50% + 10pt); background:blue }\
         </style><div class=\"col\"><div class=\"min\"></div></div><div class=\"col\"><div class=\"max\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let min = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let max = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((min.height() - 60.0).abs() < 0.1, "min={min:?}");
    assert!((max.height() - 60.0).abs() < 0.1, "max={max:?}");
}

#[tokio::test]
async fn flex_auto_minimum_size_is_zero_for_scrollable_overflow() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:100pt; margin:0 0 10pt }\
         .item { flex:1 1 0; background:red; white-space:nowrap }\
         .fixed { flex:0 0 50pt; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"item\" style=\"overflow:hidden\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>\
         <div class=\"row\"><div class=\"item\" style=\"overflow:clip\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].width() - 50.0).abs() < 0.01,
        "scrollable overflow should allow auto min-size to shrink to zero: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].width() > 100.0,
        "non-scrollable overflow:clip should keep content-based auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn row_flex_auto_minimum_size_uses_overflow_x() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 160pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .row { display:flex; width:100pt; margin:0 0 10pt }\
         .item { flex:1 1 0; background:red; white-space:nowrap }\
         .fixed { flex:0 0 50pt; height:10pt; background:blue }\
         </style>\
         <div class=\"row\"><div class=\"item\" style=\"overflow-x:hidden; overflow-y:clip\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>\
         <div class=\"row\"><div class=\"item\" style=\"overflow-x:clip; overflow-y:visible\">WWWWWWWWWWWWWWWWWWWW</div><div class=\"fixed\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].width() - 50.0).abs() < 0.01,
        "row flex should use scrollable overflow-x for main-axis auto min-size: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].width() > 100.0,
        "row flex should ignore scrollable overflow-y for main-axis auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn row_flex_min_width_auto_uses_zero_for_non_visible_overflow_x() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 260pt; margin: 10pt } body { margin:0 }\
         .flexbox { display:flex; width:30pt; margin-bottom:2pt }\
         .item { border:2pt dotted purple; background:red }\
         .item > div { width:80pt; height:40pt }\
         </style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:visible\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:hidden\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:scroll\"><div></div></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"overflow-x:auto\"><div></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 4);
    assert!(
        (red_rects[0].width() - 84.0).abs() < 0.01,
        "visible overflow should keep the row flex item's content-based auto min-width: {:?}",
        red_rects[0]
    );
    for rect in &red_rects[1..] {
        assert!(
            (rect.width() - 30.0).abs() < 0.01,
            "non-visible overflow-x should resolve min-width:auto to zero and allow shrinkage: {rect:?}"
        );
    }
}

#[tokio::test]
async fn column_flex_auto_minimum_size_uses_overflow_y() {
    let lines = "A\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA\nA";
    let html = format!(
        "<style>@page {{ size: 260pt 320pt; margin: 10pt }} body {{ margin:0; font-size:10pt; line-height:10pt }}\
         .col {{ display:flex; flex-direction:column; height:100pt; width:40pt; margin:0 0 10pt }}\
         .item {{ flex:1 1 0; background:red; white-space:pre-line }}\
         .fixed {{ flex:0 0 50pt; width:40pt; background:blue }}\
         </style>\
         <div class=\"col\"><div class=\"item\" style=\"overflow-y:hidden; overflow-x:clip\">{lines}</div><div class=\"fixed\"></div></div>\
         <div class=\"col\"><div class=\"item\" style=\"overflow-y:clip; overflow-x:visible\">{lines}</div><div class=\"fixed\"></div></div>"
    );
    let document = Html::from_string(&html)
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].height() - 50.0).abs() < 0.01,
        "column flex should use scrollable overflow-y for main-axis auto min-size: {:?}",
        red_rects[0]
    );
    assert!(
        red_rects[1].height() > 100.0,
        "column flex should ignore scrollable overflow-x for main-axis auto min-size: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn flex_main_axis_auto_margin_absorbs_free_space() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 } .row { display:flex; width:200pt } .a { width:40pt; height:10pt; background:red } .b { margin-left:auto; width:30pt; height:10pt; background:blue }</style><div class=\"row\"><div class=\"a\"></div><div class=\"b\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 180.0).abs() < 0.01);
}

#[tokio::test]
async fn supports_flex_wrap_and_flex_basis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 140pt; margin: 10pt } body { margin: 0 } .wrap { display:flex; flex-wrap:wrap; align-content:space-between; width:100pt; height:100pt } .wrap div { flex: 1 50%; height:10pt }</style><div class=\"wrap\"><div style=\"background:red\"></div><div style=\"background:blue\"></div><div style=\"background:green\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();
    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert_eq!(red.width(), 50.0);
    assert_eq!(blue.width(), 50.0);
    assert!((red.y() - blue.y()).abs() < 0.01);
    assert!(green.y() < red.y() - 50.0);
}

#[tokio::test]
async fn row_flex_item_page_break_before_does_not_create_standalone_pages() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0 } .flexbox { display: flex; flex-wrap: wrap; float: left; width: 60pt; height: 20pt; border: 1pt dashed black; margin: 0 2pt 4pt 0 } .item { width: 28pt; border: 1pt solid blue; background: lightblue } .clear { clear: both }</style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: always\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: left\"></div></div>\
         <div class=\"clear\"></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: right\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: always\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let item_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(173, 216, 230)))
        .collect::<Vec<_>>();
    assert_eq!(item_rects.len(), 7);
    assert!((item_rects[0].height() - 20.0).abs() < 0.01);
    assert!((item_rects[1].x() - item_rects[0].x() - 64.0).abs() < 0.01);
    assert!((item_rects[1].y() - item_rects[0].y()).abs() < 0.01);
    assert!((item_rects[3].x() - item_rects[2].x() - 30.0).abs() < 0.01);
    assert!((item_rects[3].y() - item_rects[2].y()).abs() < 0.01);
    assert!((item_rects[2].height() - 20.0).abs() < 0.01);
    assert!((item_rects[3].height() - 20.0).abs() < 0.01);
    assert!((item_rects[5].x() - item_rects[4].x() - 30.0).abs() < 0.01);
    assert!((item_rects[5].y() - item_rects[4].y()).abs() < 0.01);
    assert!((item_rects[6].x() - item_rects[4].x()).abs() < 0.01);
    assert!(
        (item_rects[4].y() - item_rects[6].y() - 10.0).abs() < 0.01,
        "item rects: {item_rects:?}"
    );
    for rect in &item_rects[4..] {
        assert!((rect.height() - 10.0).abs() < 0.01);
    }
}

#[tokio::test]
async fn column_flex_item_page_break_before_does_not_create_standalone_pages() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 180pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; flex-direction: column; float: left; width: 20pt; height: 60pt; border: 1pt dashed black; margin: 0 2pt 4pt 0 }\
         .item { height: 28pt; border: 1pt solid blue; background: lightblue; background-clip: padding-box } .clear { clear: both }</style>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: always\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\" style=\"page-break-before: left\"></div></div>\
         <div class=\"clear\"></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: right\"></div></div>\
         <div class=\"flexbox\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\" style=\"page-break-before: always\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages.len(), 1);
    let item_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(173, 216, 230)))
        .collect::<Vec<_>>();
    assert_eq!(item_rects.len(), 7);
    assert!(
        (item_rects[0].height() - 28.0).abs() < 0.01,
        "item rects: {item_rects:?}"
    );
    assert!((item_rects[1].x() - item_rects[0].x() - 24.0).abs() < 0.01);
    assert!((item_rects[1].y() - item_rects[0].y()).abs() < 0.01);
    assert!((item_rects[3].x() - item_rects[2].x()).abs() < 0.01);
    assert!((item_rects[2].y() - item_rects[3].y() - 30.0).abs() < 0.01);
    assert!((item_rects[5].x() - item_rects[4].x()).abs() < 0.01);
    assert!((item_rects[4].y() - item_rects[5].y() - 20.0).abs() < 0.01);
    assert!((item_rects[5].y() - item_rects[6].y() - 20.0).abs() < 0.01);
    for rect in &item_rects[4..] {
        assert!((rect.height() - 18.0).abs() < 0.01);
    }
}

#[tokio::test]
async fn oversized_flex_container_at_page_top_does_not_create_leading_blank_page() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; width: 40pt; height: 140pt; background: green }\
         .item { width: 20pt; height: 20pt }</style>\
         <div class=\"flexbox\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    assert_eq!(document.pages.len(), 2);
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.height() - 80.0).abs() < 0.01),
        "oversized flex container should start on the first page without a leading blank page"
    );
    assert!(
        document.pages[1]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.height() - 60.0).abs() < 0.01),
        "oversized flex container should continue on the next page"
    );
}

#[tokio::test]
async fn column_wrapped_flex_container_honors_min_height_without_wrapping() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 500pt; margin: 10pt } body { margin: 0 }\
         .flexbox { display: flex; flex-direction: column; flex-wrap: wrap; border: 1px dashed black; width: 12px; min-height: 100px; margin-right: 2px; float: left }\
         .smallItem { height: 30px; border: 1px solid blue; background: lightblue; background-clip: padding-box }\
         </style>\
         <div class=\"flexbox\"></div>\
         <div class=\"flexbox\"><div class=\"smallItem\"></div></div>\
         <div class=\"flexbox\"><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div></div>\
         <div class=\"flexbox\" style=\"max-height: 120px\"><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div><div class=\"smallItem\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let mut item_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(173, 216, 230)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();
    item_rects.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then_with(|| b.1.partial_cmp(&a.1).unwrap())
    });
    let expected = [
        (23.5, 466.0, 7.5, 22.5),
        (35.5, 466.0, 7.5, 22.5),
        (35.5, 442.0, 7.5, 22.5),
        (35.5, 418.0, 7.5, 22.5),
        (35.5, 394.0, 7.5, 22.5),
        (35.5, 370.0, 7.5, 22.5),
        (47.5, 466.0, 3.0, 22.5),
        (47.5, 442.0, 3.0, 22.5),
        (47.5, 418.0, 3.0, 22.5),
        (52.0, 466.0, 3.0, 22.5),
        (52.0, 442.0, 3.0, 22.5),
    ];

    assert_eq!(item_rects.len(), expected.len());
    for (actual, expected) in item_rects.iter().zip(expected) {
        assert!(
            (actual.0 - expected.0).abs() < 0.01
                && (actual.1 - expected.1).abs() < 0.01
                && (actual.2 - expected.2).abs() < 0.01
                && (actual.3 - expected.3).abs() < 0.01,
            "expected item rect {expected:?}, got {actual:?}"
        );
    }
}

#[tokio::test]
async fn column_flex_auto_height_treats_zero_percent_flex_basis_as_content() {
    let target = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"display: flex; flex-direction: column; border: 1px solid purple\">\
         <div>Header</div><div style=\"flex: 1\">Flexible content<br></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(
        "<style>@page { size: 220pt 120pt; margin: 10pt } body { margin: 0 }</style>\
         <div style=\"border: 1px solid purple\">\
         <div>Header</div><div>Flexible content<br></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let target_text = target.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();
    let reference_text = reference.pages[0]
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.x(), line.y()))
        .collect::<Vec<_>>();

    assert_eq!(target_text, reference_text);

    let target_border = target.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();
    let reference_border = reference.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)))
        .map(|rect| (rect.x(), rect.y(), rect.width(), rect.height()))
        .collect::<Vec<_>>();

    assert_eq!(target_border, reference_border);
}

#[tokio::test]
async fn flex_container_creates_anonymous_items_for_nbsp_text_runs() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; justify-content: flex-end; width: 300pt; height: 40pt; font-size: 12pt; line-height: 14.4pt }\
         .item { width: 50pt; height: 30pt } .a { background: red } .b { background: green } .c { background: blue }</style>\
         <div class=\"row\"><div class=\"item a\"></div>&nbsp;<div class=\"item b\"></div>&nbsp;<div class=\"item c\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((blue.x() - 260.0).abs() < 0.01);
    assert!(red.x() < 160.0, "anonymous NBSP items must consume width");
    assert!(
        green.x() - red.x() - 50.0 > 2.5,
        "gap between flex items should include the NBSP anonymous flex item"
    );
}

#[tokio::test]
async fn flex_container_ignores_preserved_document_whitespace_text_runs() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 }\
         .row { display: flex; justify-content: flex-end; white-space: pre; width: 100pt; height: 20pt; font-size: 12pt; line-height: 14.4pt }\
         .item { width: 20pt; height: 20pt } .a { background: red } .b { background: blue }</style>\
         <div class=\"row\">\n\t<div class=\"item a\"></div>\n\t<div class=\"item b\"></div>\n</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!(
        (red.x() - 70.0).abs() < 0.01,
        "preserved document whitespace should not affect flex-end placement: {red:?}"
    );
    assert!(
        (blue.x() - 90.0).abs() < 0.01,
        "second flex item should immediately follow the first: red={red:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn flex_column_item_definite_height_resolves_anonymous_descendant_percentage_heights() {
    let document = Html::from_string(
        "<!DOCTYPE html><meta charset=\"utf-8\">\
         <style>@page { size: 300px 260px; margin: 0 } body { margin: 0 }\
         .flexbox { display: flex; flex-direction: column; width: 200px; height: 200px }</style>\
         <div class=\"flexbox\"><div style=\"height: 50%\">\
         <button style=\"box-sizing: border-box; width: 200px; height: 100%; padding: 0; border: 0\">\
         <div style=\"width: 200px; height: 100%; background-color: green\"></div>\
         </button></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("percentage-height descendant should paint green");

    assert!(
        (green.width() - 150.0).abs() < 0.01,
        "green rectangle should be 200 CSS px wide: {green:?}"
    );
    assert!(
        (green.height() - 75.0).abs() < 0.01,
        "green rectangle should be 100 CSS px tall: {green:?}"
    );
}

#[tokio::test]
async fn inline_block_fragment_lays_out_atomic_inline_children_in_one_line() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } body { margin: 0 }\
         .outer { display: inline-block; text-align: right; width: 300pt; height: 40pt; font-size: 12pt; line-height: 14.4pt }\
         .item { display: inline-block; width: 50pt; height: 30pt } .a { background: red } .b { background: green } .c { background: blue }</style>\
         <div class=\"outer\"><div class=\"item a\"></div> <div class=\"item b\"></div> <div class=\"item c\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(CssColor::new(255, 0, 0));
    let green = rect(CssColor::new(0, 128, 0));
    let blue = rect(CssColor::new(0, 0, 255));

    assert!((blue.x() - 260.0).abs() < 0.01);
    assert!(
        red.x() < 160.0,
        "inline spaces should affect right alignment"
    );
    assert!(
        green.x() - red.x() - 50.0 > 2.5,
        "inline-block children should share a line with whitespace gaps"
    );
}

#[tokio::test]
async fn inline_flex_exports_first_item_baseline() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexContainer { display: inline-flex; background: lightblue }\
         .smallFont { font-size: 10px; line-height: 10px }\
         .bigFont { font-size: 20px; line-height: 20px }</style>\
         a <div class=\"flexContainer\"><div class=\"smallFont\">b</div><div class=\"bigFont\">c</div></div>\
         <div class=\"flexContainer\"><div class=\"bigFont\">d</div><div class=\"smallFont\">e</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };

    let a = line("a");
    let b = line("b");
    let d = line("d");
    assert!(
        (b.y() - a.y()).abs() < 0.01,
        "expected b baseline {} to match a baseline {}",
        b.y(),
        a.y()
    );
    assert!(
        (d.y() - a.y()).abs() < 0.01,
        "expected d baseline {} to match a baseline {}",
        d.y(),
        a.y()
    );
}

#[tokio::test]
async fn inline_flex_exports_shared_baseline_over_startmost_table_item() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } \
         body { margin: 0; font: 10pt/10pt sans-serif } \
         .flexContainer { display: inline-flex; height: 80pt; align-items: center; background: lightblue } \
         .flexContainer > div { display: table; width: 30pt } \
         .start { align-self: flex-start; font: 8pt/8pt sans-serif } \
         .baseline { align-self: baseline; font: 20pt/20pt sans-serif }</style>\
         a <div class=\"flexContainer\"><div class=\"start\">b</div><div class=\"baseline\">c</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };

    let surrounding = line("a");
    let baseline_participant = line("c");
    assert!(
        (baseline_participant.y() - surrounding.y()).abs() < 0.01,
        "the shared flex-line baseline should align with surrounding inline text: participant={}, surrounding={}",
        baseline_participant.y(),
        surrounding.y()
    );
}

#[tokio::test]
async fn inline_flex_baseline_uses_first_order_modified_item() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 100pt; margin: 10pt } body { margin: 0 }\
         .flexContainer { display: inline-flex; background: lightblue }\
         .smallFont { font-size: 10px; line-height: 10px }\
         .bigFont { font-size: 20px; line-height: 20px }\
         .smallOrder { order: -1 } .bigOrder { order: 30 }</style>\
         a <div class=\"flexContainer\"><div class=\"bigFont\">c</div><div class=\"smallFont smallOrder\">b</div></div>\
         <div class=\"flexContainer\"><div class=\"smallFont bigOrder\">e</div><div class=\"bigFont\">d</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line = |text: &str| {
        document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text.trim() == text)
            .unwrap_or_else(|| panic!("{text} should render"))
    };

    let a = line("a");
    let b = line("b");
    let d = line("d");
    assert!(
        (b.y() - a.y()).abs() < 0.01,
        "expected ordered b baseline {} to match a baseline {}",
        b.y(),
        a.y()
    );
    assert!(
        (d.y() - a.y()).abs() < 0.01,
        "expected ordered d baseline {} to match a baseline {}",
        d.y(),
        a.y()
    );
}

#[tokio::test]
async fn empty_inline_flex_synthesizes_baseline_from_margin_box() {
    let document = Html::from_string(
        "<style>@page { size: 360pt 120pt; margin: 10pt } body { margin: 0; font: 20pt/20pt sans-serif }\
         .flexContainer { display: inline-flex; height: 16pt; width: 16pt; background: purple; border: 0pt dotted black }\
         </style>\
         A\
         <div class=\"flexContainer\"></div>\
         <div class=\"flexContainer\" style=\"padding-bottom: 20pt\"></div>\
         <div class=\"flexContainer\" style=\"padding: 10pt\"></div>\
         <div class=\"flexContainer\" style=\"border-width: 3pt\"></div>\
         <div class=\"flexContainer\" style=\"border-bottom-width: 4pt\"></div>\
         <div class=\"flexContainer\" style=\"border-bottom-width: 4pt; margin: 2pt\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let purple = CssColor::new(128, 0, 128);
    let purple_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(purple))
        .collect::<Vec<_>>();
    assert_eq!(
        purple_rects.len(),
        6,
        "each empty inline-flex background should paint once: {purple_rects:?}"
    );

    let baseline_bottom = purple_rects[0].y();
    for rect in purple_rects.iter().take(5) {
        assert!(
            (rect.y() - baseline_bottom).abs() < 0.01,
            "empty inline-flex border-box bottoms should share the synthesized baseline: {purple_rects:?}"
        );
    }
    let margin_rect = purple_rects[5];
    assert!(
        (margin_rect.y() - 2.0 - baseline_bottom).abs() < 0.01,
        "empty inline-flex with bottom margin should align its margin-box bottom: {purple_rects:?}"
    );
}

#[tokio::test]
async fn flex_flow_wrap_align_content_stretch_stretches_lines() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 } #flexbox { background: red; align-content: center; align-content: stretch; display: flex; flex-flow: wrap; height: 75pt; width: 225pt } #flexbox div { background-color: green; width: 112.5pt }</style><div id=\"flexbox\"><div></div><div></div><div></div><div></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();
    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();

    assert_eq!(green_rects.len(), 4);
    for rect in &green_rects {
        assert!((rect.width() - 112.5).abs() < 0.01);
        assert!((rect.height() - 37.5).abs() < 0.01);
    }
    let top = green_rects
        .iter()
        .map(|rect| rect.y() + rect.height())
        .fold(f32::MIN, f32::max);
    let bottom = green_rects
        .iter()
        .map(|rect| rect.y())
        .fold(f32::MAX, f32::min);

    assert!((red.height() - 75.0).abs() < 0.01);
    assert!((top - (red.y() + red.height())).abs() < 0.01);
    assert!((bottom - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn column_reverse_wrap_reverse_places_lines_in_reversed_cross_axis_order() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 10pt } body { margin: 0 } .container { display: flex; flex-direction: column-reverse; flex-wrap: wrap-reverse; height: 90pt; width: 150pt } .container > div { width: 40pt } .a, .b, .c { height: 25pt } .d, .e { height: 40pt } .f { height: 85pt } .a { background: red } .b { background: green } .c { background: blue } .d { background: yellow } .e { background: magenta } .f { background: cyan }</style><div class=\"container\"><div class=\"f\"></div><div class=\"e\"></div><div class=\"d\"></div><div class=\"c\"></div><div class=\"b\"></div><div class=\"a\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let a = rect(CssColor::new(255, 0, 0));
    let b = rect(CssColor::new(0, 128, 0));
    let c = rect(CssColor::new(0, 0, 255));
    let d = rect(CssColor::new(255, 255, 0));
    let e = rect(CssColor::new(255, 0, 255));
    let f = rect(CssColor::new(0, 255, 255));

    assert!((a.x() - b.x()).abs() < 0.01);
    assert!((b.x() - c.x()).abs() < 0.01);
    assert!((d.x() - e.x()).abs() < 0.01);
    assert!(a.x() < d.x());
    assert!(d.x() < f.x());
    assert!(a.y() > b.y());
    assert!(b.y() > c.y());
    assert!(d.y() > e.y());
}

#[tokio::test]
async fn row_reverse_wrap_places_multiline_items_in_reverse_main_axis_order() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 260pt; margin: 10pt } body { margin: 0 }\
         .container { display: flex; flex-direction: row-reverse; flex-wrap: wrap; width: 225pt }\
         p { margin: 12pt 7.5pt 12pt 0; background: #ccc; font-size: 12pt; line-height: 14.4pt }\
         .w90 { width: 67.5pt } .w140 { width: 105pt } .w290 { width: 217.5pt }\
         </style><div class=\"container\">\
         <p class=\"w90\">1-3</p><p class=\"w90\">1-2</p><p class=\"w90\">1-1</p>\
         <p class=\"w140\">2-2</p><p class=\"w140\">2-1</p><p class=\"w290\">3-1</p>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(204, 204, 204)))
        .collect::<Vec<_>>();

    assert_eq!(rects.len(), 6);
    let expected = [
        (160.0, 223.6, 67.5, 14.4),
        (85.0, 223.6, 67.5, 14.4),
        (10.0, 223.6, 67.5, 14.4),
        (122.5, 185.2, 105.0, 14.4),
        (10.0, 185.2, 105.0, 14.4),
        (10.0, 146.8, 217.5, 14.4),
    ];
    for (index, (rect, (x, y, width, height))) in rects.iter().zip(expected).enumerate() {
        assert!((rect.x() - x).abs() < 0.01, "{index}: x {}", rect.x());
        assert!((rect.y() - y).abs() < 0.01, "{index}: y {}", rect.y());
        assert!(
            (rect.width() - width).abs() < 0.01,
            "{index}: width {}",
            rect.width()
        );
        assert!(
            (rect.height() - height).abs() < 0.01,
            "{index}: height {}",
            rect.height()
        );
    }
}

#[tokio::test]
async fn order_with_row_reverse_matches_right_floated_reference() {
    let style = "<style>@page { size: 800pt 300pt; margin: 10pt } body { margin: 0 }</style>";
    let target = Html::from_string(format!(
        "{style}<style>\
         #test {{ display: flex; flex-direction: row-reverse }}\
         #leftmost {{ order: 1 }} #middle {{ order: 0 }} #rightmost {{ order: -1 }}\
         </style>\
         <p>Test passes if the paragraph below reads 'First,Second,Third' from leftmost.</p>\
         <div id=\"test\"><p id=\"leftmost\">First,</p><p id=\"middle\">Second,</p><p id=\"rightmost\">Third</p></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let reference = Html::from_string(format!(
        "{style}<style>#leftmost, #middle, #rightmost {{ float: right }}</style>\
         <p>Test passes if the paragraph below reads 'First,Second,Third' from leftmost.</p>\
         <div id=\"test\"><p id=\"rightmost\">Third</p><p id=\"middle\">Second,</p><p id=\"leftmost\">First,</p></div>",
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let line_positions = |document: &spindrift::Document| {
        ["First,", "Second,", "Third"]
            .into_iter()
            .map(|text| {
                let line = document.pages[0]
                    .lines()
                    .iter()
                    .find(|line| line.text == text)
                    .unwrap_or_else(|| panic!("{text} should render"));
                (line.text.clone(), line.x(), line.y())
            })
            .collect::<Vec<_>>()
    };

    let target_lines = line_positions(&target);
    let reference_lines = line_positions(&reference);
    let target_row_y = target_lines
        .first()
        .map(|(_, _, y)| *y)
        .expect("target row should contain text");
    for ((target_text, target_x, target_y), (reference_text, reference_x, _reference_y)) in
        target_lines.iter().zip(&reference_lines)
    {
        assert_eq!(target_text, reference_text);
        assert!(
            (target_x - reference_x).abs() < 0.01,
            "{target_text}: target x {target_x}, reference x {reference_x}"
        );
        assert!(
            (target_y - target_row_y).abs() < 0.01,
            "{target_text}: target y {target_y}, row y {target_row_y}"
        );
    }
}

#[tokio::test]
async fn floated_flex_container_min_content_contains_inflexible_auto_basis_item_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .red { position: absolute; background: red; width: 75pt; height: 75pt; z-index: -1 } .outer { width: 0 } .flex { display: flex; float: left; background: green; height: 75pt } .item { flex: 0 0 auto } .inline-block { float: left; width: 75pt }</style><div class=\"red\"></div><div class=\"outer\"><div class=\"flex\"><div class=\"item\"><div class=\"inline-block\"></div><div class=\"inline-block\"></div></div></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("floated flex container background should paint");
    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("absolute red reference should paint behind it");

    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn absolute_flex_children_use_flex_static_position_and_ignore_justify_self() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .container { display: flex; flex-flow: row; float: left; padding: 1px 2px; border: 1px solid black; background: yellow; margin: 0 5px 5px 0; height: 10px; width: 16px } .container > div { position: absolute; background: teal; height: 6px; width: 8px }</style><div class=\"container\"><div style=\"justify-self: auto\"></div></div><div class=\"container\"><div style=\"justify-self: center\"></div></div><div class=\"container\"><div style=\"justify-self: end\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let yellow_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    let teal_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 128)))
        .collect::<Vec<_>>();

    assert_eq!(yellow_rects.len(), 3);
    assert_eq!(teal_rects.len(), 3);
    for (container, child) in yellow_rects.iter().zip(teal_rects.iter()) {
        assert!((child.x() - (container.x() + 2.25)).abs() < 0.01);
        assert!(
            (child.y() + child.height() - (container.y() + container.height() - 1.5)).abs() < 0.01
        );
    }
}

#[tokio::test]
async fn floated_empty_block_children_preserve_backgrounds_in_scoped_paint() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 120pt; margin: 10pt } body { margin: 0 } .container { float: left; padding: 1px 2px; border: 1px solid black; background: yellow; margin: 0 5px 5px 0; height: 10px; width: 16px } .container > div { background: teal; height: 6px; width: 8px }</style><div class=\"container\"><div></div></div><div class=\"container\"><div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let yellow_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 255, 0)))
        .collect::<Vec<_>>();
    let teal_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 128)))
        .collect::<Vec<_>>();

    assert_eq!(yellow_rects.len(), 2);
    assert_eq!(teal_rects.len(), 2, "{:#?}", document.pages[0].rects());
    for (container, child) in yellow_rects.iter().zip(teal_rects.iter()) {
        assert!(
            (child.x() - (container.x() + 2.25)).abs() < 0.01,
            "{child:?}"
        );
        assert!(
            (child.y() + child.height() - (container.y() + container.height() - 1.5)).abs() < 0.01,
            "container={container:?}, child={child:?}"
        );
    }
}

#[tokio::test]
async fn absolute_flex_children_ignore_flex_basis_for_auto_width() {
    let document = Html::from_string(
        r#"<style>
         @page { size: 80pt 180pt; margin: 0 }
         body { margin: 0 }
         .flex {
           display: flex;
           height: 10px;
           width: 10px;
           background: purple;
           margin-bottom: 5px;
           position: relative;
         }
         .flex > * {
           position: absolute;
           background: teal;
           height: 10px;
         }
         .sized { width: 10px }
         .implied { left: 0; right: 0 }
         </style>
         <div class="flex"><div style="flex-basis: 2px"></div></div>
         <div class="flex"><div style="flex-basis: 100px"></div></div>
         <div class="flex"><div style="flex-basis: 80%"></div></div>
         <div class="flex"><div style="flex-basis: content"></div></div>
         <div class="flex"><div class="sized" style="flex-basis: 2px"></div></div>
         <div class="flex"><div class="sized" style="flex-basis: 100px"></div></div>
         <div class="flex"><div class="sized" style="flex-basis: 80%"></div></div>
         <div class="flex"><div class="sized" style="flex-basis: content"></div></div>
         <div class="flex"><div class="implied" style="flex-basis: 2px"></div></div>
         <div class="flex"><div class="implied" style="flex-basis: 100px"></div></div>
         <div class="flex"><div class="implied" style="flex-basis: 80%"></div></div>
         <div class="flex"><div class="implied" style="flex-basis: content"></div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let visible_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.width() > 0.01 && rect.height() > 0.01)
        .collect::<Vec<_>>();
    let purple_rects = visible_rects
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 0, 128)));
    let teal_rects = visible_rects
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 128)))
        .collect::<Vec<_>>();

    assert_eq!(purple_rects.count(), 12);
    assert_eq!(
        teal_rects.len(),
        8,
        "auto-width abspos flex children should shrink-wrap to zero and leave the first four containers purple: {teal_rects:?}"
    );
    for child in teal_rects {
        assert!(
            (child.width() - 7.5).abs() < 0.01,
            "explicit and inset-constrained abspos flex children should stay 10 CSS px wide: {child:?}"
        );
    }
}

#[tokio::test]
async fn flex_root_honors_align_items_and_percent_height() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } html { display: flex; height: 100%; align-items: center; justify-content: center } body { margin: 0; width: 20pt; height: 20pt; font-size: 10pt; line-height: 10pt }</style><body>X</body>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "X")
        .unwrap();

    assert!((line.x() - 40.0).abs() < 0.01);
    assert_line_baseline_at_top(&document, line, 60.0);
}

#[tokio::test]
async fn column_flex_indefinite_percentage_flex_basis_uses_content() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 200pt; margin: 10pt } body { margin:0; font-size:10pt; line-height:10pt }\
         .col { display:flex; flex-direction:column; width:60pt; margin:0 0 10pt }\
         .definite { height:100pt }\
         .item { flex:0 0 50%; background:red }\
         </style>\
         <div class=\"col\"><div class=\"item\">A</div></div>\
         <div class=\"col definite\"><div class=\"item\">A</div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(red_rects.len(), 2);
    assert!(
        (red_rects[0].height() - 10.0).abs() < 0.01,
        "indefinite percentage flex-basis should use content height: {:?}",
        red_rects[0]
    );
    assert!(
        (red_rects[1].height() - 50.0).abs() < 0.01,
        "definite percentage flex-basis should resolve against container height: {:?}",
        red_rects[1]
    );
}

#[tokio::test]
async fn column_flex_indefinite_zero_percent_flex_basis_ignores_authored_height() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 520pt; margin: 10pt } body { margin:0 }\
         .container { background:red; display:flex; flex-direction:column; width:75pt }\
         .item { flex:0 0 0%; height:375pt; background:red }\
         .child { width:75pt; height:75pt; background:green }\
         </style><div class=\"container\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("content-height child should paint");

    assert!((green.width() - 75.0).abs() < 0.01, "green={green:?}");
    assert!((green.height() - 75.0).abs() < 0.01, "green={green:?}");
    assert_eq!(
        final_rect_fill_at(&document.pages[0], green.x() + 37.5, green.y() + 37.5),
        Some(CssColor::new(0, 128, 0))
    );
    assert!(
        document.pages[0]
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .all(|rect| rect.height() <= 75.0 + 0.01),
        "indefinite 0% flex-basis should use content height, not the authored height: {:?}",
        document.pages[0].rects()
    );
}

#[tokio::test]
async fn column_flex_zero_percent_flex_basis_only_falls_back_when_indefinite() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 220pt; margin: 10pt } body { margin:0 }\
         .col { display:flex; flex-direction:column; width:20pt; margin-bottom:10pt }\
         .definite { height:40pt }\
         .item { height:40pt; min-height:0; overflow:hidden; background:red }\
         .percent .item { flex:0 0 0% }\
         .length .item { flex:0 0 0px }\
         .child { width:20pt; height:20pt; background:green }\
         </style>\
         <div class=\"col percent\"><div class=\"item\"><div class=\"child\"></div></div></div>\
         <div class=\"col percent definite\"><div class=\"item\"><div class=\"child\"></div></div></div>\
         <div class=\"col length\"><div class=\"item\"><div class=\"child\"></div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .collect::<Vec<_>>();
    let red_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .collect::<Vec<_>>();

    assert_eq!(
        green_rects.len(),
        1,
        "only the indefinite 0% flex-basis item should reveal its clipped content: {green_rects:?}"
    );
    assert!((green_rects[0].height() - 20.0).abs() < 0.01);
    assert!(
        red_rects.iter().all(|rect| rect.height() <= 20.0),
        "definite 0% and 0px flex-basis must not use the authored 40pt height: {red_rects:?}"
    );
}

#[tokio::test]
async fn flex_start_items_cover_cross_start_gradient_band() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; align-items: flex-start; width: 225pt; height: 75pt;\
           background: linear-gradient(to bottom, red 0, red 37.5pt, green 37.5pt, green 75pt) }\
         .item { width: 112.5pt; height: 38.25pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red_band_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(255, 0, 0)) && rect.height() == 37.5)
        .expect("gradient red band should be painted");
    let green_items = page
        .rects()
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 112.5).abs() < 0.01
                && (rect.height() - 38.25).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 2);
    for (green_index, green) in green_items {
        let red = &page.rects()[red_band_index];
        assert!(green_index > red_band_index);
        assert!(green.y() <= red.y());
        assert!(green.y() + green.height() >= red.y() + red.height());
    }
}

#[tokio::test]
async fn align_content_flex_end_packs_lines_against_cross_end() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 140pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-flow: wrap; align-content: flex-end; width: 225pt; height: 75pt;\
           background: linear-gradient(to bottom, green 0, green 37.5pt, red 37.5pt, red 75pt) }\
         .item { width: 112.5pt; height: 19.5pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let page = &document.pages[0];
    let red_band_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::new(255, 0, 0)) && rect.height() == 37.5)
        .expect("gradient red band should be painted");
    let red = &page.rects()[red_band_index];
    let green_items = page
        .rects()
        .iter()
        .enumerate()
        .filter(|(_, rect)| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 112.5).abs() < 0.01
                && (rect.height() - 19.5).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 4);
    let bottom = green_items
        .iter()
        .map(|(_, rect)| rect.y())
        .fold(f32::MAX, f32::min);
    let top = green_items
        .iter()
        .map(|(_, rect)| rect.y() + rect.height())
        .fold(f32::MIN, f32::max);
    assert!(bottom <= red.y());
    assert!(top >= red.y() + red.height());
    for (green_index, _) in green_items {
        assert!(green_index > red_band_index);
    }
}

#[tokio::test]
async fn flex_place_content_expands_to_align_and_justify_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 130pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; flex-wrap: wrap; place-content: flex-end space-between; width: 100pt; height: 80pt }\
         .item { width: 40pt; height: 10pt; background: green }\
         </style><div class=\"flex\"><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green_items = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 40.0).abs() < 0.01
                && (rect.height() - 10.0).abs() < 0.01
        })
        .collect::<Vec<_>>();

    assert_eq!(green_items.len(), 4);
    let min_x = green_items
        .iter()
        .map(|rect| rect.x())
        .fold(f32::MAX, f32::min);
    let max_x = green_items
        .iter()
        .map(|rect| rect.x())
        .fold(f32::MIN, f32::max);
    assert!((min_x - 10.0).abs() < 0.1, "min_x={min_x}");
    assert!((max_x - 70.0).abs() < 0.1, "max_x={max_x}");
}

#[tokio::test]
async fn flex_gap_accepts_css_math_functions() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; gap: calc(5pt + 5pt); width: 120pt }\
         .item { width: 20pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background: green\"></div><div class=\"item\" style=\"background: blue\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - (green.x() + green.width() + 10.0)).abs() < 0.1);
}

#[tokio::test]
async fn vertical_rl_column_flex_gap_uses_physical_horizontal_axis() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin:0 }\
         .flex { writing-mode: vertical-rl; display:flex; flex-direction:column; gap:10pt; width:80pt; height:20pt; background:green }\
         .item { flex:0 0 auto; width:20pt; height:10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:blue\"></div><div class=\"item\" style=\"background:black\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let black = rect(CssColor::new(0, 0, 0));

    assert!((red.x() - 70.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 40.0).abs() < 0.01, "blue={blue:?}");
    assert!((black.x() - 10.0).abs() < 0.01, "black={black:?}");
    assert!((red.y() - blue.y()).abs() < 0.01 && (blue.y() - black.y()).abs() < 0.01);
}

#[tokio::test]
async fn vertical_rl_row_wrap_stacks_lines_from_physical_right() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-flow:row wrap; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         .h > div { writing-mode:horizontal-tb }\
         </style>\
         <div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>\
         <div class=\"flex h\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let rects = |color| {
        page.rects()
            .iter()
            .filter(|rect| rect.fill == Some(color))
            .collect::<Vec<_>>()
    };
    let cyans = rects(CssColor::new(0, 255, 255));
    let magentas = rects(CssColor::new(255, 0, 255));
    let yellows = rects(CssColor::new(255, 255, 0));
    let blacks = rects(CssColor::new(0, 0, 0));

    assert_eq!(cyans.len(), 2, "{:?}", page.rects());
    assert_eq!(magentas.len(), 2, "{:?}", page.rects());
    assert_eq!(yellows.len(), 2, "{:?}", page.rects());
    assert_eq!(blacks.len(), 2, "{:?}", page.rects());

    for ((cyan, magenta), yellow, black) in cyans
        .into_iter()
        .zip(magentas)
        .zip(yellows)
        .zip(blacks)
        .map(|(((cyan, magenta), yellow), black)| ((cyan, magenta), yellow, black))
    {
        assert!((cyan.width() - 20.0).abs() < 0.01 && (cyan.height() - 15.0).abs() < 0.01);
        assert!((cyan.x() - 20.0).abs() < 0.01, "cyan={cyan:?}");
        assert!((magenta.x() - 20.0).abs() < 0.01, "magenta={magenta:?}");
        assert!((yellow.x() - 0.0).abs() < 0.01, "yellow={yellow:?}");
        assert!((black.x() - 0.0).abs() < 0.01, "black={black:?}");
        assert!(
            (cyan.y() - yellow.y()).abs() < 0.01,
            "cyan={cyan:?}, yellow={yellow:?}"
        );
        assert!(
            (magenta.y() - black.y()).abs() < 0.01,
            "magenta={magenta:?}, black={black:?}"
        );
        assert!(
            ((cyan.y() - magenta.y()).abs() - 15.0).abs() < 0.01,
            "cyan={cyan:?}, magenta={magenta:?}"
        );
    }
}

#[tokio::test]
async fn vertical_rl_row_wrap_reverse_stacks_lines_from_physical_left() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-flow:row wrap-reverse; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         </style><div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let cyan = rect(CssColor::new(0, 255, 255));
    let magenta = rect(CssColor::new(255, 0, 255));
    let yellow = rect(CssColor::new(255, 255, 0));
    let black = rect(CssColor::new(0, 0, 0));

    assert!((cyan.x() - 0.0).abs() < 0.01, "cyan={cyan:?}");
    assert!((magenta.x() - 0.0).abs() < 0.01, "magenta={magenta:?}");
    assert!((yellow.x() - 20.0).abs() < 0.01, "yellow={yellow:?}");
    assert!((black.x() - 20.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_rl_row_align_items_flex_start_uses_physical_right() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-rl; flex-direction:row; align-items:flex-start; width:40pt; height:30pt }\
         .item { width:20pt; height:15pt; background:green }\
         </style><div class=\"flex\"><div class=\"item\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((green.x() - 20.0).abs() < 0.01, "green={green:?}");
}

#[tokio::test]
async fn vertical_rl_row_flex_items_use_vertical_inline_forced_breaks() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 120pt 140pt; margin: 10pt } body { margin: 0 }\
         .container { display: flex; flex-flow: row; writing-mode: vertical-rl; border: 2pt solid black; height: 90pt }\
         .item { line-height: 0; float: right }\
         .color-block { display: inline-block; width: 15pt; height: 45pt }\
         </style><div class=\"container\">\
         <div class=\"item\"><span class=\"color-block\" style=\"background: orange\"></span><br><span class=\"color-block\" style=\"background: grey\"></span></div>\
         <div class=\"item\"><span class=\"color-block\" style=\"background: blue\"></span><br><span class=\"color-block\" style=\"background: yellow\"></span></div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let grey = rect(CssColor::new(128, 128, 128));
    let orange = rect(CssColor::new(255, 165, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let yellow = rect(CssColor::new(255, 255, 0));

    assert!(
        (grey.y() - orange.y()).abs() < 0.01 && (yellow.y() - blue.y()).abs() < 0.01,
        "each flex item should keep its forced-break columns on the same inline row: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        grey.y() > yellow.y() + 40.0 && orange.y() > blue.y() + 40.0,
        "row flex main axis should place the first item above the second: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        grey.x() + grey.width() <= orange.x() + 0.01
            && yellow.x() + yellow.width() <= blue.x() + 0.01,
        "vertical-rl forced breaks should put second-line blocks to the physical left: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
    assert!(
        (grey.x() - yellow.x()).abs() < 0.01 && (orange.x() - blue.x()).abs() < 0.01,
        "colors should form columns in clockwise WPT order: grey={grey:?}, orange={orange:?}, yellow={yellow:?}, blue={blue:?}"
    );
}

#[tokio::test]
async fn vertical_lr_row_wrap_stacks_lines_from_physical_left() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin:0 }\
         .flex { display:flex; writing-mode:vertical-lr; flex-flow:row wrap; align-content:flex-start; width:40pt; height:30pt }\
         .flex > div { width:20pt; height:15pt }\
         </style><div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let cyan = rect(CssColor::new(0, 255, 255));
    let magenta = rect(CssColor::new(255, 0, 255));
    let yellow = rect(CssColor::new(255, 255, 0));
    let black = rect(CssColor::new(0, 0, 0));

    assert!((cyan.x() - 0.0).abs() < 0.01, "cyan={cyan:?}");
    assert!((magenta.x() - 0.0).abs() < 0.01, "magenta={magenta:?}");
    assert!((yellow.x() - 20.0).abs() < 0.01, "yellow={yellow:?}");
    assert!((black.x() - 20.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_rl_column_flex_ignores_direction_for_block_axis() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 60pt; margin: 0 } body { margin:0 }\
         .flex { writing-mode:vertical-rl; direction:rtl; display:flex; flex-direction:column; gap:10pt; width:80pt; height:20pt }\
         .item { flex:0 0 auto; width:20pt; height:10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background:red\"></div><div class=\"item\" style=\"background:blue\"></div><div class=\"item\" style=\"background:black\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let rect = |color| {
        document.pages[0]
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(color))
            .unwrap()
    };
    let red = rect(CssColor::new(255, 0, 0));
    let blue = rect(CssColor::new(0, 0, 255));
    let black = rect(CssColor::new(0, 0, 0));

    assert!((red.x() - 60.0).abs() < 0.01, "red={red:?}");
    assert!((blue.x() - 30.0).abs() < 0.01, "blue={blue:?}");
    assert!((black.x() - 0.0).abs() < 0.01, "black={black:?}");
}

#[tokio::test]
async fn vertical_rl_rtl_column_wrapping_uses_vertical_inline_cross_start() {
    for (flex_flow, expected) in [
        (
            "column wrap",
            [
                (CssColor::new(0, 0, 0), 0.0, 15.0),
                (CssColor::new(255, 255, 0), 20.0, 15.0),
                (CssColor::new(255, 0, 255), 0.0, 0.0),
                (CssColor::new(0, 255, 255), 20.0, 0.0),
            ],
        ),
        (
            "column wrap-reverse",
            [
                (CssColor::new(255, 0, 255), 0.0, 15.0),
                (CssColor::new(0, 255, 255), 20.0, 15.0),
                (CssColor::new(0, 0, 0), 0.0, 0.0),
                (CssColor::new(255, 255, 0), 20.0, 0.0),
            ],
        ),
        (
            "column-reverse wrap",
            [
                (CssColor::new(255, 255, 0), 0.0, 15.0),
                (CssColor::new(0, 0, 0), 20.0, 15.0),
                (CssColor::new(0, 255, 255), 0.0, 0.0),
                (CssColor::new(255, 0, 255), 20.0, 0.0),
            ],
        ),
        (
            "column-reverse wrap-reverse",
            [
                (CssColor::new(0, 255, 255), 0.0, 15.0),
                (CssColor::new(255, 0, 255), 20.0, 15.0),
                (CssColor::new(255, 255, 0), 0.0, 0.0),
                (CssColor::new(0, 0, 0), 20.0, 0.0),
            ],
        ),
    ] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 50pt 40pt; margin: 0 }} body {{ margin:0 }}\
             .flex {{ display:flex; flex-flow:{flex_flow}; width:40pt; height:30pt; direction:rtl; writing-mode:vertical-rl }}\
             .flex > div {{ flex:0 0 auto; width:20pt; height:15pt }}\
             </style><div class=\"flex\"><div style=\"background:cyan\"></div><div style=\"background:magenta\"></div><div style=\"background:yellow\"></div><div style=\"background:black\"></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let rect = |color| {
            document.pages[0]
                .rects()
                .iter()
                .find(|rect| rect.fill == Some(color))
                .unwrap_or_else(|| {
                    panic!(
                        "{flex_flow} should paint {color:?}: {:?}",
                        document.pages[0].rects()
                    )
                })
        };
        let top_y = document.pages[0]
            .rects()
            .iter()
            .map(|rect| rect.y())
            .fold(f32::INFINITY, f32::min);
        let bottom_y = document.pages[0]
            .rects()
            .iter()
            .map(|rect| rect.y())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (bottom_y - top_y - 15.0).abs() < 0.01,
            "{flex_flow} should have two 15pt rows: {:?}",
            document.pages[0].rects()
        );
        for (color, x, y) in expected {
            let rect = rect(color);
            let expected_y = if y == 0.0 { top_y } else { bottom_y };
            assert!(
                (rect.x() - x).abs() < 0.01 && (rect.y() - expected_y).abs() < 0.01,
                "{flex_flow} placed {color:?} at {rect:?}, expected ({x}, {expected_y}); rects={:?}",
                document.pages[0].rects()
            );
        }
    }
}

#[tokio::test]
async fn vertical_inline_block_and_inline_flex_atoms_use_logical_inline_size() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<link rel="help" href="https://drafts.csswg.org/css-align-3/#generate-baselines">
<link rel="help" href="https://www.w3.org/TR/css-inline-3/#valdef-dominant-baseline-auto">
<style>
#inline-block {
  display: inline-block;
  width: 100px;
  height: 50px;
  background: green;
}

#inline-flex {
  display: inline-flex;
}

#inline-flex > div {
  width: 100px;
  height: 50px;
  background: green;
}
</style>
<p>Test passes if there is a filled green square.</p>
<div style="width: 100px; height: 100px; line-height: 0; writing-mode: vertical-rl; background: red;">
  <span id="inline-block"></span><span id="inline-flex"><div></div></span>
</div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut green_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .cloned()
        .collect::<Vec<_>>();
    green_rects.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap());

    assert_eq!(green_rects.len(), 2, "{green_rects:?}");
    let lower = &green_rects[0];
    let upper = &green_rects[1];
    assert!(
        (lower.x() - upper.x()).abs() < 0.01
            && (lower.width() - upper.width()).abs() < 0.01
            && (lower.height() - upper.height()).abs() < 0.01
            && (lower.width() - lower.height() * 2.0).abs() < 0.01
            && (lower.y() + lower.height() - upper.y()).abs() < 0.01,
        "vertical inline atoms should stack into one square: {green_rects:?}"
    );

    let page = &document.pages[0];
    assert_eq!(
        final_rect_fill_at(
            page,
            lower.x() + lower.width() / 2.0,
            lower.y() + lower.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0))
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            upper.x() + upper.width() / 2.0,
            upper.y() + upper.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0))
    );
}

#[tokio::test]
async fn column_inline_flex_logical_block_margins_match_gap_spacing() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 140pt; margin: 10pt } body { margin:0; direction:rtl }\
         section { display:inline-flex; flex-direction:column; background:green }\
         section > div { width:50pt; height:10pt; background:gray }\
         .spaced { margin-block-end:15pt }\
         </style><section><div class=\"spaced\"></div><div class=\"spaced\"></div><div></div></section>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let green = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("inline-flex background should paint");
    assert!(
        (green.height() - 60.0).abs() < 0.01,
        "two 15pt logical block-end margins should create column gaps: {green:?}"
    );

    let gray_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(128, 128, 128)))
        .collect::<Vec<_>>();
    assert_eq!(gray_rects.len(), 3);
    assert!(
        (gray_rects[0].y() - gray_rects[1].y() - 25.0).abs() < 0.01
            && (gray_rects[1].y() - gray_rects[2].y() - 25.0).abs() < 0.01,
        "successive 10pt items should be separated by 15pt logical block-end margins: {gray_rects:?}"
    );
}

#[tokio::test]
async fn flex_visibility_collapse_removes_item_from_main_axis_layout() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 100pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; width: 120pt }\
         .item { width: 40pt; height: 10pt }\
         </style><div class=\"flex\"><div class=\"item\" style=\"background: green\"></div><div class=\"item\" style=\"visibility: collapse; background: red\"></div><div class=\"item\" style=\"background: blue\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(
        !document.pages[0]
            .rects()
            .iter()
            .any(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
    );
    let blue = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
        .unwrap();

    assert!((blue.x() - 50.0).abs() < 0.1, "blue.x()={}", blue.x());
}

#[tokio::test]
async fn flex_visibility_collapse_preserves_row_cross_size_strut() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 120pt; margin: 10pt } body { margin: 0 }\
         .flex { display: flex; background: black; width: 120pt }\
         .short { width: 10pt; height: 10pt; background: green }\
         .tall { visibility: collapse; width: 40pt; height: 40pt; background: red }\
         </style><div class=\"flex\"><div class=\"short\"></div><div class=\"tall\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let flex_background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert!((flex_background.height() - 40.0).abs() < 0.1);
}

#[tokio::test]
async fn flex_root_preserves_subpoint_absolute_lengths() {
    let document = Html::from_string(
        r#"<style>
        @page { size: landscape; margin: 0 }
        body { margin: 0 }
        .root {
          align-items: center;
          background: #eef1f5;
          display: flex;
          height: 595.2756pt;
          justify-content: center;
          width: 841.8898pt;
        }
        .card {
          background: white;
          height: 8cm;
          width: 25cm;
        }
        </style><div class="root"><div class="card"></div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let body = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::WHITE))
        .unwrap();

    // CSS Values and Units defines 1in = 96px = 72pt; flex layout must not
    // round the used box size/location before PDF painting.
    let expected_width = 25.0 * 72.0 / 2.54;
    let expected_height = 8.0 * 72.0 / 2.54;
    assert!((body.width() - expected_width).abs() < 0.001);
    assert!((body.height() - expected_height).abs() < 0.001);
    assert!((body.x() - ((841.8898 - expected_width) / 2.0)).abs() < 0.001);
}

#[tokio::test]
async fn flex_root_paints_html_background_on_page_canvas() {
    let document = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 10pt } :root { --page: #eef1f5 } html { display: flex; height: 100%; background: var(--page) } body { margin: 0 }</style><body>X</body>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert!(document.pages[0].rects().iter().any(|rect| {
        rect.x() == 10.0
            && rect.y() == 10.0
            && rect.width() == 80.0
            && rect.height() == 80.0
            && rect.fill == Some(CssColor::new(238, 241, 245))
    }));
}

#[tokio::test]
async fn flex_parent_background_paints_before_child_backgrounds() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 80pt; margin: 10pt } body { margin: 0 } .parent { display: flex; background: white; width: 80pt; height: 40pt } .child { background: black; width: 40pt; height: 20pt }</style><div class=\"parent\"><div class=\"child\"></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let white_index = document.pages[0]
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::WHITE))
        .unwrap();
    let black_index = document.pages[0]
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert!(white_index < black_index);
}

#[tokio::test]
async fn nested_column_flex_item_uses_intrinsic_auto_width() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body { margin: 0; font-size: 10pt; line-height: 10pt } .row { display: flex; width: 200pt } .fill { flex-grow: 1; height: 20pt } .stub { display: flex; flex-direction: column; background: black; padding: 0 10pt } .stub p { margin: 0 }</style><div class=\"row\"><div class=\"fill\"></div><div class=\"stub\"><p>Stub</p></div></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let stub = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();

    assert!(stub.width() > 20.0);
    assert!(stub.width() < 80.0);
    assert!(stub.x() > 120.0);
}

#[tokio::test]
async fn flex_auto_basis_preserves_non_growing_item_content_width() {
    let document = Html::from_file("weasyprint-samples/invoice/invoice.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let developers_line = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.contains("Our awesome developers"))
        .or_else(|| {
            let first_line = ["Our", "awesome", "developers"]
                .into_iter()
                .map(|text| {
                    document.pages[0]
                        .lines()
                        .iter()
                        .find(|line| line.text == text)
                })
                .collect::<Option<Vec<_>>>()?;
            first_line
                .windows(2)
                .all(|pair| (pair[0].y() - pair[1].y()).abs() < 0.1)
                .then_some(first_line[0])
        })
        .expect("invoice developer text should stay on one line");

    assert!(developers_line.x() > 380.0);
}

#[tokio::test]
async fn flex_auto_basis_border_box_includes_padding_and_border() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let divider = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(42, 50, 57))
                // The sample authors a 1pt border. Used border widths are
                // layout geometry, so it remains one PDF point at the paint
                // boundary rather than being converted as a CSS pixel.
                && (rect.width() - 1.0).abs() < 0.01
                && rect.height() > 2.0
                && rect.height() < 4.0
        })
        .min_by(|left, right| left.y().total_cmp(&right.y()))
        .unwrap();

    assert!((divider.x() - 598.84).abs() < 1.0);
}

#[tokio::test]
async fn shrink_to_fit_inline_block_uses_exact_graph_max_content_width() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "CDG ✈ LFLL" && line.font_size == 25.0)
    );
}

#[tokio::test]
async fn flex_item_text_line_fit_uses_sequence_backed_max_content_width() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert!(
        document.pages[0]
            .lines()
            .iter()
            .any(|line| line.text == "THÉODORE MARCELIN" && line.font_size == 18.0)
    );
}

#[tokio::test]
async fn inline_origin_abspos_uses_inline_static_position() {
    let document = Html::from_file("weasyprint-samples/ticket/ticket.html")
        .await
        .unwrap()
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let name = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "THÉODORE MARCELIN" && line.font_size == 25.0)
        .unwrap();
    let destination = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "CDG ✈ LFLL" && line.font_size == 25.0)
        .unwrap();

    assert!(
        (name.y() - destination.y()).abs() < 0.01,
        "name y={} destination y={}",
        name.y(),
        destination.y()
    );
}

#[tokio::test]
async fn absolutely_positioned_inline_block_shrink_wraps_auto_width() {
    let document = Html::from_string(
        "<style>@page { size: 220pt 100pt; margin: 10pt } body { margin: 0 } .box { position: relative; width: 160pt; height: 40pt; font-size: 10pt; line-height: 10pt } h1 { display: inline-block; position: absolute; right: 0; margin: 0; font-size: 10pt; line-height: 10pt; font-weight: 400 }</style><div class=\"box\"><h1>Wide Label</h1></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let lines = &document.pages[0].lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "Wide Label");
    assert!(lines[0].x() > 100.0);
}

#[tokio::test]
async fn inline_block_content_participates_in_parent_inline_line() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt }</style><p>Before <span style=\"display:inline-block\">Box</span> After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let boxed = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Box")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!(before.x() < boxed.x());
    assert!(boxed.x() < after.x());
    assert!((before.y() - after.y()).abs() < 0.1);
}

#[tokio::test]
async fn inline_block_block_child_paints_above_atom_background() {
    let document = Html::from_string(
        "<style>@page { size: 320px 140px; margin: 0 } body { margin: 0; font-size: 0; line-height: 0 } .empty, .box { display: inline-block; vertical-align: top; width: 100px; height: 100px } .box { background: red } .box > div { width: 100px; height: 100px; background: green }</style><div class=\"empty\"></div><div class=\"box\"><div></div></div><div class=\"empty\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("inline-block background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("block child background should paint");
    let red_index = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
    let green_index = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));

    assert!(
        red_index < green_index,
        "inline-block background must paint before its in-flow block child"
    );
    assert_eq!(
        final_rect_fill_at(
            page,
            red.x() + red.width() / 2.0,
            red.y() + red.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0)),
    );
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_absolute_child_escapes_pseudo_context_at_static_position() {
    let document = Html::from_string(
        "<style>@page { size: 320px 140px; margin: 0 } body { margin: 0; font-size: 0; line-height: 0 } .empty, .box { display: inline-block; vertical-align: top; width: 100px; height: 100px } .box { background: red } .box > div { position: absolute; width: 100px; height: 100px; background: green }</style><div class=\"empty\"></div><div class=\"box\"><div></div></div><div class=\"empty\"></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("inline-block background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolute child background should paint");

    assert_eq!(
        final_rect_fill_at(
            page,
            red.x() + red.width() / 2.0,
            red.y() + red.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0)),
    );
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_absolute_child_static_position_uses_atom_origin_after_text() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<title>Static position inside inline-block</title>
<link rel="author" title="Martin Robinson" href="mrobinson@igalia.com">
<link rel="help" href="https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width" title="10.3.7 Absolutely positioned, non-replaced elements">
<link rel="match" href="static-inside-inline-block-ref.html">

<p>Test passes if there is a filled green square and <strong>no red</strong>.</p>
<div style="display: inline-block; width: 100px; height: 100px;"></div>
<div style="display: inline-block; width: 100px; height: 100px; background: red;">
    <div style="position: absolute; width: 100px; height: 100px; background: green;"></div>
</div>
<div style="display: inline-block; width: 100px; height: 100px;"></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("inline-block background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolute child background should paint");

    assert_eq!(
        final_rect_fill_at(
            page,
            red.x() + red.width() / 2.0,
            red.y() + red.height() / 2.0
        ),
        Some(CssColor::new(0, 128, 0)),
        "an auto-inset descendant of a static inline-block must replay at the atom origin: red={red:?}, green={green:?}",
    );
    assert!((green.x() - red.x()).abs() < 0.01);
    assert!((green.y() - red.y()).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_explicit_absolute_child_keeps_page_resolved_insets() {
    let document = Html::from_string(
        "<style>\
         @page { size: 320px 160px; margin: 20px }\
         body { margin: 0; font-size: 0; line-height: 0 }\
         .empty, .box { display: inline-block; vertical-align: top; width: 100px; height: 100px }\
         .box { background: red }\
         .box > div { position: absolute; left: 0; top: 0; width: 20px; height: 20px; background: green }\
         </style>\
         <div class=\"empty\"></div><div class=\"box\"><div></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let red = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("inline-block background should paint");
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("absolute child background should paint");

    assert!(
        green.x() < red.x(),
        "explicit left:0 should resolve against the page containing block, not the inline-block atom: green={green:?}, red={red:?}"
    );
    assert!(
        (green.x() - 15.0).abs() < 0.01,
        "20px page margin should place left:0 at 15pt: {green:?}"
    );
}

#[tokio::test]
async fn nested_absolute_children_follow_their_atomic_inline_coordinate_space() {
    for (writing_mode, direction) in [
        ("horizontal-tb", "ltr"),
        ("horizontal-tb", "rtl"),
        ("sideways-rl", "ltr"),
        ("sideways-rl", "rtl"),
        ("sideways-lr", "ltr"),
        ("sideways-lr", "rtl"),
    ] {
        let document = Html::from_string(format!(
            "<style>\
             @page {{ size: 320px 240px; margin: 0 }}\
             body {{ margin: 0; font-size: 0; line-height: 0 }}\
             .spacer, .atom {{ display: inline-block; vertical-align: top; width: 80px; height: 80px }}\
             .atom {{ position: relative; writing-mode: {writing_mode}; direction: {direction}; background: blue }}\
             .parent {{ position: absolute; padding: 7px; margin: 5px; inline-size: auto; text-indent: 11px }}\
             .child {{ position: absolute; inset-inline-start: 0; inset-block-start: 0; width: 20px; height: 20px }}\
             .red {{ background: red }} .green {{ background: green }}\
             </style>\
             <div class=\"spacer\"></div><div class=\"atom\"><div class=\"parent\"><div class=\"child red\"></div><div class=\"child green\"></div></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let atom = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .unwrap();
        let red = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .unwrap();
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .unwrap();
        assert!(
            (red.x() - green.x()).abs() < 0.01 && (red.y() - green.y()).abs() < 0.01,
            "nested absolute siblings must share their atom-local containing block in {writing_mode} {direction}: red={red:?}, green={green:?}"
        );
        assert!(
            green.x() >= atom.x() - 1.0
                && green.y() >= atom.y() - 1.0
                && green.x() + green.width() <= atom.x() + atom.width() + 1.0
                && green.y() + green.height() <= atom.y() + atom.height() + 1.0,
            "nested absolute descendants must replay inside their final atom in {writing_mode} {direction}: atom={atom:?}, green={green:?}"
        );
    }
}

async fn atom_owned_parent_and_child_x(
    atom_display: &str,
    writing_mode: &str,
    direction: &str,
    parent_left: f32,
    child_left: Option<f32>,
) -> (f32, f32) {
    let child_left = child_left
        .map(|left| format!("left: {left}px;"))
        .unwrap_or_default();
    let document = Html::from_string(format!(
        "<style>\
         @page {{ size: 320px 240px; margin: 0 }}\
         body {{ margin: 0; font-size: 0; line-height: 0 }}\
         .spacer {{ display: inline-block; vertical-align: top; width: 80px; height: 80px }}\
         .atom {{ display: {atom_display}; vertical-align: top; width: 80px; height: 80px; position: relative; writing-mode: {writing_mode}; direction: {direction}; background: blue }}\
         .parent {{ position: absolute; left: {parent_left}px; padding: 7px; background: red }}\
         .child {{ position: absolute; {child_left} width: 20px; height: 20px; background: green }}\
         </style>\
         <div class=spacer></div><div class=atom><div class=parent><div class=child></div></div></div>"
    ))
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let parent = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("positioned parent background should paint");
    let child = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("positioned child background should paint");
    (parent.x(), child.x())
}

#[tokio::test]
async fn atom_owned_auto_child_preserves_positioned_parent_displacement() {
    for atom_display in ["inline-block", "inline-flex"] {
        for (writing_mode, direction) in [
            ("horizontal-tb", "ltr"),
            ("horizontal-tb", "rtl"),
            ("sideways-rl", "ltr"),
            ("sideways-rl", "rtl"),
            ("sideways-lr", "ltr"),
            ("sideways-lr", "rtl"),
        ] {
            let (auto_parent_start, auto_child_start) =
                atom_owned_parent_and_child_x(atom_display, writing_mode, direction, 0.0, None)
                    .await;
            let (auto_parent_end, auto_child_end) =
                atom_owned_parent_and_child_x(atom_display, writing_mode, direction, 20.0, None)
                    .await;
            let (explicit_parent_start, explicit_child_start) = atom_owned_parent_and_child_x(
                atom_display,
                writing_mode,
                direction,
                0.0,
                Some(0.0),
            )
            .await;
            let (explicit_parent_end, explicit_child_end) = atom_owned_parent_and_child_x(
                atom_display,
                writing_mode,
                direction,
                20.0,
                Some(0.0),
            )
            .await;

            for (label, parent_delta, child_delta) in [
                (
                    "automatic child",
                    auto_parent_end - auto_parent_start,
                    auto_child_end - auto_child_start,
                ),
                (
                    "explicit child control",
                    explicit_parent_end - explicit_parent_start,
                    explicit_child_end - explicit_child_start,
                ),
            ] {
                assert!(
                    (parent_delta - 15.0).abs() < 0.01,
                    "{atom_display} {writing_mode} {direction} {label}: parent delta"
                );
                assert!(
                    (child_delta - parent_delta).abs() < 0.01,
                    "{atom_display} {writing_mode} {direction} {label} must retain its atom-owned parent's displacement: parent_delta={parent_delta}, child_delta={child_delta}",
                );
            }
        }
    }
}

#[tokio::test]
async fn inline_block_does_not_create_implicit_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:20pt; background:black }</style><p>A<span>B</span>C</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "C")
        .unwrap();

    assert!((after.x() - (background.x() + background.width())).abs() < 1.0);
}

#[tokio::test]
async fn inline_block_preserves_explicit_collapsed_spaces() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:20pt; background:black }</style><p>A <span>B</span> C</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK))
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "C")
        .unwrap();

    assert!(first_visible_glyph_x(after) - (background.x() + background.width()) > 2.0);
}

#[tokio::test]
async fn inline_block_paints_atomic_box_before_following_inline_text() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 100pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:5pt; background:black }</style><p>Before <span>Box</span> After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() > 40.0)
        .unwrap();
    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let boxed = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Box")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!((background.width() - 50.0).abs() < 0.1);
    assert!(before.x() < background.x());
    assert!(boxed.x() > background.x());
    assert!(first_visible_glyph_x(after) > background.x() + background.width());
}

#[tokio::test]
async fn inline_block_explicit_height_is_not_expanded_by_line_height() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 100pt; margin: 10pt } body, p { margin: 0 } span { display:inline-block; width:28.5pt; height:28.5pt; border:0.75pt solid #32cd32; background:green; font-size:12pt; line-height:30pt; color:white }</style><p><span>1</span></p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap();

    assert!((background.height() - 30.0).abs() < 0.01);
}

#[tokio::test]
async fn inline_block_middle_alignment_does_not_inflate_wrapped_rows() {
    let document = Html::from_string(
        "<style>@page { size: 180pt 180pt; margin: 0 } body { margin: 0 }\
         .wrapper { width:90pt; height:90pt; background:red; direction:ltr; writing-mode:horizontal-tb }\
         .wrapper div { display:inline-block; width:28.5pt; height:28.5pt; border:0.75pt solid #32cd32;\
             background:green; color:white; font-size:12pt; line-height:30pt; text-align:center; vertical-align:middle }\
         </style><div class=\"wrapper\"><div>1</div><div>2</div><div>3</div><div>4</div><div>5</div><div>6</div><div>7</div><div>8</div><div>9</div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let wrapper = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .expect("wrapper background should paint");
    let mut green_rects = page
        .rects()
        .iter()
        .filter(|rect| {
            rect.fill == Some(CssColor::new(0, 128, 0))
                && (rect.width() - 30.0).abs() < 0.01
                && (rect.height() - 30.0).abs() < 0.01
        })
        .collect::<Vec<_>>();
    green_rects.sort_by(|left, right| {
        left.y()
            .total_cmp(&right.y())
            .then_with(|| left.x().total_cmp(&right.x()))
    });
    assert_eq!(
        green_rects.len(),
        9,
        "expected nine 30pt green backgrounds: {green_rects:?}"
    );

    let mut row_bottoms = green_rects.iter().map(|rect| rect.y()).collect::<Vec<_>>();
    row_bottoms.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    assert_eq!(row_bottoms.len(), 3, "expected three rows: {row_bottoms:?}");
    assert!(
        row_bottoms
            .windows(2)
            .all(|pair| (pair[1] - pair[0] - 30.0).abs() < 0.01),
        "inline-block rows should advance by exactly 30pt: {row_bottoms:?}"
    );
    assert!(
        (row_bottoms[0] - wrapper.y()).abs() < 0.01
            && (row_bottoms[2] + 30.0 - (wrapper.y() + wrapper.height())).abs() < 0.01,
        "green rows should cover the 90pt wrapper with no exposed red: wrapper={wrapper:?}, rows={row_bottoms:?}"
    );
}

#[tokio::test]
async fn inline_block_lays_out_block_children_as_atomic_fragment() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:4pt; background:black } b { display:block; font-weight:400 }</style><p>Before <span><b>One</b><b>Two</b></span> After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let background = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() > 40.0)
        .unwrap();
    let before = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "Before")
        .unwrap();
    let one = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "One")
        .unwrap();
    let two = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "Two")
        .unwrap();
    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text.trim() == "After")
        .unwrap();

    assert!((background.width() - 48.0).abs() < 0.1);
    assert!(background.height() >= 32.0);
    assert!(before.x() < background.x());
    assert!(one.x() > background.x());
    assert!(two.x() > background.x());
    assert!(two.y() < one.y());
    assert!(first_visible_glyph_x(after) > background.x() + background.width());
}

#[tokio::test]
async fn inline_block_fragment_replays_through_paint_operation_stream() {
    let document = Html::from_string(
        "<style>@page { size: 260pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 12pt } span { display:inline-block; width:40pt; padding:4pt; background:black } b { display:block; font-weight:400 }</style><p>Before <span><b>One</b><b>Two</b></span> After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let page = &document.pages[0];

    let background_index = page
        .rects()
        .iter()
        .position(|rect| rect.fill == Some(CssColor::BLACK) && rect.width() > 40.0)
        .unwrap();
    let one_index = page
        .lines()
        .iter()
        .position(|line| line.text == "One")
        .unwrap();
    let two_index = page
        .lines()
        .iter()
        .position(|line| line.text == "Two")
        .unwrap();

    let background_operation = page
        .operations()
        .iter()
        .position(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Rect(index) if *index == background_index)
        })
        .unwrap();
    let one_operation = page
        .operations()
        .iter()
        .position(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Line(index) if *index == one_index)
        })
        .unwrap();
    let two_operation = page
        .operations()
        .iter()
        .position(|operation| {
            matches!(operation, crate::document::paint::page::PaintOperation::Line(index) if *index == two_index)
        })
        .unwrap();

    assert!(background_operation < one_operation);
    assert!(one_operation < two_operation);
}

#[tokio::test]
async fn flex_items_are_blockified_for_painting() {
    let document = Html::from_string(
        "<style>@page { size: 140pt 80pt; margin: 10pt } body { margin: 0 } .flex { display:flex; width:100pt } address { flex:1 50%; height:10pt; background:red }</style><div class=\"flex\"><address>Item</address></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let red = document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
        .unwrap();

    assert_eq!(red.width(), 100.0);
    assert_eq!(document.pages[0].lines()[0].text, "Item");
}

#[tokio::test]
async fn supports_flex_column() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 120pt; margin: 10pt } body { margin: 0 }</style><div style=\"display:flex; flex-direction:column; font-size:10pt; line-height:10pt\"><span>One</span><span>Two</span></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "One");
    assert_eq!(document.pages[0].lines()[1].text, "Two");
    assert!(document.pages[0].lines()[1].y() < document.pages[0].lines()[0].y());
}

#[tokio::test]
async fn flex_inline_svg_rows_ignore_formatting_whitespace_for_height() {
    let document = Html::from_string(
        "<style>@page { size: 200pt 200pt; margin: 20pt } html, body, p { margin: 0; font-size: 10pt; line-height: 10pt }</style><div style=\"display:flex; margin:0\"><div style=\"flex-grow:1\">\n<svg width=\"15\" height=\"15\"><rect width=\"15\" height=\"15\" fill=\"#2292d4\" /></svg>\n<small> Half Match </small>\n<svg width=\"15\" height=\"15\"><rect width=\"15\" height=\"15\" fill=\"#175377\" /></svg>\n<small> Full Match </small>\n</div></div><p>After</p>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let after = document.pages[0]
        .lines()
        .iter()
        .find(|line| line.text == "After")
        .unwrap();

    assert_eq!(
        document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec![" Half Match ", " Full Match", "After"]
    );
    // The selected-font ascent is quantized by the shaping backend, so assert
    // the following line's physical top with a sub-pixel tolerance rather
    // than reconstructing its baseline from the embedded-font metadata.
    assert!(
        (rendered_line_baseline_top(&document, after) - 166.0).abs() < 0.2,
        "After top={}",
        rendered_line_baseline_top(&document, after)
    );
}

#[tokio::test]
async fn column_flex_block_wrappers_reserve_svg_used_height() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0 }\
         .stack { display:flex; flex-direction:column; align-items:flex-start; gap:8pt; width:75pt }\
         .diagram { width:75pt }</style>\
         <div class=\"stack\"><div class=\"diagram\"><svg style=\"display:block;width:100%;height:auto\" viewBox=\"0 0 150 30\"><rect width=\"150\" height=\"30\" fill=\"red\"/></svg></div>\
         <div class=\"diagram\"><svg style=\"display:block;width:100%;height:auto\" viewBox=\"0 0 150 30\"><rect width=\"150\" height=\"30\" fill=\"blue\"/></svg></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let first = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(CssColor::new(255, 0, 0)))
        .and_then(|path| path.paint_bounds())
        .unwrap();
    let second = document.pages[0]
        .paths()
        .iter()
        .find(|path| path.fill == Some(CssColor::new(0, 0, 255)))
        .and_then(|path| path.paint_bounds())
        .unwrap();

    assert!((first.size.height - 15.0).abs() < 0.01, "first={first:?}");
    assert!(
        (second.size.height - 15.0).abs() < 0.01,
        "second={second:?}"
    );
    assert!(
        (first.origin.y - second.origin.y).abs() >= 23.0 - 0.01,
        "the 8pt flex gap must follow the first 15pt SVG box: first={first:?}, second={second:?}"
    );
}

#[tokio::test]
async fn supports_min_max_block_dimensions() {
    let document = Html::from_string(
        "<div style=\"margin: 0; width: 50pt; min-width: 80pt; height: 50pt; max-height: 20pt; background: red\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects()[0];
    assert_eq!(rect.width(), 80.0);
    assert_eq!(rect.height(), 20.0);
}

#[tokio::test]
async fn supports_border_box_sizing() {
    let document = Html::from_string(
        "<div style=\"margin: 0; box-sizing: border-box; width: 50pt; height: 20pt; padding: 2pt; border: 1pt solid black; background: red\"></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();

    let rect = &document.pages[0].rects()[0];
    assert_eq!(rect.width(), 50.0);
    assert_eq!(rect.height(), 20.0);
}

#[tokio::test]
async fn collects_inline_children_and_line_breaks() {
    let document = Html::from_string("<p>Hello <span>nested</span><br>line &amp; more</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    assert_eq!(document.pages[0].lines()[0].text, "Hello nested");
    assert_eq!(document.pages[0].lines()[1].text, "line & more");
    assert_eq!(document.pages[0].lines().len(), 2);
}

#[tokio::test]
async fn mixed_block_and_inline_content_keeps_document_order() {
    let document = Html::from_string(
        "<div><div><strong>Othram</strong><br>Address</div><strong>Disclaimer</strong><br>Text</div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lines, ["Othram", "Address", "Disclaimer", "Text"]);
}

#[tokio::test]
async fn block_parents_do_not_duplicate_heading_text_or_split_plain_spans() {
    let document = Html::from_string(
        "<div><h4>Parameters</h4><p>Segment detection: <span>&ge;7 cM &bull;&nbsp;</span><span>&ge;200 SNPs &bull;&nbsp;</span><span>0 &le; MAF &le; 0.5 &bull;&nbsp;</span><span>MB 100 SNPs</span></p></div>",
    )
    .render(&RenderOptions::default()).await
    .unwrap();
    let lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        lines.iter().filter(|line| **line == "Parameters").count(),
        1
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Segment detection:") && line.contains("MB 100 SNPs"))
    );
}

#[tokio::test]
async fn wrapped_inline_fragments_keep_line_text_coalesced_and_trimmed() {
    let text = "alpha beta gamma delta epsilon";
    let document = Html::from_string(format!(
        "<style>@page {{ size: 80pt 200pt; margin: 10pt }} body, p {{ margin: 0; font-size: 10pt; line-height: 10pt }}</style><p>{text}</p>"
    ))
    .render(&RenderOptions::default()).await
    .unwrap();

    let rendered_lines = document.pages[0]
        .lines()
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(rendered_lines.len() > 1);
    assert_eq!(rendered_lines.join(" "), text);
    assert!(
        rendered_lines
            .iter()
            .all(|line| !line.starts_with(' ') && !line.ends_with(' '))
    );
}

#[tokio::test]
async fn block_outline_paints_after_child_content() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 }\
         .parent { width: 40pt; height: 40pt; outline: 2pt solid red }\
         .child { width: 20pt; height: 20pt; background: blue }</style>\
         <div class=\"parent\"><div class=\"child\"></div></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let child_operation = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
    let outline_operation = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));

    assert!(
        outline_operation > child_operation,
        "outline should paint after descendant content: child={child_operation}, outline={outline_operation}"
    );
}

/// Spindrift's CSS-UI-permitted compatibility policy paints an ordinary in-flow
/// outline before an auto/zero-z positioned sibling, regardless of whether
/// the in-flow formatting context is block, Grid, Flex, or a table.
#[tokio::test]
async fn normal_flow_outlines_precede_auto_positioned_siblings() {
    for (name, normal) in [
        ("block", "<div class=\"normal\"></div>"),
        (
            "grid",
            "<div class=\"normal grid\"><div class=\"item\"></div></div>",
        ),
        (
            "flex",
            "<div class=\"normal flex\"><div class=\"item\"></div></div>",
        ),
        (
            "table",
            "<table class=\"table\"><tbody class=\"normal\"><tr><td></td></tr></tbody></table>",
        ),
    ] {
        let document = Html::from_string(format!(
            "<style>@page {{ size: 120pt 120pt; margin: 0 }} body {{ margin: 0 }}\
             .outer {{ position: relative; width: 60pt; height: 60pt }}\
             .normal {{ width: 40pt; height: 40pt; outline: 2pt solid red }}\
             .grid {{ display: grid }} .flex {{ display: flex }} .table {{ border-spacing: 0; width: 40pt }}\
             .item, td {{ width: 40pt; height: 40pt }}\
             .absolute {{ position: absolute; inset: 0; width: 40pt; height: 40pt; background: rgb(0 128 0) }}\
             </style><div class=\"outer\">{normal}<div class=\"absolute\"></div></div>"
        ))
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let page = &document.pages[0];
        let first_rect_operation = |color| {
            page.paint_operations().iter().position(|operation| {
                matches!(
                    operation,
                    crate::document::paint::page::PaintOperation::Rect(index)
                        if page.rects().get(*index).is_some_and(|rect| rect.fill == Some(color))
                )
            })
        };
        let outline = first_rect_operation(CssColor::new(255, 0, 0)).unwrap_or_else(|| {
            panic!(
                "{name} outline did not emit a red rectangle: {:?}",
                page.rects()
            )
        });
        let positioned = first_rect_operation(CssColor::new(0, 128, 0)).unwrap_or_else(|| {
            panic!(
                "{name} positioned box did not emit a green rectangle: {:?}",
                page.rects()
            )
        });
        assert!(
            outline < positioned,
            "{name} normal-flow outline must precede auto positioned paint: {:?}",
            page.paint_operations()
        );
    }
}

#[tokio::test]
async fn inline_block_outline_paints_after_atomic_content() {
    let document = Html::from_string(
        "<style>@page { size: 120pt 120pt; margin: 10pt } body, p { margin: 0; font-size: 10pt; line-height: 20pt }\
         span { display: inline-block; width: 30pt; height: 30pt; outline: 2pt solid red }\
         b { display: block; width: 15pt; height: 15pt; background: blue }</style>\
         <p><span><b></b></span></p>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let child_operation = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));
    let outline_operation = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));

    assert!(
        outline_operation > child_operation,
        "inline-block outline should paint after atomic descendant content"
    );
}

#[tokio::test]
async fn zero_font_separators_still_create_atomic_inline_break_opportunities() {
    let document = Html::from_string(
        "<!DOCTYPE html><style>@page { size: 200px 260px; margin: 0 } body { margin: 0 }\
         div { width: 100px; background: blue }\
         inline-block { display: inline-block; width: 80px; height: 1em; background: rgb(255 165 0) }\
         sep { font-size: 0 }</style>\
         <div>\
           <inline-block></inline-block><sep> </sep>\
           <inline-block></inline-block><sep>, </sep>\
           <inline-block></inline-block><sep>) (</sep>\
           <inline-block></inline-block><sep>a</sep>\
           <inline-block></inline-block>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let mut orange_rects = document.pages[0]
        .rects()
        .iter()
        .filter(|rect| rect.fill == Some(CssColor::new(255, 165, 0)))
        .collect::<Vec<_>>();
    orange_rects.sort_by(|left, right| right.y().total_cmp(&left.y()));

    assert_eq!(
        orange_rects.len(),
        5,
        "expected five inline-block backgrounds: {orange_rects:?}"
    );
    assert!(
        orange_rects
            .iter()
            .all(|rect| (rect.width() - 60.0).abs() < 0.01),
        "each 80px inline-block should be 60pt wide: {orange_rects:?}"
    );
    assert!(
        orange_rects
            .windows(2)
            .all(|pair| (pair[0].y() - pair[1].y()).abs() > 0.5),
        "inline-blocks should occupy five distinct visual lines: {orange_rects:?}"
    );
    let row_advances = orange_rects
        .windows(2)
        .map(|pair| pair[0].y() - pair[1].y())
        .collect::<Vec<_>>();
    let expected_advance = row_advances[0];
    assert!(
        row_advances
            .iter()
            .all(|advance| (*advance - expected_advance).abs() < 0.25),
        "zero-font separator fragments must not inflate selected line metrics unevenly: advances={row_advances:?}, rects={orange_rects:?}"
    );
}

#[tokio::test]
async fn float_band_paints_between_in_flow_block_and_inline_content() {
    let document = Html::from_string(
        "<style>@page { size: 160pt 120pt; margin: 10pt } body { margin: 0; font-size: 0 }\
         .block { width: 20pt; height: 20pt; background: red }\
         .float { float: left; width: 20pt; height: 20pt; background: green }\
         .inline { display: inline-block; width: 20pt; height: 20pt; background: blue }</style>\
         <div class=\"block\"></div><div class=\"float\"></div><span class=\"inline\"></span>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];

    let block_operation = first_rect_paint_operation_index(page, CssColor::new(255, 0, 0));
    let float_operation = first_rect_paint_operation_index(page, CssColor::new(0, 128, 0));
    let inline_operation = first_rect_paint_operation_index(page, CssColor::new(0, 0, 255));

    assert!(block_operation < float_operation);
    assert!(float_operation < inline_operation);
}

#[tokio::test]
async fn bfc_root_separates_adjoining_float_replay_when_it_cannot_fit() {
    let document = Html::from_string(
        "<!DOCTYPE html>\
         <style>@page { size: 400px 500px; margin: 0 } body { margin: 0 }</style>\
         <div style=\"overflow:hidden; width:200px; background:red\">\
           <div>\
             <div>\
               <div style=\"float:left; width:200px; height:200px; background:green\"></div>\
             </div>\
             <div style=\"margin-top:200px; overflow:hidden; width:200px; height:1px; background:white\"></div>\
           </div>\
         </div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();
    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .expect("green float should paint");

    assert!((green.width() - 150.0).abs() < 0.01, "{green:?}");
    assert!((green.height() - 150.0).abs() < 0.01, "{green:?}");
    assert_eq!(
        final_rect_fill_at(page, green.x() + 75.0, green.y() + 75.0),
        Some(CssColor::new(0, 128, 0))
    );
    assert_eq!(
        final_rect_fill_at(page, 75.0, 300.0),
        Some(CssColor::new(0, 128, 0)),
        "the float must not be replayed below the adjoining BFC margin"
    );
}

fn colored_rect_width(document: &spindrift::Document, color: CssColor) -> f32 {
    document.pages[0]
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(color))
        .unwrap_or_else(|| {
            panic!(
                "expected rect with color {color:?}: {:?}",
                document.pages[0].rects()
            )
        })
        .width()
}

#[tokio::test]
async fn inline_block_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         div { height: 18pt } span { display: inline-block }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div><span class=\"min\">aa bb</span></div><div><span class=\"fit\">aa bb</span></div><div><span class=\"max\">aa bb</span></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, CssColor::new(0, 128, 0));
    let fit = colored_rect_width(&document, CssColor::new(0, 0, 255));
    let max = colored_rect_width(&document, CssColor::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "inline-block intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn abspos_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 140pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         .box { position: absolute; left: 0; height: 12px }\
         .min { top: 0; width: min-content; background: green }\
         .fit { top: 20px; width: fit-content(14px); background: blue }\
         .max { top: 40px; width: max-content; background: black }</style>\
         <div class=\"box min\">aa bb</div><div class=\"box fit\">aa bb</div><div class=\"box max\">aa bb</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, CssColor::new(0, 128, 0));
    let fit = colored_rect_width(&document, CssColor::new(0, 0, 255));
    let max = colored_rect_width(&document, CssColor::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "abspos intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn float_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         .box { float: left; clear: left; height: 12px }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div class=\"box min\">aa bb</div><div class=\"box fit\">aa bb</div><div class=\"box max\">aa bb</div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, CssColor::new(0, 128, 0));
    let fit = colored_rect_width(&document, CssColor::new(0, 0, 255));
    let max = colored_rect_width(&document, CssColor::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "float intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn inline_table_width_intrinsic_keywords_use_min_fit_and_max_content() {
    let document = Html::from_string(
        "<style>@page { size: 240pt 160pt; margin: 0 } body { margin: 0; font: 10px/12px sans-serif }\
         div { height: 18pt } table { display: inline-table; border-spacing: 0 } td { padding: 0 }\
         .min { width: min-content; background: green }\
         .fit { width: fit-content(14px); background: blue }\
         .max { width: max-content; background: black }</style>\
         <div><table class=\"min\"><tr><td>aa bb</td></tr></table></div>\
         <div><table class=\"fit\"><tr><td>aa bb</td></tr></table></div>\
         <div><table class=\"max\"><tr><td>aa bb</td></tr></table></div>",
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let min = colored_rect_width(&document, CssColor::new(0, 128, 0));
    let fit = colored_rect_width(&document, CssColor::new(0, 0, 255));
    let max = colored_rect_width(&document, CssColor::new(0, 0, 0));
    assert!(
        min < fit && fit < max,
        "inline-table intrinsic widths should order min < fit < max: min={min}, fit={fit}, max={max}"
    );
}

#[tokio::test]
async fn column_flex_item_aspect_ratio_content_box_padding_sizes_border_box() {
    let document = Html::from_string(
        r#"<!DOCTYPE html>
<style>
@page { size: 360pt 360pt; margin: 0 }
body { margin: 0 }
.container { display: flex; flex-direction: column }
.box {
    width: 200px;
    aspect-ratio: 1;
    padding: 100px;
    overflow: hidden;
    background: green;
}
</style>
<div class="container"><div class="box"></div></div>"#,
    )
    .render(&RenderOptions::default())
    .await
    .unwrap();

    let page = &document.pages[0];
    let green = page
        .rects()
        .iter()
        .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
        .unwrap_or_else(|| panic!("expected green flex item background: {:?}", page.rects()));

    assert!(
        (green.width() - 300.0).abs() < 0.01 && (green.height() - 300.0).abs() < 0.01,
        "200px content plus 100px padding on each side should paint a 300pt square: {green:?}"
    );
}
