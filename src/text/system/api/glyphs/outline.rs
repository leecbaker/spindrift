use super::super::*;
use crate::document::PaintStrokeWidth;
use crate::document::paint::geometry::{PaintPoint, PaintSpace, PaintTransform};
use crate::document::paint::paths::{RenderedPath, RenderedPathCommand, RenderedPathFillRule};

///
/// A glyph outline is not page-local paint geometry. Its conversion to
/// [`PaintPoint`] happens through [`GlyphOutlineToPaint`] exactly once at the
/// font-outline paint boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GlyphOutlineSpace {}

pub(super) type GlyphOutlinePoint = euclid::Point2D<f32, GlyphOutlineSpace>;
pub(super) type GlyphOutlineToPaint = euclid::ScaleOffset2D<f32, GlyphOutlineSpace, PaintSpace>;

impl FontSystem {
    pub(crate) fn full_em_rect_glyph_coverage_paths(
        &self,
        origin: PaintPoint,
        runs: &[RenderedTextRun],
        color: CssColor,
    ) -> Vec<RenderedPath> {
        if !color.is_opaque() {
            return Vec::new();
        }

        let mut paths = Vec::new();
        for run in runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };
            let units_per_em = font.units_per_em.max(1) as f32;
            let scale = run.font_size / units_per_em;
            let [a, b, c, d] = run.text_matrix.pdf_components();
            // Mirror PDF text emission exactly: `x_offset`/`y_offset` select
            // the run text matrix origin, while each glyph starts from the
            // run-local pen.  Folding the run offset into the pen would put
            // a rotated outline on the wrong physical axis.
            let transform =
                PaintTransform::new(a, b, c, d, origin.x + run.x_offset, origin.y + run.y_offset);
            let mut cursor = 0.0;
            for glyph in glyphs.iter() {
                let Some(glyph_id) = glyph.painted_id().map(ttf_parser::GlyphId) else {
                    cursor += glyph.x_advance;
                    continue;
                };
                if !full_em_rectangle_outline(&face, glyph_id, units_per_em) {
                    cursor += glyph.x_advance;
                    continue;
                }
                let mut builder = GlyphPathBuilder::new(GlyphOutlineToPaint::new(
                    scale,
                    scale,
                    cursor + glyph.x_offset,
                    run.y_offset + glyph.y_offset,
                ));
                if face.outline_glyph(glyph_id, &mut builder).is_some()
                    && !builder.commands.is_empty()
                {
                    let path = RenderedPath::new(
                        builder.commands,
                        Some(color),
                        RenderedPathFillRule::NonZero,
                        None,
                        PaintStrokeWidth::ZERO,
                        None,
                    )
                    .with_transform(transform);
                    // `full_em_rectangle_outline` established that this
                    // transformed path covers its bounding rectangle without
                    // holes, curves, or transparency.
                    let coverage = path
                        .bounds()
                        .expect("full-em rectangle outline has finite bounds");
                    paths.push(path.with_opaque_coverage_rect(coverage));
                }
                cursor += glyph.x_advance;
            }
        }
        paths
    }
}

pub(super) fn append_colr_outline(
    face: &ttf_parser::Face<'_>,
    glyph_id: ttf_parser::GlyphId,
    outline_to_paint: GlyphOutlineToPaint,
    color: CssColor,
    paths: &mut Vec<RenderedPath>,
) {
    let mut builder = GlyphPathBuilder::new(outline_to_paint);
    if face.outline_glyph(glyph_id, &mut builder).is_some() && !builder.commands.is_empty() {
        paths.push(RenderedPath::new(
            builder.commands,
            Some(color),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        ));
    }
}

/// Return whether `glyph_id` is an unadorned rectangle spanning one em in
/// both dimensions and one em of horizontal advance.
///
/// The check deliberately rejects curves, multiple contours, holes, and
/// partial-em rectangles.  Its only purpose is to recognize glyphs for which
/// an additional vector fill is semantically identical to normal text ink.
fn full_em_rectangle_outline(
    face: &ttf_parser::Face<'_>,
    glyph_id: ttf_parser::GlyphId,
    units_per_em: f32,
) -> bool {
    let Some(bounds) = face.glyph_bounding_box(glyph_id) else {
        return false;
    };
    let Some(advance) = face.glyph_hor_advance(glyph_id) else {
        return false;
    };
    if bounds.x_min != 0
        || bounds.x_max as f32 != units_per_em
        || (bounds.y_max - bounds.y_min) as f32 != units_per_em
        || advance as f32 != units_per_em
    {
        return false;
    }

    let mut probe = FullEmRectangleOutlineProbe::default();
    if face.outline_glyph(glyph_id, &mut probe).is_none() {
        return false;
    }
    probe.is_rectangle(bounds)
}

#[derive(Default)]
struct FullEmRectangleOutlineProbe {
    points: Vec<(i16, i16)>,
    close_count: usize,
    invalid: bool,
}

impl FullEmRectangleOutlineProbe {
    fn coordinate(value: f32) -> Option<i16> {
        (value.is_finite() && value.fract() == 0.0)
            .then_some(value as i16)
            .filter(|coordinate| *coordinate as f32 == value)
    }

    fn push_point(&mut self, x: f32, y: f32) {
        let Some(x) = Self::coordinate(x) else {
            self.invalid = true;
            return;
        };
        let Some(y) = Self::coordinate(y) else {
            self.invalid = true;
            return;
        };
        self.points.push((x, y));
    }

    fn is_rectangle(&self, bounds: ttf_parser::Rect) -> bool {
        if self.invalid || self.close_count != 1 {
            return false;
        }
        let points = match self.points.as_slice() {
            [a, b, c, d] => [*a, *b, *c, *d],
            [a, b, c, d, e] if a == e => [*a, *b, *c, *d],
            _ => return false,
        };
        let corners = [
            (bounds.x_min, bounds.y_min),
            (bounds.x_min, bounds.y_max),
            (bounds.x_max, bounds.y_min),
            (bounds.x_max, bounds.y_max),
        ];
        if !corners.iter().all(|corner| points.contains(corner)) {
            return false;
        }
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(4)
            .all(|((x0, y0), (x1, y1))| (x0 == x1) != (y0 == y1))
    }
}

impl ttf_parser::OutlineBuilder for FullEmRectangleOutlineProbe {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.points.is_empty() {
            self.invalid = true;
        }
        self.push_point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_point(x, y);
    }

    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }

    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }

    fn close(&mut self) {
        self.close_count += 1;
    }
}

struct GlyphPathBuilder {
    outline_to_paint: GlyphOutlineToPaint,
    current: Option<GlyphOutlinePoint>,
    commands: Vec<RenderedPathCommand>,
}

impl GlyphPathBuilder {
    fn new(outline_to_paint: GlyphOutlineToPaint) -> Self {
        Self {
            outline_to_paint,
            current: None,
            commands: Vec::new(),
        }
    }
}

impl ttf_parser::OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::move_to(
            self.outline_to_paint.transform_point(point),
        ));
        self.current = Some(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::line_to(
            self.outline_to_paint.transform_point(point),
        ));
        self.current = Some(point);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(start) = self.current else {
            return;
        };
        let control = GlyphOutlinePoint::new(x1, y1);
        let end = GlyphOutlinePoint::new(x, y);
        let control_1 = start + (control - start) * (2.0 / 3.0);
        let control_2 = end + (control - end) * (2.0 / 3.0);
        self.commands.push(RenderedPathCommand::curve_to(
            self.outline_to_paint.transform_point(control_1),
            self.outline_to_paint.transform_point(control_2),
            self.outline_to_paint.transform_point(end),
        ));
        self.current = Some(end);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let control_1 = GlyphOutlinePoint::new(x1, y1);
        let control_2 = GlyphOutlinePoint::new(x2, y2);
        let end = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::curve_to(
            self.outline_to_paint.transform_point(control_1),
            self.outline_to_paint.transform_point(control_2),
            self.outline_to_paint.transform_point(end),
        ));
        self.current = Some(end);
    }

    fn close(&mut self) {
        self.commands.push(RenderedPathCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyph_outline_transform_preserves_source_to_paint_mapping() {
        let mut builder = GlyphPathBuilder::new(GlyphOutlineToPaint::new(2.0, 3.0, 10.0, 20.0));

        ttf_parser::OutlineBuilder::move_to(&mut builder, 1.0, 2.0);
        ttf_parser::OutlineBuilder::quad_to(&mut builder, 4.0, 5.0, 7.0, 8.0);

        assert_eq!(
            builder.commands,
            vec![
                RenderedPathCommand::move_to(PaintPoint::new(12.0, 26.0)),
                RenderedPathCommand::curve_to(
                    PaintPoint::new(16.0, 32.0),
                    PaintPoint::new(20.0, 38.0),
                    PaintPoint::new(24.0, 44.0),
                ),
            ]
        );
    }
}
