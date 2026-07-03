use super::*;

impl RenderedStroke {
    pub fn new(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Color,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self::from_paint_points(
            PaintPoint::new(x1, y1),
            PaintPoint::new(x2, y2),
            width,
            color,
            dash,
        )
    }

    pub(crate) fn from_paint_points(
        start: PaintPoint,
        end: PaintPoint,
        width: f32,
        color: Color,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self {
            start,
            end,
            width,
            color,
            dash,
        }
    }

    pub fn x1(&self) -> f32 {
        self.start.x
    }

    pub fn y1(&self) -> f32 {
        self.start.y
    }

    pub fn x2(&self) -> f32 {
        self.end.x
    }

    pub fn y2(&self) -> f32 {
        self.end.y
    }

    pub(crate) fn paint_points(self) -> (PaintPoint, PaintPoint) {
        (self.start, self.end)
    }

    pub(crate) fn paint_bounds(self) -> PaintClip {
        let (start, end) = self.paint_points();
        let half = self.width / 2.0;
        let left = start.x.min(end.x) - half;
        let right = start.x.max(end.x) + half;
        let bottom = start.y.min(end.y) - half;
        let top = start.y.max(end.y) + half;
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(left, bottom),
            PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
        ))
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.start += offset;
        self.end += offset;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub text: String,
    pub(in crate::document) origin: PaintPoint,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub color: Color,
    pub runs: Vec<RenderedTextRun>,
    pub(crate) source: RenderedLineSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedLineSource {
    Normal,
    InlineAtom,
    Marker,
}

impl RenderedLine {
    pub fn new(
        text: String,
        x: f32,
        y: f32,
        font_size: f32,
        font_id: Option<usize>,
        color: Color,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self::from_paint_origin(text, PaintPoint::new(x, y), font_size, font_id, color, runs)
    }

    pub fn x(&self) -> f32 {
        self.origin.x
    }

    pub fn y(&self) -> f32 {
        self.origin.y
    }

    pub(crate) fn from_paint_origin(
        text: String,
        origin: PaintPoint,
        font_size: f32,
        font_id: Option<usize>,
        color: Color,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self::from_paint_origin_with_source(
            text,
            origin,
            font_size,
            font_id,
            color,
            runs,
            RenderedLineSource::Normal,
        )
    }

    pub(crate) fn from_paint_origin_with_source(
        text: String,
        origin: PaintPoint,
        font_size: f32,
        font_id: Option<usize>,
        color: Color,
        runs: Vec<RenderedTextRun>,
        source: RenderedLineSource,
    ) -> Self {
        Self {
            text,
            origin,
            font_size,
            font_id,
            color,
            runs,
            source,
        }
    }

    pub(crate) fn origin(&self) -> PaintPoint {
        self.origin
    }

    pub(crate) fn paint_bounds(&self) -> PaintClip {
        let origin = self.origin();
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(origin.x, origin.y - self.font_size),
            PaintSize::new(rendered_line_width(self), self.font_size * 1.35),
        ))
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.origin += offset;
        self
    }

    pub(crate) fn translate_origin(&mut self, offset: PaintVector) {
        self.origin += offset;
    }

    pub(in crate::document) fn append_same_line_with_gap(&mut self, other: &RenderedLine) {
        let offset = other.origin.x - self.origin.x;
        self.text.push(' ');
        self.text.push_str(&other.text);
        self.runs.extend(other.runs.iter().cloned().map(|mut run| {
            run.x_offset += offset;
            run
        }));
    }

    pub(in crate::document) fn append_same_line_continuation(&mut self, other: &RenderedLine) {
        let offset = other.origin.x - self.origin.x;
        self.text.push_str(&other.text);
        self.runs.extend(other.runs.iter().cloned().map(|mut run| {
            run.x_offset += offset;
            run
        }));
    }
}

/// Return whether adjacent rendered text groups are fragments of one inline line.
///
/// CSS Inline allows bidi reordering and whitespace processing to split a
/// visual line into multiple shaped text groups. The public rendered-line
/// metadata should still expose contiguous same-style groups as one line, while
/// preserving distinct styled fragments and deliberate word-gap merges:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::document) fn rendered_lines_can_merge_as_inline_continuation(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    let y_tolerance = left.font_size.min(right.font_size) * 0.25;
    let has_preserved_tab_text = left
        .text
        .chars()
        .chain(right.text.chars())
        .any(|character| character == '\t');
    let left_text_font_id = rendered_line_first_significant_font_id(left);
    let right_text_font_id = rendered_line_first_significant_font_id(right);
    if (left.origin.y - right.origin.y).abs() > y_tolerance
        || right.origin.x < left.origin.x
        || left.source != right.source
        || (left.font_size - right.font_size).abs() >= 0.01
        || (left_text_font_id != right_text_font_id
            && !has_preserved_tab_text
            && !rendered_lines_can_merge_across_font_fallback_boundary(left, right))
        || left.color != right.color
    {
        return false;
    }
    let has_continuation_space = left.text.chars().last().is_some_and(char::is_whitespace)
        || right.text.chars().next().is_some_and(char::is_whitespace);
    if !has_continuation_space && !has_preserved_tab_text {
        return false;
    }
    let gap = rendered_line_visual_start(right) - rendered_line_visual_end(left);
    gap.abs() <= 0.1
}

fn rendered_line_first_significant_font_id(line: &RenderedLine) -> Option<usize> {
    line.runs
        .iter()
        .find(|run| run.text.chars().any(|character| !character.is_whitespace()))
        .and_then(|run| run.font_id)
        .or(line.font_id)
}

fn rendered_lines_can_merge_across_font_fallback_boundary(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if !left.text.chars().last().is_some_and(char::is_whitespace)
        && !right.text.chars().next().is_some_and(char::is_whitespace)
    {
        return false;
    }
    rendered_line_trimmed_single_non_ascii_char(left).is_some()
        || rendered_line_trimmed_single_non_ascii_char(right).is_some()
}

fn rendered_line_trimmed_single_non_ascii_char(line: &RenderedLine) -> Option<char> {
    let mut chars = line.text.trim().chars();
    let character = chars.next()?;
    if chars.next().is_none() && !character.is_ascii() && character != '\u{2708}' {
        Some(character)
    } else {
        None
    }
}

pub(in crate::document) fn split_rendered_line_at_font_run_boundaries(
    line: RenderedLine,
) -> Vec<RenderedLine> {
    let [symbol_run, text_run] = line.runs.as_slice() else {
        return vec![line];
    };
    if !symbol_run.text_matrix.is_identity()
        || !text_run.text_matrix.is_identity()
        || !text_run
            .text
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        || text_run.text.trim_start().is_empty()
        || symbol_run.font_id == text_run.font_id
    {
        return vec![line];
    }
    let mut symbol_chars = symbol_run.text.chars();
    let Some(symbol) = symbol_chars.next() else {
        return vec![line];
    };
    if symbol_chars.next().is_some() || symbol.is_whitespace() || symbol.is_alphanumeric() {
        return vec![line];
    }

    vec![
        rendered_line_segment(
            &line,
            vec![symbol_run.clone()],
            symbol_run.font_id,
            symbol_run.x_offset,
            symbol_run.y_offset,
            false,
        ),
        rendered_line_segment(
            &line,
            vec![text_run.clone()],
            text_run.font_id,
            text_run.x_offset,
            text_run.y_offset,
            true,
        ),
    ]
}

fn rendered_line_segment(
    source: &RenderedLine,
    runs: Vec<RenderedTextRun>,
    font_id: Option<usize>,
    x_offset: f32,
    y_offset: f32,
    trim_public_leading_space: bool,
) -> RenderedLine {
    let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
    let text = if trim_public_leading_space {
        text.trim_start().to_string()
    } else {
        text
    };
    RenderedLine {
        text,
        origin: PaintPoint::new(source.origin.x + x_offset, source.origin.y + y_offset),
        font_size: source.font_size,
        font_id,
        color: source.color,
        runs: runs
            .into_iter()
            .map(|mut run| {
                run.x_offset -= x_offset;
                run.y_offset -= y_offset;
                run
            })
            .collect(),
        source: source.source,
    }
}

pub(in crate::document) fn rendered_lines_can_merge_with_word_gap(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if (left.origin.y - right.origin.y).abs() >= 0.01
        || right.origin.x < left.origin.x
        || (left.font_size - right.font_size).abs() >= 0.01
        || left.font_id != right.font_id
        || left.color != right.color
        || left.text.chars().count() <= 1
        || !right.text.chars().all(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    let gap = right.origin.x - left.origin.x - rendered_line_width(left);
    gap > 0.1 && gap <= left.font_size
}

pub(in crate::document) fn rendered_line_visual_start(line: &RenderedLine) -> f32 {
    line.origin.x
        + line
            .runs
            .iter()
            .map(|run| run.x_offset)
            .fold(0.0_f32, f32::min)
}

pub(in crate::document) fn rendered_line_visual_end(line: &RenderedLine) -> f32 {
    line.origin.x + rendered_line_width(line)
}

pub(in crate::document) fn rendered_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = if run.text_matrix.is_identity() {
            run.glyphs
                .as_ref()
                .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5)
        } else {
            run.font_size
        };
        width.max(run.x_offset + run_width)
    })
}

pub(in crate::document) fn rect_bounds(rect: PaintRect) -> Option<PaintClip> {
    (rect.size.width > 0.0 && rect.size.height > 0.0).then_some(PaintClip::from_paint_rect(rect))
}

pub(in crate::document) fn path_bounds(path: &RenderedPath) -> Option<PaintClip> {
    let mut bounds: Option<PaintClip> = None;
    for command in &path.commands {
        for point in command_points(*command) {
            let point = PaintClip::from_paint_point(point);
            bounds = Some(match bounds {
                Some(existing) => existing.union(point),
                None => point,
            });
        }
    }
    bounds.map(|bounds| {
        let outset = path.stroke_width.max(0.0) / 2.0;
        PaintClip::new(
            bounds.x() - outset,
            bounds.y() - outset,
            bounds.width() + outset * 2.0,
            bounds.height() + outset * 2.0,
        )
    })
}

pub(in crate::document) fn command_points(command: RenderedPathCommand) -> Vec<PaintPoint> {
    match command.typed_points() {
        RenderedPathCommandPoints::MoveTo(point) | RenderedPathCommandPoints::LineTo(point) => {
            vec![point]
        }
        RenderedPathCommandPoints::CurveTo {
            control_1,
            control_2,
            end,
        } => vec![control_1, control_2, end],
        RenderedPathCommandPoints::Close => Vec::new(),
    }
}

/// A shaped text run positioned relative to a [`RenderedLine`] origin.
///
/// CSS inline layout produces line boxes containing adjacent font/style runs.
/// Keeping runs explicit lets the PDF backend emit each run with its selected
/// embedded font instead of inferring runs from flattened text.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTextRun {
    pub text: String,
    pub x_offset: f32,
    pub y_offset: f32,
    pub text_matrix: RenderedTextMatrix,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub glyphs: Option<Vec<RenderedGlyph>>,
}

/// PDF text matrix orientation for one shaped text run.
///
/// CSS Writing Modes can place the same shaped glyph stream on a horizontal
/// or vertical baseline. Keeping the 2x2 matrix with the run lets layout own
/// writing-mode placement while PDF emission only applies the selected text
/// matrix:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-flow> and
/// ISO 32000-2:2020, 9.4.4 "Text Space Details".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedTextMatrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

impl RenderedTextMatrix {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };
    pub const ROTATE_CW: Self = Self {
        a: 0.0,
        b: -1.0,
        c: 1.0,
        d: 0.0,
    };
    pub const ROTATE_CCW: Self = Self {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
    };

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

/// Shaped glyph data kept with painted text for PDF emission.
///
/// CSS Fonts requires text to be shaped with the selected font face before
/// glyph emission; PDF text objects then encode glyph IDs with positioning and
/// ToUnicode extraction data.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyph {
    pub id: u16,
    pub x_advance: f32,
    pub nominal_x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub unicode: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLink {
    pub(in crate::document) rect: PaintRect,
    pub target: String,
}

impl RenderedLink {
    pub(crate) fn from_paint_rect(rect: PaintRect, target: String) -> Self {
        Self { rect, target }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }

    pub(in crate::document) fn transformed(&self, transform: PaintTransform) -> Self {
        let clip = transform.apply_clip_to_aabb(PaintClip::from_paint_rect(self.rect));
        Self::from_paint_rect(clip.paint_rect(), self.target.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImage {
    pub background: bool,
    pub(in crate::document) rect: PaintRect,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub source_rect: Option<RenderedImageSourceRect>,
    pub interpolate: bool,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
    pub alt_text: Option<String>,
    clip: Option<RenderedPathClip>,
}

impl RenderedImage {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        pixel_width: u32,
        pixel_height: u32,
        source_rect: Option<RenderedImageSourceRect>,
        interpolate: bool,
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            background,
            rect,
            pixel_width,
            pixel_height,
            source_rect,
            interpolate,
            rgb,
            alpha,
            alt_text,
            clip: None,
        }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn set_paint_rect(&mut self, rect: PaintRect) {
        self.rect = rect;
    }

    /// Attach a PDF path clipping scope to an image draw operation.
    ///
    /// CSS Backgrounds clips background image layers to the selected
    /// `background-clip` area, including rounded corners from
    /// `border-radius`:
    /// <https://www.w3.org/TR/css-backgrounds-3/#corner-clipping>.
    pub(crate) fn with_clip(mut self, clip: RenderedPathClip) -> Self {
        self.clip = Some(clip);
        self
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }
}

impl RenderedImage {
    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested_clip in &mut clip.additional_clips {
                for command in &mut nested_clip.commands {
                    command.translate(offset);
                }
            }
        }
        self
    }
}

/// Pixel-space source rectangle for drawing a cropped PDF image XObject.
///
/// CSS Border Images use nine-slice scaling: each destination border segment
/// maps to a source image slice. PDF image XObjects have fixed pixel data, so
/// source cropping is normalized before resource emission:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-images> and ISO
/// 32000-1:2008, 8.9 "Images".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderedImageSourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl RenderedImageSourceRect {
    pub fn x(&self) -> u32 {
        self.x
    }

    pub fn y(&self) -> u32 {
        self.y
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_clip_round_trips_through_typed_rect() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(30.0, 40.0));
        let clip = PaintClip::from_paint_rect(rect);

        assert_eq!(clip, PaintClip::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(clip.paint_rect(), rect);
    }

    #[test]
    fn rendered_rect_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let rendered = RenderedRect::from_paint_rect(rect, Some(Color::BLACK));

        assert_eq!(rendered.paint_rect(), rect);
        assert_eq!(rendered.fill, Some(Color::BLACK));
    }

    #[test]
    fn rendered_image_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let image = RenderedImage::from_paint_rect(
            rect,
            false,
            5,
            6,
            None,
            false,
            Vec::new(),
            None,
            Some("alt".to_string()),
        );

        assert_eq!(image.paint_rect(), rect);
        assert_eq!(image.width(), 5.0);
        assert_eq!(image.height(), 6.0);
    }

    #[test]
    fn paint_rect_to_pdf_is_identity_for_unrotated_pages() {
        let rect = PaintRect::new(PaintPoint::new(7.0, 8.0), PaintSize::new(9.0, 10.0));

        assert_eq!(
            paint_rect_to_pdf(rect),
            PdfRect::new(PdfPoint::new(7.0, 8.0), PdfSize::new(9.0, 10.0))
        );
    }

    #[test]
    fn paint_point_to_pdf_is_identity_for_unrotated_pages() {
        assert_eq!(
            paint_point_to_pdf(PaintPoint::new(11.0, 12.0)),
            PdfPoint::new(11.0, 12.0)
        );
    }

    #[test]
    fn paint_transform_maps_typed_points_and_clips() {
        let transform = PaintTransform::translate(PaintVector::new(5.0, -2.0));

        assert_eq!(
            transform.apply_point(PaintPoint::new(10.0, 20.0)),
            PaintPoint::new(15.0, 18.0)
        );
        assert_eq!(
            transform.apply_clip_to_aabb(PaintClip::from_paint_rect(PaintRect::new(
                PaintPoint::new(10.0, 20.0),
                PaintSize::new(30.0, 40.0),
            ))),
            PaintClip::from_paint_rect(PaintRect::new(
                PaintPoint::new(15.0, 18.0),
                PaintSize::new(30.0, 40.0),
            ))
        );
    }

    #[test]
    fn path_commands_expose_typed_paint_points() {
        assert_eq!(
            RenderedPathCommand::move_to(PaintPoint::new(1.0, 2.0)).typed_points(),
            RenderedPathCommandPoints::MoveTo(PaintPoint::new(1.0, 2.0))
        );
        assert_eq!(
            RenderedPathCommand::curve_to(
                PaintPoint::new(1.0, 2.0),
                PaintPoint::new(3.0, 4.0),
                PaintPoint::new(5.0, 6.0),
            )
            .typed_points(),
            RenderedPathCommandPoints::CurveTo {
                control_1: PaintPoint::new(1.0, 2.0),
                control_2: PaintPoint::new(3.0, 4.0),
                end: PaintPoint::new(5.0, 6.0),
            }
        );
    }

    #[test]
    fn stroke_and_line_expose_typed_paint_points() {
        let stroke = RenderedStroke::from_paint_points(
            PaintPoint::new(1.0, 2.0),
            PaintPoint::new(3.0, 4.0),
            1.0,
            Color::BLACK,
            None,
        );
        assert_eq!(
            stroke.paint_points(),
            (PaintPoint::new(1.0, 2.0), PaintPoint::new(3.0, 4.0))
        );

        let line = RenderedLine::from_paint_origin(
            "text".to_string(),
            PaintPoint::new(5.0, 6.0),
            10.0,
            None,
            Color::BLACK,
            Vec::new(),
        );
        assert_eq!(line.origin(), PaintPoint::new(5.0, 6.0));
    }

    #[test]
    fn stroke_line_and_link_expose_typed_paint_bounds() {
        let stroke = RenderedStroke::from_paint_points(
            PaintPoint::new(10.0, 20.0),
            PaintPoint::new(30.0, 40.0),
            4.0,
            Color::BLACK,
            None,
        );
        assert_eq!(stroke.paint_bounds(), PaintClip::new(8.0, 18.0, 24.0, 24.0));

        let link = RenderedLink::from_paint_rect(
            PaintRect::new(PaintPoint::new(1.0, 2.0), PaintSize::new(3.0, 4.0)),
            "https://example.com".to_string(),
        );
        assert_eq!(
            link.paint_rect(),
            PaintRect::new(PaintPoint::new(1.0, 2.0), PaintSize::new(3.0, 4.0))
        );
    }
}
