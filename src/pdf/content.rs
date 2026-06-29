use super::*;
use crate::document::RenderedPathCommandPoints;
use pdf_writer::{Content, Name, Str};

pub(super) fn page_content_render(
    page: &crate::Page,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    next_object_id: &mut usize,
) -> PageContentRender {
    let mut content = Content::new();
    let mut forms = Vec::new();
    if let Some(tree) = page.paint_tree() {
        let mut state = PaintTreeRenderState {
            next_object_id,
            forms: &mut forms,
            page_width: page.width(),
            page_height: page.height(),
        };
        write_paint_tree(
            &mut content,
            page,
            tree,
            shaped_lines,
            embedded_fonts,
            &mut state,
        );
    } else {
        let operations = page.paint_operations();
        write_page_operations(
            &mut content,
            page,
            &operations,
            shaped_lines,
            embedded_fonts,
        );
    }
    PageContentRender {
        stream: content.finish().into_vec(),
        form_xobjects: forms,
    }
}

fn write_paint_tree(
    content: &mut Content,
    page: &crate::Page,
    tree: &crate::document::PagePaintTree,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    write_stacking_context(
        content,
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
    content: &mut Content,
    page: &crate::Page,
    context: &crate::document::PaintStackingContext,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    if context.effects.needs_group() {
        write_effect_group(content, page, context, shaped_lines, embedded_fonts, state);
        return;
    }
    let effect_steps = context.effects.ordered_steps();
    let scoped = !effect_steps.is_empty();
    if scoped {
        content.save_state();
    }
    for step in effect_steps {
        match step {
            crate::document::PaintEffectStep::Clip(clip) => write_rect_clip(content, clip),
            crate::document::PaintEffectStep::Transform(transform) => {
                content.transform([
                    transform.a,
                    transform.b,
                    transform.c,
                    transform.d,
                    transform.e,
                    transform.f,
                ]);
            }
            crate::document::PaintEffectStep::ClipPath(_)
            | crate::document::PaintEffectStep::Filter(_)
            | crate::document::PaintEffectStep::Mask(_)
            | crate::document::PaintEffectStep::Opacity(_)
            | crate::document::PaintEffectStep::Blend(_)
            | crate::document::PaintEffectStep::Isolation => {}
        }
    }
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            write_display_item(content, page, item, shaped_lines, embedded_fonts, state);
        }
    }
    if scoped {
        content.restore_state();
    }
}

fn write_effect_group(
    content: &mut Content,
    page: &crate::Page,
    context: &crate::document::PaintStackingContext,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let id = *state.next_object_id;
    *state.next_object_id += 1;
    let name = format!("Fm{}", state.forms.len() + 1);
    let bbox = context.effect_bounds(crate::document::PaintClip::new(
        0.0,
        0.0,
        state.page_width,
        state.page_height,
    ));
    let mut form_content = Content::new();
    let mut form_context = context.clone();
    form_context.effects = form_context.effects.without_group_effects();
    write_stacking_context(
        &mut form_content,
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
        stream: form_content.finish().into_vec(),
    });
    content.save_state();
    if context.effects.opacity < 1.0 {
        let alpha = crate::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: context.effects.opacity,
        };
        if let Some(resource_name) = paint_alpha_resource_name(alpha) {
            content.set_parameters(pdf_name(&resource_name));
        }
    }
    if let Some(resource_name) = context.effects.blend_mode.resource_name() {
        content.set_parameters(pdf_name(&resource_name));
    }
    content.x_object(pdf_name(&name));
    content.restore_state();
}

fn write_display_item(
    content: &mut Content,
    page: &crate::Page,
    item: &crate::document::PaintDisplayItem,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match item {
        crate::document::PaintDisplayItem::Operation(operation) => {
            write_page_operation(content, page, operation, shaped_lines, embedded_fonts);
        }
        crate::document::PaintDisplayItem::StackingContext(context) => {
            write_stacking_context(content, page, context, shaped_lines, embedded_fonts, state);
        }
        crate::document::PaintDisplayItem::Primitive(_)
        | crate::document::PaintDisplayItem::Link(_) => {}
    }
}

fn write_page_operation(
    content: &mut Content,
    page: &crate::Page,
    operation: &crate::PaintOperation,
    shaped_lines: &[Option<ShapedLine>],
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    match operation {
        crate::PaintOperation::Rect(index) => {
            if let Some(rect) = page.rects.get(*index) {
                write_rect(content, rect);
            }
        }
        crate::PaintOperation::RoundedRect(index) => {
            if let Some(rect) = page.rounded_rects.get(*index) {
                write_rounded_rect(content, rect);
            }
        }
        crate::PaintOperation::Path(index) => {
            if let Some(path) = page.paths.get(*index) {
                write_path(content, path);
            }
        }
        crate::PaintOperation::Stroke(index) => {
            if let Some(stroke) = page.strokes.get(*index) {
                write_stroke(content, stroke);
            }
        }
        crate::PaintOperation::Image(index) => {
            if let Some(image) = page.images.get(*index) {
                write_image(content, image, *index);
            }
        }
        crate::PaintOperation::Line(index) => {
            if let Some(line) = page.lines.get(*index) {
                write_line(
                    content,
                    line,
                    shaped_lines.get(*index).and_then(Option::as_ref),
                    embedded_fonts,
                );
            }
        }
    }
}

fn write_rect_clip(content: &mut Content, clip: crate::document::PaintClip) {
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let rect = crate::document::paint_rect_to_pdf(clip.paint_rect());
    content
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .clip_nonzero()
        .end_path();
}

pub(super) fn write_page_operations(
    content: &mut Content,
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
                    flush_pending_rect(content, &mut pending_rect);
                    if is_mergeable_fill_rect(rect) {
                        pending_rect = Some(rect.clone());
                    } else {
                        write_rect(content, rect);
                    }
                }
            }
            crate::PaintOperation::RoundedRect(index) => {
                flush_pending_rect(content, &mut pending_rect);
                if let Some(rect) = page.rounded_rects.get(*index) {
                    write_rounded_rect(content, rect);
                }
            }
            crate::PaintOperation::Path(index) => {
                flush_pending_rect(content, &mut pending_rect);
                if let Some(path) = page.paths.get(*index) {
                    write_path(content, path);
                }
            }
            crate::PaintOperation::Stroke(index) => {
                flush_pending_rect(content, &mut pending_rect);
                if let Some(stroke) = page.strokes.get(*index) {
                    write_stroke(content, stroke);
                }
            }
            crate::PaintOperation::Image(index) => {
                flush_pending_rect(content, &mut pending_rect);
                if let Some(image) = page.images.get(*index) {
                    write_image(content, image, *index);
                }
            }
            crate::PaintOperation::Line(index) => {
                flush_pending_rect(content, &mut pending_rect);
                if let Some(line) = page.lines.get(*index) {
                    write_line(
                        content,
                        line,
                        shaped_lines.get(*index).and_then(Option::as_ref),
                        embedded_fonts,
                    );
                }
            }
        }
    }
    flush_pending_rect(content, &mut pending_rect);
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
    if nearly_equal(left.x(), right.x()) && nearly_equal(left.width(), right.width()) {
        if nearly_equal(left.y() + left.height(), right.y()) {
            left.set_paint_rect(crate::document::PaintRect::new(
                crate::document::PaintPoint::new(left.x(), left.y()),
                crate::document::PaintSize::new(left.width(), left.height() + right.height()),
            ));
            return true;
        }
        if nearly_equal(right.y() + right.height(), left.y()) {
            left.set_paint_rect(crate::document::PaintRect::new(
                crate::document::PaintPoint::new(left.x(), right.y()),
                crate::document::PaintSize::new(left.width(), left.height() + right.height()),
            ));
            return true;
        }
    }
    if nearly_equal(left.y(), right.y()) && nearly_equal(left.height(), right.height()) {
        if nearly_equal(left.x() + left.width(), right.x()) {
            left.set_paint_rect(crate::document::PaintRect::new(
                crate::document::PaintPoint::new(left.x(), left.y()),
                crate::document::PaintSize::new(left.width() + right.width(), left.height()),
            ));
            return true;
        }
        if nearly_equal(right.x() + right.width(), left.x()) {
            left.set_paint_rect(crate::document::PaintRect::new(
                crate::document::PaintPoint::new(right.x(), left.y()),
                crate::document::PaintSize::new(left.width() + right.width(), left.height()),
            ));
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

fn flush_pending_rect(content: &mut Content, pending_rect: &mut Option<crate::RenderedRect>) {
    if let Some(rect) = pending_rect.take() {
        write_rect(content, &rect);
    }
}

fn rects_intersect(left: &crate::RenderedRect, right: &crate::RenderedRect) -> bool {
    left.x() < right.x() + right.width()
        && right.x() < left.x() + left.width()
        && left.y() < right.y() + right.height()
        && right.y() < left.y() + left.height()
}

fn rect_area_is_covered_by_rects(
    rect: &crate::RenderedRect,
    covers: &[&crate::RenderedRect],
) -> bool {
    if covers.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let right = rect.x() + rect.width();
    let top = rect.y() + rect.height();
    let mut x_edges = vec![rect.x(), right];
    let mut y_edges = vec![rect.y(), top];
    for cover in covers {
        x_edges.push(cover.x().clamp(rect.x(), right));
        x_edges.push((cover.x() + cover.width()).clamp(rect.x(), right));
        y_edges.push(cover.y().clamp(rect.y(), top));
        y_edges.push((cover.y() + cover.height()).clamp(rect.y(), top));
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
                cover.x() <= cell_left + 0.001
                    && cover.x() + cover.width() >= cell_right - 0.001
                    && cover.y() <= cell_bottom + 0.001
                    && cover.y() + cover.height() >= cell_top - 0.001
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

pub(super) fn write_rect(content: &mut Content, rect: &crate::RenderedRect) {
    let pdf_rect = crate::document::paint_rect_to_pdf(rect.paint_rect());
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        content
            .set_fill_rgb(fill.r, fill.g, fill.b)
            .rect(
                pdf_rect.origin.x,
                pdf_rect.origin.y,
                pdf_rect.size.width,
                pdf_rect.size.height,
            )
            .fill_nonzero();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content
            .set_line_width(rect.stroke_width)
            .set_stroke_rgb(stroke.r, stroke.g, stroke.b)
            .rect(
                pdf_rect.origin.x,
                pdf_rect.origin.y,
                pdf_rect.size.width,
                pdf_rect.size.height,
            )
            .stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect(content: &mut Content, rect: &RenderedRoundedRect) {
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        content.set_fill_rgb(fill.r, fill.g, fill.b);
        write_rounded_rect_path(content, rect);
        content.fill_nonzero();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content
            .set_line_width(rect.stroke_width)
            .set_stroke_rgb(stroke.r, stroke.g, stroke.b);
        write_rounded_rect_path(content, rect);
        content.stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect_path(content: &mut Content, rect: &RenderedRoundedRect) {
    // PDF paths use cubic Beziers for arcs. The kappa constant approximates a
    // quarter ellipse, matching the CSS border-radius curve shape closely
    // enough for filled/stroked page graphics.
    const KAPPA: f32 = 0.552_284_8;

    let pdf_rect = crate::document::paint_rect_to_pdf(rect.paint_rect());
    let x0 = pdf_rect.origin.x;
    let y0 = pdf_rect.origin.y;
    let x1 = pdf_rect.origin.x + pdf_rect.size.width;
    let y1 = pdf_rect.origin.y + pdf_rect.size.height;
    let tl = rect.radii.top_left;
    let tr = rect.radii.top_right;
    let br = rect.radii.bottom_right;
    let bl = rect.radii.bottom_left;

    content.move_to(x0 + bl.x(), y0);
    content.line_to(x1 - br.x(), y0);
    if br.x() > 0.0 || br.y() > 0.0 {
        content.cubic_to(
            x1 - br.x() + br.x() * KAPPA,
            y0,
            x1,
            y0 + br.y() - br.y() * KAPPA,
            x1,
            y0 + br.y(),
        );
    }
    content.line_to(x1, y1 - tr.y());
    if tr.x() > 0.0 || tr.y() > 0.0 {
        content.cubic_to(
            x1,
            y1 - tr.y() + tr.y() * KAPPA,
            x1 - tr.x() + tr.x() * KAPPA,
            y1,
            x1 - tr.x(),
            y1,
        );
    }
    content.line_to(x0 + tl.x(), y1);
    if tl.x() > 0.0 || tl.y() > 0.0 {
        content.cubic_to(
            x0 + tl.x() - tl.x() * KAPPA,
            y1,
            x0,
            y1 - tl.y() + tl.y() * KAPPA,
            x0,
            y1 - tl.y(),
        );
    }
    content.line_to(x0, y0 + bl.y());
    if bl.x() > 0.0 || bl.y() > 0.0 {
        content.cubic_to(
            x0,
            y0 + bl.y() - bl.y() * KAPPA,
            x0 + bl.x() - bl.x() * KAPPA,
            y0,
            x0 + bl.x(),
            y0,
        );
    }
    content.close_path();
}

/// Serialize a generic vector path into a PDF content stream.
///
/// PDF path construction and painting operators are defined in ISO
/// 32000-1:2008, 8.5.2 and 8.5.3. CSS border rings use `f*` when their inner
/// padding-edge subpath must cut out the content area using even-odd filling.
pub(super) fn write_path(content: &mut Content, path: &RenderedPath) {
    if path.commands.is_empty() {
        return;
    }
    let clipped = path
        .clip
        .as_ref()
        .is_some_and(|clip| !clip.commands.is_empty());
    if clipped {
        content.save_state();
        let clip = path.clip.as_ref().unwrap();
        write_clip_path(content, &clip.commands, clip.fill_rule);
        for additional_clip in &clip.additional_clips {
            write_clip_path(
                content,
                &additional_clip.commands,
                additional_clip.fill_rule,
            );
        }
    }
    if let Some(fill) = path.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        content.set_fill_rgb(fill.r, fill.g, fill.b);
        write_path_commands(content, &path.commands);
        match path.fill_rule {
            RenderedPathFillRule::NonZero => {
                content.fill_nonzero();
            }
            RenderedPathFillRule::EvenOdd => {
                content.fill_even_odd();
            }
        }
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = path.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content
            .set_line_width(path.stroke_width)
            .set_stroke_rgb(stroke.r, stroke.g, stroke.b);
        write_path_commands(content, &path.commands);
        content.stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if clipped {
        content.restore_state();
    }
}

fn write_clip_path(
    content: &mut Content,
    commands: &[RenderedPathCommand],
    fill_rule: RenderedPathFillRule,
) {
    write_path_commands(content, commands);
    match fill_rule {
        RenderedPathFillRule::NonZero => {
            content.clip_nonzero();
        }
        RenderedPathFillRule::EvenOdd => {
            content.clip_even_odd();
        }
    }
    content.end_path();
}

fn write_path_commands(content: &mut Content, commands: &[RenderedPathCommand]) {
    for command in commands {
        match command.typed_points() {
            RenderedPathCommandPoints::MoveTo(point) => {
                let point = crate::document::paint_point_to_pdf(point);
                content.move_to(point.x, point.y);
            }
            RenderedPathCommandPoints::LineTo(point) => {
                let point = crate::document::paint_point_to_pdf(point);
                content.line_to(point.x, point.y);
            }
            RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                let control_1 = crate::document::paint_point_to_pdf(control_1);
                let control_2 = crate::document::paint_point_to_pdf(control_2);
                let end = crate::document::paint_point_to_pdf(end);
                content.cubic_to(
                    control_1.x,
                    control_1.y,
                    control_2.x,
                    control_2.y,
                    end.x,
                    end.y,
                );
            }
            RenderedPathCommandPoints::Close => {
                content.close_path();
            }
        }
    }
}

pub(super) fn write_stroke(content: &mut Content, stroke: &crate::RenderedStroke) {
    if !stroke.color.is_visible() {
        return;
    }
    content.save_state();
    if let Some(resource_name) = paint_alpha_resource_name(stroke.color) {
        content.set_parameters(pdf_name(&resource_name));
    }
    if let Some((dash, gap)) = stroke.dash {
        content.set_dash_pattern([dash, gap], 0.0);
    } else {
        content.set_dash_pattern([], 0.0);
    }
    let (start, end) = stroke.paint_points();
    let start = crate::document::paint_point_to_pdf(start);
    let end = crate::document::paint_point_to_pdf(end);
    content
        .set_line_width(stroke.width)
        .set_stroke_rgb(stroke.color.r, stroke.color.g, stroke.color.b)
        .move_to(start.x, start.y)
        .line_to(end.x, end.y)
        .stroke()
        .restore_state();
}

pub(super) fn write_image(content: &mut Content, image: &crate::RenderedImage, index: usize) {
    let rect = crate::document::paint_rect_to_pdf(image.paint_rect());
    content
        .save_state()
        .transform([
            rect.size.width,
            0.0,
            0.0,
            rect.size.height,
            rect.origin.x,
            rect.origin.y,
        ])
        .x_object(pdf_name(&format!("Im{}", index + 1)))
        .restore_state();
}

pub(super) fn write_line(
    content: &mut Content,
    line: &crate::RenderedLine,
    shaped_line: Option<&ShapedLine>,
    embedded_fonts: &EmbeddedFontPlans<'_>,
) {
    if let Some(shaped_line) = shaped_line {
        write_shaped_line(content, line, shaped_line, embedded_fonts);
    } else if !line.text.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
    }
}

pub(super) fn write_shaped_line(
    content: &mut Content,
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
    let scoped_alpha = write_alpha_graphics_state(content, line.color);
    content
        .set_fill_rgb(line.color.r, line.color.g, line.color.b)
        .begin_text();
    let line_origin = crate::document::paint_point_to_pdf(line.origin());
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
        let pdf_font_size = quantized_pdf_font_size(run.font_size);
        let matrix = run.text_matrix;
        content
            .set_text_matrix([
                matrix.a,
                matrix.b,
                matrix.c,
                matrix.d,
                line_origin.x + run.x_offset,
                line_origin.y + run.y_offset,
            ])
            .set_font(pdf_name(&font.resource_name), pdf_font_size);
        write_shaped_glyphs(content, run.font_size, &run.glyphs);
    }
    content.end_text();
    close_alpha_graphics_state(content, scoped_alpha);
}

/// Activate a PDF ExtGState for semi-transparent paint.
///
/// PDF 1.4 uses the `gs` operator to load graphics-state parameters,
/// including stroking and nonstroking alpha constants:
/// ISO 32000-1:2008, 8.4.4 "Graphics State Operators" and 11.7.4.3
/// "Constant Shape and Opacity".
fn write_alpha_graphics_state(content: &mut Content, color: Color) -> bool {
    if let Some(resource_name) = paint_alpha_resource_name(color) {
        content
            .save_state()
            .set_parameters(pdf_name(&resource_name));
        true
    } else {
        false
    }
}

fn close_alpha_graphics_state(content: &mut Content, scoped_alpha: bool) {
    if scoped_alpha {
        content.restore_state();
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

fn write_shaped_glyphs(content: &mut Content, font_size: f32, glyphs: &[ShapedGlyph]) {
    if !needs_positioned_glyphs(glyphs) {
        let glyph_bytes = glyph_bytes(glyphs);
        content.show(Str(&glyph_bytes));
        return;
    }

    let mut positioned = content.show_positioned();
    let mut items = positioned.items();
    for (index, glyph) in glyphs.iter().enumerate() {
        let glyph_bytes = glyph_id_bytes(glyph.id);
        items.show(Str(&glyph_bytes));
        if index + 1 < glyphs.len() {
            let adjustment =
                ((glyph.nominal_x_advance - glyph.x_advance) * 1000.0) / font_size.max(0.001);
            if adjustment.abs() > 0.01 {
                items.adjust(adjustment);
            }
        }
    }
}

pub(super) fn needs_positioned_glyphs(glyphs: &[ShapedGlyph]) -> bool {
    glyphs.iter().any(|glyph| {
        (glyph.x_advance - glyph.nominal_x_advance).abs() > 0.01
            || glyph.x_offset.abs() > 0.01
            || glyph.y_offset.abs() > 0.01
    })
}

fn glyph_bytes(glyphs: &[ShapedGlyph]) -> Vec<u8> {
    glyphs
        .iter()
        .flat_map(|glyph| glyph_id_bytes(glyph.id))
        .collect()
}

fn glyph_id_bytes(glyph_id: u16) -> [u8; 2] {
    glyph_id.to_be_bytes()
}

fn pdf_name(name: &str) -> Name<'_> {
    Name(name.as_bytes())
}
