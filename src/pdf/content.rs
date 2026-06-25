use super::*;

pub(super) fn page_content_render(
    page: &crate::Page,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    next_object_id: &mut usize,
) -> PageContentRender {
    let mut stream = Vec::new();
    let mut forms = Vec::new();
    if let Some(tree) = page.paint_tree() {
        let mut state = PaintTreeRenderState {
            next_object_id,
            forms: &mut forms,
            page_width: page.width,
            page_height: page.height,
        };
        write_paint_tree(
            &mut stream,
            page,
            tree,
            shaped_lines,
            embedded_fonts,
            &mut state,
        );
    } else {
        let operations = page.paint_operations();
        write_page_operations(&mut stream, page, &operations, shaped_lines, embedded_fonts);
    }
    PageContentRender {
        stream,
        form_xobjects: forms,
    }
}

fn write_paint_tree(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    tree: &crate::document::PagePaintTree,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    write_stacking_context(
        stream,
        page,
        &tree.root,
        shaped_lines,
        embedded_fonts,
        state,
    );
}

struct PaintTreeRenderState<'a, 'b> {
    next_object_id: &'a mut usize,
    forms: &'b mut Vec<FormXObjectRender>,
    page_width: f32,
    page_height: f32,
}

fn write_stacking_context(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    context: &crate::document::PaintStackingContext,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    if context.effects.opacity < 1.0 {
        write_opacity_group(stream, page, context, shaped_lines, embedded_fonts, state);
        return;
    }
    let scoped = context.effects.transform.is_some()
        || context.effects.overflow_clip.is_some()
        || context.effects.absolute_clip.is_some();
    if scoped {
        stream.extend_from_slice(b"q ");
    }
    if let Some(clip) = context
        .effects
        .absolute_clip
        .or(context.effects.overflow_clip)
    {
        write_rect_clip(stream, clip);
    }
    if let Some(transform) = context.effects.transform {
        stream.extend_from_slice(
            format!(
                "{:.6} {:.6} {:.6} {:.6} {:.6} {:.6} cm ",
                transform.a, transform.b, transform.c, transform.d, transform.e, transform.f
            )
            .as_bytes(),
        );
    }
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            write_display_item(stream, page, item, shaped_lines, embedded_fonts, state);
        }
    }
    if scoped {
        stream.extend_from_slice(b"Q\n");
    }
}

fn write_opacity_group(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    context: &crate::document::PaintStackingContext,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let id = *state.next_object_id;
    *state.next_object_id += 1;
    let name = format!("Fm{}", state.forms.len() + 1);
    let bbox = context.bounds.unwrap_or(crate::document::PaintClip {
        x: 0.0,
        y: 0.0,
        width: state.page_width,
        height: state.page_height,
    });
    let mut form_stream = Vec::new();
    let mut form_context = context.clone();
    form_context.effects.opacity = 1.0;
    write_stacking_context(
        &mut form_stream,
        page,
        &form_context,
        shaped_lines,
        embedded_fonts,
        state,
    );
    state.forms.push(FormXObjectRender {
        id,
        name: name.clone(),
        bbox,
        stream: form_stream,
    });
    let alpha = crate::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: context.effects.opacity,
    };
    stream.extend_from_slice(b"q ");
    if let Some(resource_name) = paint_alpha_resource_name(alpha) {
        stream.extend_from_slice(format!("/{resource_name} gs ").as_bytes());
    }
    stream.extend_from_slice(format!("/{name} Do Q\n").as_bytes());
}

fn write_display_item(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    item: &crate::document::PaintDisplayItem,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match item {
        crate::document::PaintDisplayItem::Operation(operation) => {
            write_page_operation(stream, page, operation, shaped_lines, embedded_fonts);
        }
        crate::document::PaintDisplayItem::StackingContext(context) => {
            write_stacking_context(stream, page, context, shaped_lines, embedded_fonts, state);
        }
        crate::document::PaintDisplayItem::Primitive(_)
        | crate::document::PaintDisplayItem::Link(_) => {}
    }
}

fn write_page_operation(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    operation: &crate::PaintOperation,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    match operation {
        crate::PaintOperation::Rect(index) => {
            if let Some(rect) = page.rects.get(*index) {
                write_rect(stream, rect);
            }
        }
        crate::PaintOperation::RoundedRect(index) => {
            if let Some(rect) = page.rounded_rects.get(*index) {
                write_rounded_rect(stream, rect);
            }
        }
        crate::PaintOperation::Path(index) => {
            if let Some(path) = page.paths.get(*index) {
                write_path(stream, path);
            }
        }
        crate::PaintOperation::Stroke(index) => {
            if let Some(stroke) = page.strokes.get(*index) {
                write_stroke(stream, stroke);
            }
        }
        crate::PaintOperation::Image(index) => {
            if let Some(image) = page.images.get(*index) {
                write_image(stream, image, *index);
            }
        }
        crate::PaintOperation::Line(index) => {
            if let Some(line) = page.lines.get(*index) {
                write_line(
                    stream,
                    line,
                    shaped_lines.get(*index).and_then(Option::as_ref),
                    embedded_fonts,
                );
            }
        }
    }
}

fn write_rect_clip(stream: &mut Vec<u8>, clip: crate::document::PaintClip) {
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return;
    }
    stream.extend_from_slice(
        format!(
            "{:.6} {:.6} {:.6} {:.6} re W n ",
            clip.x, clip.y, clip.width, clip.height
        )
        .as_bytes(),
    );
}

pub(super) fn write_page_operations(
    stream: &mut Vec<u8>,
    page: &crate::Page,
    operations: &[crate::PaintOperation],
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    let mut pending_rect = None;
    for (operation_index, operation) in operations.iter().enumerate() {
        match operation {
            crate::PaintOperation::Rect(index) => {
                if let Some(rect) = page.rects.get(*index) {
                    if fill_rect_is_covered_by_later_opaque_rects(
                        page,
                        operations,
                        operation_index,
                        rect,
                    ) {
                        continue;
                    }
                    if let Some(pending) = pending_rect.as_mut()
                        && merge_adjacent_fill_rect(pending, rect)
                    {
                        continue;
                    }
                    flush_pending_rect(stream, &mut pending_rect);
                    if is_mergeable_fill_rect(rect) {
                        pending_rect = Some(rect.clone());
                    } else {
                        write_rect(stream, rect);
                    }
                }
            }
            crate::PaintOperation::RoundedRect(index) => {
                flush_pending_rect(stream, &mut pending_rect);
                if let Some(rect) = page.rounded_rects.get(*index) {
                    write_rounded_rect(stream, rect);
                }
            }
            crate::PaintOperation::Path(index) => {
                flush_pending_rect(stream, &mut pending_rect);
                if let Some(path) = page.paths.get(*index) {
                    write_path(stream, path);
                }
            }
            crate::PaintOperation::Stroke(index) => {
                flush_pending_rect(stream, &mut pending_rect);
                if let Some(stroke) = page.strokes.get(*index) {
                    write_stroke(stream, stroke);
                }
            }
            crate::PaintOperation::Image(index) => {
                flush_pending_rect(stream, &mut pending_rect);
                if let Some(image) = page.images.get(*index) {
                    write_image(stream, image, *index);
                }
            }
            crate::PaintOperation::Line(index) => {
                flush_pending_rect(stream, &mut pending_rect);
                if let Some(line) = page.lines.get(*index) {
                    write_line(
                        stream,
                        line,
                        shaped_lines.get(*index).and_then(Option::as_ref),
                        embedded_fonts,
                    );
                }
            }
        }
    }
    flush_pending_rect(stream, &mut pending_rect);
}

/// Detects a PDF fill that is completely obscured by later opaque fills.
///
/// Omitting a fully covered underpaint preserves the final composited page and
/// avoids rasterizer antialiasing at later fill boundaries sampling hidden
/// colors underneath the top paint:
/// <https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf#page=122>.
fn fill_rect_is_covered_by_later_opaque_rects(
    page: &crate::Page,
    operations: &[crate::PaintOperation],
    operation_index: usize,
    rect: &crate::RenderedRect,
) -> bool {
    if !is_opaque_fill_rect(rect) {
        return false;
    }
    let later_rects = operations
        .iter()
        .skip(operation_index + 1)
        .filter_map(|operation| match operation {
            crate::PaintOperation::Rect(index) => page.rects.get(*index),
            _ => None,
        })
        .filter(|later| is_opaque_fill_rect(later) && rects_intersect(rect, later))
        .collect::<Vec<_>>();
    rect_area_is_covered_by_rects(rect, &later_rects)
}

/// Coalesces adjacent filled PDF rectangles with identical paint.
///
/// PDF 1.7 paints each path independently, and common PDF rasterizers
/// antialias the edge of each filled rectangle. Combining same-color adjacent
/// rectangles into one path preserves the vector result while preventing
/// underpaint color from leaking through shared edges:
/// <https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf#page=122>.
fn merge_adjacent_fill_rect(left: &mut crate::RenderedRect, right: &crate::RenderedRect) -> bool {
    if !is_mergeable_fill_rect(left) || !is_mergeable_fill_rect(right) || left.fill != right.fill {
        return false;
    }
    if nearly_equal(left.x, right.x) && nearly_equal(left.width, right.width) {
        if nearly_equal(left.y + left.height, right.y) {
            left.height += right.height;
            return true;
        }
        if nearly_equal(right.y + right.height, left.y) {
            left.y = right.y;
            left.height += right.height;
            return true;
        }
    }
    if nearly_equal(left.y, right.y) && nearly_equal(left.height, right.height) {
        if nearly_equal(left.x + left.width, right.x) {
            left.width += right.width;
            return true;
        }
        if nearly_equal(right.x + right.width, left.x) {
            left.x = right.x;
            left.width += right.width;
            return true;
        }
    }
    false
}

fn is_mergeable_fill_rect(rect: &crate::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.is_visible())
}

fn is_opaque_fill_rect(rect: &crate::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.a >= 1.0)
}

fn flush_pending_rect(stream: &mut Vec<u8>, pending_rect: &mut Option<crate::RenderedRect>) {
    if let Some(rect) = pending_rect.take() {
        write_rect(stream, &rect);
    }
}

fn rects_intersect(left: &crate::RenderedRect, right: &crate::RenderedRect) -> bool {
    left.x < right.x + right.width
        && right.x < left.x + left.width
        && left.y < right.y + right.height
        && right.y < left.y + left.height
}

fn rect_area_is_covered_by_rects(
    rect: &crate::RenderedRect,
    covers: &[&crate::RenderedRect],
) -> bool {
    if covers.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
        return false;
    }
    let right = rect.x + rect.width;
    let top = rect.y + rect.height;
    let mut x_edges = vec![rect.x, right];
    let mut y_edges = vec![rect.y, top];
    for cover in covers {
        x_edges.push(cover.x.clamp(rect.x, right));
        x_edges.push((cover.x + cover.width).clamp(rect.x, right));
        y_edges.push(cover.y.clamp(rect.y, top));
        y_edges.push((cover.y + cover.height).clamp(rect.y, top));
    }
    sort_unique_edges(&mut x_edges);
    sort_unique_edges(&mut y_edges);

    x_edges.windows(2).all(|x_pair| {
        y_edges.windows(2).all(|y_pair| {
            let cell_left = x_pair[0];
            let cell_right = x_pair[1];
            let cell_bottom = y_pair[0];
            let cell_top = y_pair[1];
            if cell_right <= cell_left || cell_top <= cell_bottom {
                return true;
            }
            covers.iter().any(|cover| {
                cover.x <= cell_left + 0.001
                    && cover.x + cover.width >= cell_right - 0.001
                    && cover.y <= cell_bottom + 0.001
                    && cover.y + cover.height >= cell_top - 0.001
            })
        })
    })
}

fn sort_unique_edges(edges: &mut Vec<f32>) {
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|left, right| nearly_equal(*left, *right));
}

fn nearly_equal(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.001
}

pub(super) fn write_rect(stream: &mut Vec<u8>, rect: &crate::RenderedRect) {
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, fill);
        stream.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} rg {:.6} {:.6} {:.6} {:.6} re f\n",
                fill.r, fill.g, fill.b, rect.x, rect.y, rect.width, rect.height
            )
            .as_bytes(),
        );
        close_alpha_graphics_state(stream, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, stroke);
        stream.extend_from_slice(
            format!(
                "{:.3} w {:.3} {:.3} {:.3} RG {:.6} {:.6} {:.6} {:.6} re S\n",
                rect.stroke_width,
                stroke.r,
                stroke.g,
                stroke.b,
                rect.x,
                rect.y,
                rect.width,
                rect.height
            )
            .as_bytes(),
        );
        close_alpha_graphics_state(stream, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect(stream: &mut Vec<u8>, rect: &RenderedRoundedRect) {
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, fill);
        stream
            .extend_from_slice(format!("{:.3} {:.3} {:.3} rg ", fill.r, fill.g, fill.b).as_bytes());
        write_rounded_rect_path(stream, rect);
        stream.extend_from_slice(b" f\n");
        close_alpha_graphics_state(stream, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, stroke);
        stream.extend_from_slice(
            format!(
                "{:.3} w {:.3} {:.3} {:.3} RG ",
                rect.stroke_width, stroke.r, stroke.g, stroke.b
            )
            .as_bytes(),
        );
        write_rounded_rect_path(stream, rect);
        stream.extend_from_slice(b" S\n");
        close_alpha_graphics_state(stream, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect_path(stream: &mut Vec<u8>, rect: &RenderedRoundedRect) {
    // PDF paths use cubic Beziers for arcs. The kappa constant approximates a
    // quarter ellipse, matching the CSS border-radius curve shape closely
    // enough for filled/stroked page graphics.
    const KAPPA: f32 = 0.552_284_8;

    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let tl = rect.radii.top_left;
    let tr = rect.radii.top_right;
    let br = rect.radii.bottom_right;
    let bl = rect.radii.bottom_left;

    stream.extend_from_slice(format!("{:.3} {:.3} m ", x0 + bl.x, y0).as_bytes());
    stream.extend_from_slice(format!("{:.3} {:.3} l ", x1 - br.x, y0).as_bytes());
    if br.x > 0.0 || br.y > 0.0 {
        stream.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                x1 - br.x + br.x * KAPPA,
                y0,
                x1,
                y0 + br.y - br.y * KAPPA,
                x1,
                y0 + br.y
            )
            .as_bytes(),
        );
    }
    stream.extend_from_slice(format!("{:.3} {:.3} l ", x1, y1 - tr.y).as_bytes());
    if tr.x > 0.0 || tr.y > 0.0 {
        stream.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                x1,
                y1 - tr.y + tr.y * KAPPA,
                x1 - tr.x + tr.x * KAPPA,
                y1,
                x1 - tr.x,
                y1
            )
            .as_bytes(),
        );
    }
    stream.extend_from_slice(format!("{:.3} {:.3} l ", x0 + tl.x, y1).as_bytes());
    if tl.x > 0.0 || tl.y > 0.0 {
        stream.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                x0 + tl.x - tl.x * KAPPA,
                y1,
                x0,
                y1 - tl.y + tl.y * KAPPA,
                x0,
                y1 - tl.y
            )
            .as_bytes(),
        );
    }
    stream.extend_from_slice(format!("{:.3} {:.3} l ", x0, y0 + bl.y).as_bytes());
    if bl.x > 0.0 || bl.y > 0.0 {
        stream.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                x0,
                y0 + bl.y - bl.y * KAPPA,
                x0 + bl.x - bl.x * KAPPA,
                y0,
                x0 + bl.x,
                y0
            )
            .as_bytes(),
        );
    }
    stream.extend_from_slice(b"h");
}

/// Serialize a generic vector path into a PDF content stream.
///
/// PDF path construction and painting operators are defined in ISO
/// 32000-1:2008, 8.5.2 and 8.5.3. CSS border rings use `f*` when their inner
/// padding-edge subpath must cut out the content area using even-odd filling.
pub(super) fn write_path(stream: &mut Vec<u8>, path: &RenderedPath) {
    if path.commands.is_empty() {
        return;
    }
    let clipped = path
        .clip
        .as_ref()
        .is_some_and(|clip| !clip.commands.is_empty());
    if clipped {
        stream.extend_from_slice(b"q ");
        let clip = path.clip.as_ref().unwrap();
        write_clip_path(stream, &clip.commands, clip.fill_rule);
        for additional_clip in &clip.additional_clips {
            write_clip_path(stream, &additional_clip.commands, additional_clip.fill_rule);
        }
    }
    if let Some(fill) = path.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, fill);
        stream
            .extend_from_slice(format!("{:.3} {:.3} {:.3} rg ", fill.r, fill.g, fill.b).as_bytes());
        write_path_commands(stream, &path.commands);
        match path.fill_rule {
            RenderedPathFillRule::NonZero => stream.extend_from_slice(b" f\n"),
            RenderedPathFillRule::EvenOdd => stream.extend_from_slice(b" f*\n"),
        }
        close_alpha_graphics_state(stream, scoped_alpha);
    }
    if let Some(stroke) = path.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(stream, stroke);
        stream.extend_from_slice(
            format!(
                "{:.3} w {:.3} {:.3} {:.3} RG ",
                path.stroke_width, stroke.r, stroke.g, stroke.b
            )
            .as_bytes(),
        );
        write_path_commands(stream, &path.commands);
        stream.extend_from_slice(b" S\n");
        close_alpha_graphics_state(stream, scoped_alpha);
    }
    if clipped {
        stream.extend_from_slice(b"Q\n");
    }
}

fn write_clip_path(
    stream: &mut Vec<u8>,
    commands: &[RenderedPathCommand],
    fill_rule: RenderedPathFillRule,
) {
    write_path_commands(stream, commands);
    match fill_rule {
        RenderedPathFillRule::NonZero => stream.extend_from_slice(b"W n "),
        RenderedPathFillRule::EvenOdd => stream.extend_from_slice(b"W* n "),
    }
}

fn write_path_commands(stream: &mut Vec<u8>, commands: &[RenderedPathCommand]) {
    for command in commands {
        match *command {
            RenderedPathCommand::MoveTo(x, y) => {
                stream.extend_from_slice(format!("{x:.3} {y:.3} m ").as_bytes());
            }
            RenderedPathCommand::LineTo(x, y) => {
                stream.extend_from_slice(format!("{x:.3} {y:.3} l ").as_bytes());
            }
            RenderedPathCommand::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                stream.extend_from_slice(
                    format!("{x1:.3} {y1:.3} {x2:.3} {y2:.3} {x3:.3} {y3:.3} c ").as_bytes(),
                );
            }
            RenderedPathCommand::Close => stream.extend_from_slice(b"h "),
        }
    }
}

pub(super) fn write_stroke(stream: &mut Vec<u8>, stroke: &crate::RenderedStroke) {
    if !stroke.color.is_visible() {
        return;
    }
    stream.extend_from_slice(b"q ");
    if let Some(resource_name) = paint_alpha_resource_name(stroke.color) {
        stream.extend_from_slice(format!("/{resource_name} gs ").as_bytes());
    }
    if let Some((dash, gap)) = stroke.dash {
        stream.extend_from_slice(format!("[{dash:.3} {gap:.3}] 0 d ").as_bytes());
    } else {
        stream.extend_from_slice(b"[] 0 d ");
    }
    stream.extend_from_slice(
        format!(
            "{:.3} w {:.3} {:.3} {:.3} RG {:.3} {:.3} m {:.3} {:.3} l S Q\n",
            stroke.width,
            stroke.color.r,
            stroke.color.g,
            stroke.color.b,
            stroke.x1,
            stroke.y1,
            stroke.x2,
            stroke.y2
        )
        .as_bytes(),
    );
}

pub(super) fn write_image(stream: &mut Vec<u8>, image: &crate::RenderedImage, index: usize) {
    stream.extend_from_slice(
        format!(
            "q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im{} Do Q\n",
            image.width,
            image.height,
            image.x,
            image.y,
            index + 1
        )
        .as_bytes(),
    );
}

pub(super) fn write_line(
    stream: &mut Vec<u8>,
    line: &crate::RenderedLine,
    shaped_line: Option<&ShapedLine>,
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    if let Some(shaped_line) = shaped_line {
        write_shaped_line(stream, line, shaped_line, embedded_fonts);
    } else if !line.text.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
    }
}

pub(super) fn write_shaped_line(
    stream: &mut Vec<u8>,
    line: &crate::RenderedLine,
    shaped_line: &ShapedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    if !line.color.is_visible() {
        return;
    }
    if !shaped_line.runs.iter().any(|run| !run.glyphs.is_empty()) {
        return;
    }

    // PDF 2.0 9.4.4 text matrices position each glyph stream in user space.
    // CSS inline layout stores shaped runs at visual offsets inside one line
    // box, so reset the text matrix for each run instead of assuming all
    // fallback/style runs are contiguous after the previous text operator.
    let scoped_alpha = write_alpha_graphics_state(stream, line.color);
    stream.extend_from_slice(
        format!(
            "{:.3} {:.3} {:.3} rg BT ",
            line.color.r, line.color.g, line.color.b
        )
        .as_bytes(),
    );
    for run in &shaped_line.runs {
        if run.glyphs.is_empty() {
            continue;
        }
        let Some(font) = embedded_fonts
            .document_font_to_embedded_font
            .get(run.document_font_id)
            .and_then(|index| *index)
            .and_then(|index| embedded_fonts.fonts.get(index))
        else {
            log::warn!(
                "skipping shaped text run with unmapped document font id {}",
                run.document_font_id
            );
            continue;
        };
        let text_operator = shaped_text_operator(run.font_size, &run.glyphs);
        let pdf_font_size = quantized_pdf_font_size(run.font_size);
        stream.extend_from_slice(
            format!(
                "1 0 0 1 {:.6} {:.6} Tm /{} {:.6} Tf {} ",
                line.x + run.x_offset,
                line.y,
                font.resource_name,
                pdf_font_size,
                text_operator
            )
            .as_bytes(),
        );
    }
    stream.extend_from_slice(b"ET\n");
    close_alpha_graphics_state(stream, scoped_alpha);
}

/// Activate a PDF ExtGState for semi-transparent paint.
///
/// PDF 1.4 uses the `gs` operator to load graphics-state parameters,
/// including stroking and nonstroking alpha constants:
/// ISO 32000-1:2008, 8.4.4 "Graphics State Operators" and 11.7.4.3
/// "Constant Shape and Opacity".
fn write_alpha_graphics_state(stream: &mut Vec<u8>, color: Color) -> bool {
    if let Some(resource_name) = paint_alpha_resource_name(color) {
        stream.extend_from_slice(format!("q /{resource_name} gs ").as_bytes());
        true
    } else {
        false
    }
}

fn close_alpha_graphics_state(stream: &mut Vec<u8>, scoped_alpha: bool) {
    if scoped_alpha {
        stream.extend_from_slice(b"Q\n");
    }
}

pub(super) fn quantized_pdf_font_size(font_size: f32) -> f32 {
    // WeasyPrint shapes through Pango, whose public units are fixed at
    // 1024 units per CSS pixel. CSS Values defines 1px as 0.75pt, and PDF
    // text space uses points here, so mirror that quantization at emission to
    // keep glyph rasterization aligned with WeasyPrint.
    let css_px = font_size / crate::css::CSS_PX_TO_PT;
    (css_px * 1024.0).floor() / 1024.0 * crate::css::CSS_PX_TO_PT
}

pub(super) fn shaped_text_operator(font_size: f32, glyphs: &[ShapedGlyph]) -> String {
    if !needs_positioned_glyphs(glyphs) {
        return format!("{} Tj", glyph_string(glyphs));
    }

    let mut parts = Vec::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        parts.push(format!("<{:04X}>", glyph.id));
        if index + 1 < glyphs.len() {
            let adjustment =
                ((glyph.nominal_x_advance - glyph.x_advance) * 1000.0) / font_size.max(0.001);
            if adjustment.abs() > 0.01 {
                parts.push(format!("{adjustment:.3}"));
            }
        }
    }
    format!("[{}] TJ", parts.join(" "))
}

pub(super) fn needs_positioned_glyphs(glyphs: &[ShapedGlyph]) -> bool {
    glyphs.iter().any(|glyph| {
        (glyph.x_advance - glyph.nominal_x_advance).abs() > 0.01
            || glyph.x_offset.abs() > 0.01
            || glyph.y_offset.abs() > 0.01
    })
}

pub(super) fn glyph_string(glyphs: &[ShapedGlyph]) -> String {
    format!(
        "<{}>",
        glyphs
            .iter()
            .map(|glyph| format!("{:04X}", glyph.id))
            .collect::<String>()
    )
}

pub(super) fn escape_pdf_string(text: &str) -> String {
    escape_pdf_bytes(&encode_winansi_bytes(text))
}

pub(super) fn encode_winansi_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| winansi_byte(character).unwrap_or(b'?'))
        .collect()
}

pub(super) fn winansi_byte(character: char) -> Option<u8> {
    match character {
        '\n' | '\r' | '\t' => Some(b' '),
        '\u{00a0}' => Some(0xa0),
        character if ('\u{20}'..='\u{7e}').contains(&character) => Some(character as u8),
        '€' => Some(0x80),
        '‚' => Some(0x82),
        'ƒ' => Some(0x83),
        '„' => Some(0x84),
        '…' => Some(0x85),
        '†' => Some(0x86),
        '‡' => Some(0x87),
        'ˆ' => Some(0x88),
        '‰' => Some(0x89),
        'Š' => Some(0x8a),
        '‹' => Some(0x8b),
        'Œ' => Some(0x8c),
        'Ž' => Some(0x8e),
        '‘' => Some(0x91),
        '’' => Some(0x92),
        '“' => Some(0x93),
        '”' => Some(0x94),
        '•' => Some(0x95),
        '–' => Some(0x96),
        '—' => Some(0x97),
        '˜' => Some(0x98),
        '™' => Some(0x99),
        'š' => Some(0x9a),
        '›' => Some(0x9b),
        'œ' => Some(0x9c),
        'ž' => Some(0x9e),
        'Ÿ' => Some(0x9f),
        character if ('\u{00a1}'..='\u{00ff}').contains(&character) => Some(character as u8),
        _ => None,
    }
}

pub(super) fn escape_pdf_bytes(bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in bytes {
        match byte {
            b'(' => output.push_str("\\("),
            b')' => output.push_str("\\)"),
            b'\\' => output.push_str("\\\\"),
            0x20..=0x7e => output.push(*byte as char),
            _ => output.push_str(&format!("\\{byte:03o}")),
        }
    }
    output
}
