use super::*;
use crate::image_store::ImageId;
use std::ops::Deref;
use std::rc::Rc;

#[allow(dead_code)]
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.start = offset.transform_point(self.start);
        self.end = offset.transform_point(self.end);
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
    RunIn,
    InlineAtom,
    Marker,
}

#[allow(dead_code)]
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

    /// Returns the line origin's horizontal position in PDF points.
    pub fn x(&self) -> f32 {
        self.origin.x
    }

    /// Returns the line origin's vertical position in PDF points.
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.origin = offset.transform_point(self.origin);
        self
    }

    pub(crate) fn translate_origin(&mut self, offset: PaintTranslation) {
        self.origin = offset.transform_point(self.origin);
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
    let text = runs.iter().map(|run| run.text.as_ref()).collect::<String>();
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

/// Accumulates the page-local bounds of paint geometry, including points and
/// line segments with zero area.
///
/// [`PaintRect::union`] deliberately ignores empty rectangles, which is right
/// for rectangle geometry but would discard path endpoints while calculating
/// paint bounds. This accumulator instead extends its typed extrema with every
/// included point before producing a rectangle at the bounds boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::document) struct PaintBounds {
    min: PaintPoint,
    max: PaintPoint,
}

impl PaintBounds {
    pub(in crate::document) fn from_paint_point(point: PaintPoint) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    pub(in crate::document) fn from_paint_rect(rect: PaintRect) -> Self {
        Self {
            min: rect.origin,
            max: PaintPoint::new(rect.max_x(), rect.max_y()),
        }
    }

    pub(in crate::document) fn include_paint_point(&mut self, point: PaintPoint) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    pub(in crate::document) fn include_paint_rect(&mut self, rect: PaintRect) {
        self.include_paint_point(rect.origin);
        self.include_paint_point(PaintPoint::new(rect.max_x(), rect.max_y()));
    }

    pub(in crate::document) fn paint_rect(self) -> PaintRect {
        PaintRect::new(
            self.min,
            PaintSize::new(self.max.x - self.min.x, self.max.y - self.min.y),
        )
    }

    pub(in crate::document) fn into_paint_clip(self) -> PaintClip {
        PaintClip::from_paint_rect(self.paint_rect())
    }
}

/// Build a closed PDF path around one page-local paint rectangle.
///
/// Keeping the rectangle intact until this paint-primitive boundary avoids
/// reassembling its four corners from untyped scalar coordinates. PDF paths
/// are ordered counter-clockwise from the bottom-left corner:
/// ISO 32000-2:2020, 8.5.2 "Path Construction Operators".
pub(crate) fn paint_rect_path_commands(rect: PaintRect) -> Vec<RenderedPathCommand> {
    vec![
        RenderedPathCommand::move_to(rect.origin),
        RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.min_y())),
        RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.max_y())),
        RenderedPathCommand::line_to(PaintPoint::new(rect.min_x(), rect.max_y())),
        RenderedPathCommand::Close,
    ]
}

pub(in crate::document) fn path_bounds(path: &RenderedPath) -> Option<PaintClip> {
    let mut bounds: Option<PaintBounds> = None;
    for command in &path.commands {
        for point in command_points(*command) {
            match &mut bounds {
                Some(bounds) => bounds.include_paint_point(point),
                None => bounds = Some(PaintBounds::from_paint_point(point)),
            }
        }
    }
    bounds.map(|bounds| {
        let bounds = bounds.paint_rect();
        let outset = path.stroke_width.max(0.0) / 2.0;
        PaintClip::new(
            bounds.origin.x - outset,
            bounds.origin.y - outset,
            bounds.size.width + outset * 2.0,
            bounds.size.height + outset * 2.0,
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
    /// Logical source text for this visual glyph run.
    pub text: Rc<str>,
    /// Exact logical replacement text for a shaped cluster that cannot be
    /// represented faithfully by the run's individual glyph ToUnicode
    /// entries.
    ///
    /// PDF's `/ActualText` property supplies the authoritative extraction
    /// value for the enclosed glyph sequence. This is needed when OpenType
    /// shaping emits several glyphs for one source cluster (or attributes a
    /// glyph to an authored default-ignorable control), while `glyphs` still
    /// preserves the selected glyph ids and positions for painting.
    /// ISO 32000-2:2020, 14.9.4.4 "ActualText".
    pub(crate) actual_text: Option<Rc<str>>,
    pub x_offset: f32,
    pub y_offset: f32,
    pub text_matrix: RenderedTextMatrix,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub glyphs: Option<RenderedGlyphs>,
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
pub struct RenderedTextMatrix(euclid::Transform2D<f32, TextRunSpace, TextRunSpace>);

/// Coordinates local to a shaped run before its writing-mode matrix applies.
/// They are deliberately distinct from page-local paint coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRunSpace {}

pub type TextRunPoint = euclid::Point2D<f32, TextRunSpace>;

impl RenderedTextMatrix {
    pub const IDENTITY: Self = Self(euclid::Transform2D::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    pub const ROTATE_CW: Self = Self(euclid::Transform2D::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0));
    pub const ROTATE_CCW: Self = Self(euclid::Transform2D::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0));

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    pub fn transform_local_point(self, point: TextRunPoint) -> TextRunPoint {
        self.0.transform_point(point)
    }

    pub(crate) fn pdf_components(self) -> [f32; 4] {
        [self.0.m11, self.0.m12, self.0.m21, self.0.m22]
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

/// Shared immutable glyph storage for painted text runs.
///
/// Layout may clone rendered lines and paint fragments while replaying CSS
/// stacking contexts, shadows, and fragmented content. Glyph records are
/// immutable once they reach `RenderedTextRun`, so sharing the slice preserves
/// the exact shaped output without deep-copying glyph data on each replay:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// ISO 32000-2:2020, 9.4 "Text".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyphs(Rc<[RenderedGlyph]>);

impl RenderedGlyphs {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RenderedGlyph> {
        self.0.iter()
    }

    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for RenderedGlyphs {
    type Target = [RenderedGlyph];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a RenderedGlyphs {
    type Item = &'a RenderedGlyph;
    type IntoIter = std::slice::Iter<'a, RenderedGlyph>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl From<Vec<RenderedGlyph>> for RenderedGlyphs {
    fn from(glyphs: Vec<RenderedGlyph>) -> Self {
        Self(Rc::from(glyphs.into_boxed_slice()))
    }
}

impl FromIterator<RenderedGlyph> for RenderedGlyphs {
    fn from_iter<T: IntoIterator<Item = RenderedGlyph>>(iter: T) -> Self {
        iter.into_iter().collect::<Vec<_>>().into()
    }
}

/// A resolved document link annotation in page-local PDF-point coordinates.
///
/// ```no_run
/// # fn inspect(link: &quire::LinkAnnotation) {
/// println!("{}", link.target());
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LinkAnnotation {
    pub(crate) rect: PaintRect,
    pub(crate) target: Rc<str>,
}

impl LinkAnnotation {
    pub(crate) fn from_paint_rect(rect: PaintRect, target: impl Into<Rc<str>>) -> Self {
        Self {
            rect,
            target: target.into(),
        }
    }

    /// Returns the link rectangle's horizontal position in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let x = link.x();
    /// # let _ = x;
    /// # }
    /// ```
    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    /// Returns the link rectangle's vertical position in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let y = link.y();
    /// # let _ = y;
    /// # }
    /// ```
    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    /// Returns the link rectangle's width in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let width = link.width();
    /// # let _ = width;
    /// # }
    /// ```
    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    /// Returns the link rectangle's height in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let height = link.height();
    /// # let _ = height;
    /// # }
    /// ```
    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    /// Returns the resolved external URL or internal fragment target.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// assert!(!link.target().is_empty());
    /// # }
    /// ```
    pub fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self
    }

    pub(in crate::document) fn transformed(&self, transform: PaintTransform) -> Self {
        let clip = transform.apply_clip_to_aabb(PaintClip::from_paint_rect(self.rect));
        Self::from_paint_rect(clip.paint_rect(), Rc::clone(&self.target))
    }
}

pub(crate) type RenderedLink = LinkAnnotation;

/// The only two legal ownership models for image paint operations.
///
/// Store-backed images are document-local handles and therefore cannot carry
/// expanded samples in page paint records. Inline samples are retained only
/// for programmatically constructed legacy/test images that have no document
/// image-store source.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedImageSource {
    Stored {
        image_id: ImageId,
        source_rect: RenderedImageSourceRect,
        pixel_width: u32,
        pixel_height: u32,
    },
    Inline {
        raster: InlineRasterImage,
        source_rect: Option<RenderedImageSourceRect>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineRasterImage {
    pub(crate) pixel_width: u32,
    pub(crate) pixel_height: u32,
    pub(crate) color_space: crate::color::RasterColorSpace,
    pub(crate) rgb: Rc<[u8]>,
    pub(crate) alpha: Option<Rc<[u8]>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImage {
    pub background: bool,
    pub(crate) source: RenderedImageSource,
    pub(in crate::document) rect: PaintRect,
    pub interpolate: bool,
    pub alt_text: Option<Rc<str>>,
    /// Exact logical text represented by this otherwise non-text paint.
    ///
    /// Bitmap OpenType glyphs are emitted as PDF image XObjects.  `/ActualText`
    /// preserves their authored Unicode content for extraction without
    /// conflating a glyph replacement with an image's alternative text.
    pub(crate) actual_text: Option<Rc<str>>,
    /// Optional local-to-page transform for paint sources whose natural
    /// geometry is not axis-aligned in page space, such as vertical bitmap
    /// OpenType glyphs.
    pub(crate) transform: Option<PaintTransform>,
    clip: Option<RenderedPathClip>,
}

#[allow(dead_code)]
impl RenderedImage {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        pixel_width: u32,
        pixel_height: u32,
        source_rect: Option<RenderedImageSourceRect>,
        interpolate: bool,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
        alt_text: Option<Rc<str>>,
    ) -> Self {
        Self {
            background,
            source: RenderedImageSource::Inline {
                raster: InlineRasterImage {
                    pixel_width,
                    pixel_height,
                    color_space: crate::color::RasterColorSpace::SRGB,
                    rgb,
                    alpha,
                },
                source_rect,
            },
            rect,
            interpolate,
            alt_text,
            actual_text: None,
            transform: None,
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

    /// Associate an exact Unicode replacement string with this image paint.
    pub(crate) fn with_actual_text(mut self, actual_text: Rc<str>) -> Self {
        if !actual_text.is_empty() {
            self.actual_text = Some(actual_text);
        }
        self
    }

    /// Apply a local affine transform before placing this image rectangle.
    pub(crate) fn with_transform(mut self, transform: PaintTransform) -> Self {
        self.transform = Some(transform);
        self
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    /// Whether this image is constrained by a paint clip.
    ///
    /// CSS background clipping is represented as a destination-space clip
    /// rather than destructively shrinking the image's destination rectangle.
    pub fn is_clipped(&self) -> bool {
        self.clip.is_some()
    }

    pub(crate) fn with_image_id(mut self, image_id: Option<ImageId>) -> Self {
        if let Some(image_id) = image_id {
            let RenderedImageSource::Inline {
                raster,
                source_rect,
            } = &self.source
            else {
                return self;
            };
            self.source = RenderedImageSource::Stored {
                image_id,
                source_rect: source_rect.unwrap_or(RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: raster.pixel_width,
                    height: raster.pixel_height,
                }),
                pixel_width: raster.pixel_width,
                pixel_height: raster.pixel_height,
            };
        }
        self
    }

    /// Preserve the calibrated component space of generated inline samples.
    pub(crate) fn with_raster_color_space(
        mut self,
        color_space: crate::color::RasterColorSpace,
    ) -> Self {
        if let RenderedImageSource::Inline { raster, .. } = &mut self.source {
            raster.color_space = color_space;
        }
        self
    }

    /// Returns the pixel rectangle selected from the intrinsic image.
    ///
    /// A store-backed image always has an explicit full-image or cropped
    /// source rectangle; legacy inline images preserve whether a crop was
    /// specified by their creator.
    pub fn source_rect(&self) -> Option<RenderedImageSourceRect> {
        match &self.source {
            RenderedImageSource::Stored { source_rect, .. } => Some(*source_rect),
            RenderedImageSource::Inline { source_rect, .. } => *source_rect,
        }
    }

    /// Returns the intrinsic image width in device pixels.
    pub fn pixel_width(&self) -> u32 {
        match &self.source {
            RenderedImageSource::Stored { pixel_width, .. } => *pixel_width,
            RenderedImageSource::Inline { raster, .. } => raster.pixel_width,
        }
    }

    /// Returns the intrinsic image height in device pixels.
    pub fn pixel_height(&self) -> u32 {
        match &self.source {
            RenderedImageSource::Stored { pixel_height, .. } => *pixel_height,
            RenderedImageSource::Inline { raster, .. } => raster.pixel_height,
        }
    }

    pub(crate) fn set_source_rect(&mut self, source_rect: RenderedImageSourceRect) {
        match &mut self.source {
            RenderedImageSource::Stored {
                source_rect: current,
                ..
            } => *current = source_rect,
            RenderedImageSource::Inline {
                source_rect: current,
                ..
            } => *current = Some(source_rect),
        }
    }

    pub(crate) fn inline_pixel_size(&self) -> Option<(u32, u32)> {
        match &self.source {
            RenderedImageSource::Stored { .. } => None,
            RenderedImageSource::Inline { raster, .. } => {
                Some((raster.pixel_width, raster.pixel_height))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn pixel_storage_ptr_eq(&self, other: &Self) -> bool {
        match (&self.source, &other.source) {
            (
                RenderedImageSource::Stored { image_id: left, .. },
                RenderedImageSource::Stored {
                    image_id: right, ..
                },
            ) => left == right,
            (
                RenderedImageSource::Inline { raster: left, .. },
                RenderedImageSource::Inline { raster: right, .. },
            ) => {
                Rc::ptr_eq(&left.rgb, &right.rgb)
                    && match (&left.alpha, &right.alpha) {
                        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl RenderedImage {
    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
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

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImagePattern {
    pub background: bool,
    pub(crate) source: RenderedImageSource,
    pub(in crate::document) rect: PaintRect,
    pub tile_width: f32,
    pub tile_height: f32,
    pub step_width: f32,
    pub step_height: f32,
    pub origin: PaintPoint,
    pub interpolate: bool,
    clip: Option<RenderedPathClip>,
}

#[allow(dead_code)]
impl RenderedImagePattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        tile_width: f32,
        tile_height: f32,
        step_width: f32,
        step_height: f32,
        origin: PaintPoint,
        pixel_width: u32,
        pixel_height: u32,
        interpolate: bool,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
    ) -> Self {
        Self {
            background,
            source: RenderedImageSource::Inline {
                raster: InlineRasterImage {
                    pixel_width,
                    pixel_height,
                    color_space: crate::color::RasterColorSpace::SRGB,
                    rgb,
                    alpha,
                },
                source_rect: None,
            },
            rect,
            tile_width,
            tile_height,
            step_width,
            step_height,
            origin,
            interpolate,
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

    /// Attach a PDF path clipping scope to a repeated background pattern.
    ///
    /// CSS Backgrounds clips each repeated image layer to the selected
    /// `background-clip` area, including rounded corners:
    /// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
    pub(crate) fn with_clip(mut self, clip: RenderedPathClip) -> Self {
        self.clip = Some(clip);
        self
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(crate) fn with_image_id(mut self, image_id: Option<ImageId>) -> Self {
        if let Some(image_id) = image_id {
            let RenderedImageSource::Inline { raster, .. } = &self.source else {
                return self;
            };
            self.source = RenderedImageSource::Stored {
                image_id,
                source_rect: RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: raster.pixel_width,
                    height: raster.pixel_height,
                },
                pixel_width: raster.pixel_width,
                pixel_height: raster.pixel_height,
            };
        }
        self
    }

    /// Preserve the calibrated component space of generated inline samples.
    pub(crate) fn with_raster_color_space(
        mut self,
        color_space: crate::color::RasterColorSpace,
    ) -> Self {
        if let RenderedImageSource::Inline { raster, .. } = &mut self.source {
            raster.color_space = color_space;
        }
        self
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self.origin = offset.transform_point(self.origin);
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

/// A repeated CSS gradient painted by a reusable PDF tiling pattern.
///
/// The pattern stores resolved CSS tile geometry independently from its PDF
/// resource allocation. Its cell paints the shared axial or radial shading;
/// the outer path applies CSS `background-clip`.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGradientPattern {
    pub(crate) rect: PaintRect,
    pub(crate) tile_width: f32,
    pub(crate) tile_height: f32,
    pub(crate) step_width: f32,
    pub(crate) step_height: f32,
    pub(crate) origin: PaintPoint,
    pub(crate) gradient: RenderedGradient,
    clip: Option<RenderedPathClip>,
}

#[allow(dead_code)]
impl RenderedGradientPattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        rect: PaintRect,
        tile_width: f32,
        tile_height: f32,
        step_width: f32,
        step_height: f32,
        origin: PaintPoint,
        gradient: RenderedGradient,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            rect,
            tile_width,
            tile_height,
            step_width,
            step_height,
            origin,
            gradient,
            clip,
        }
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
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
    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self.origin = offset.transform_point(self.origin);
        self.gradient.transform =
            PaintTransform::translate(offset).multiply(self.gradient.transform);
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested in &mut clip.additional_clips {
                for command in &mut nested.commands {
                    command.translate(offset);
                }
            }
        }
        self
    }
}

/// A reusable vector tile for a repeated URL SVG background.
///
/// PDF emission serializes its paths once into a Form XObject and invokes that
/// form from a Type 1 tiling pattern, avoiding one page primitive per CSS tile.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedSvgPattern {
    pub(crate) rect: PaintRect,
    pub(crate) tile_width: f32,
    pub(crate) tile_height: f32,
    pub(crate) step_width: f32,
    pub(crate) step_height: f32,
    pub(crate) origin: PaintPoint,
    pub(crate) paths: Vec<RenderedPath>,
    clip: Option<RenderedPathClip>,
}

impl RenderedSvgPattern {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        rect: PaintRect,
        tile_width: f32,
        tile_height: f32,
        step_width: f32,
        step_height: f32,
        origin: PaintPoint,
        paths: Vec<RenderedPath>,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            rect,
            tile_width,
            tile_height,
            step_width,
            step_height,
            origin,
            paths,
            clip,
        }
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn clip(&self) -> Option<&RenderedPathClip> {
        self.clip.as_ref()
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self.origin = offset.transform_point(self.origin);
        // `paths` are local to the Form XObject cell and deliberately remain
        // at its origin. Only the page placement and its CSS clip move.
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested in &mut clip.additional_clips {
                for command in &mut nested.commands {
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

    fn paint_rect(x: f32, y: f32, width: f32, height: f32) -> PaintRect {
        PaintRect::new(PaintPoint::new(x, y), PaintSize::new(width, height))
    }

    fn test_rendered_glyph(unicode: &str) -> RenderedGlyph {
        RenderedGlyph {
            id: 42,
            x_advance: 7.0,
            nominal_x_advance: 7.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        }
    }

    fn test_rendered_text_run() -> RenderedTextRun {
        RenderedTextRun {
            text: Rc::from("A"),
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(0),
            glyphs: Some(vec![test_rendered_glyph("A")].into()),
        }
    }

    fn test_rendered_line() -> RenderedLine {
        RenderedLine::from_paint_origin(
            "A".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            Color::BLACK,
            vec![test_rendered_text_run()],
        )
    }

    #[test]
    fn cloned_text_runs_and_lines_share_rendered_glyph_storage() {
        let run = test_rendered_text_run();
        let cloned_run = run.clone();
        assert!(
            run.glyphs
                .as_ref()
                .unwrap()
                .ptr_eq(cloned_run.glyphs.as_ref().unwrap())
        );

        let line = test_rendered_line();
        let cloned_line = line.clone();
        assert!(
            line.runs[0]
                .glyphs
                .as_ref()
                .unwrap()
                .ptr_eq(cloned_line.runs[0].glyphs.as_ref().unwrap())
        );
    }

    #[test]
    fn translated_paint_fragment_preserves_shared_rendered_glyph_storage() {
        let line = test_rendered_line();
        let glyphs = line.runs[0].glyphs.as_ref().unwrap().clone();
        let fragment = PaintFragment::from_primitives(vec![PaintPrimitive::Line(line)], Vec::new());

        let translated = fragment.translated(PaintTranslation::new(3.0, -4.0));
        let primitives = translated.flattened_primitives();
        let PaintPrimitive::Line(translated_line) = &primitives[0] else {
            panic!("expected translated line primitive");
        };

        assert_eq!(translated_line.origin(), PaintPoint::new(13.0, 16.0));
        assert!(glyphs.ptr_eq(translated_line.runs[0].glyphs.as_ref().unwrap()));
    }

    #[test]
    fn paint_fragment_translation_moves_paths_patterns_and_links_together() {
        let path = RenderedPath::new(
            vec![RenderedPathCommand::move_to(PaintPoint::new(1.0, 2.0))],
            Some(Color::BLACK),
            RenderedPathFillRule::NonZero,
            None,
            0.0,
            Some(RenderedPathClip::new(
                vec![RenderedPathCommand::line_to(PaintPoint::new(3.0, 4.0))],
                RenderedPathFillRule::NonZero,
                Vec::new(),
            )),
        );
        let pattern = RenderedImagePattern::from_paint_rect(
            PaintRect::new(PaintPoint::new(5.0, 6.0), PaintSize::new(7.0, 8.0)),
            true,
            7.0,
            8.0,
            7.0,
            8.0,
            PaintPoint::new(9.0, 10.0),
            1,
            1,
            true,
            Rc::<[u8]>::from(vec![0, 0, 0]),
            None,
        );
        let link = RenderedLink::from_paint_rect(
            PaintRect::new(PaintPoint::new(11.0, 12.0), PaintSize::new(13.0, 14.0)),
            "https://example.com",
        );
        let translated = PaintFragment::from_primitives(
            vec![
                PaintPrimitive::Path(path),
                PaintPrimitive::ImagePattern(pattern),
            ],
            vec![link],
        )
        .translated(PaintTranslation::new(20.0, -30.0));

        let primitives = translated.flattened_primitives();
        let [
            PaintPrimitive::Path(path),
            PaintPrimitive::ImagePattern(pattern),
        ] = primitives.as_slice()
        else {
            panic!("expected translated path and image pattern");
        };
        assert_eq!(
            path.commands,
            vec![RenderedPathCommand::move_to(PaintPoint::new(21.0, -28.0))]
        );
        assert_eq!(
            path.clip.as_ref().unwrap().commands,
            vec![RenderedPathCommand::line_to(PaintPoint::new(23.0, -26.0))]
        );
        assert_eq!(
            pattern.paint_rect(),
            PaintRect::new(PaintPoint::new(25.0, -24.0), PaintSize::new(7.0, 8.0))
        );
        assert_eq!(pattern.origin, PaintPoint::new(29.0, -20.0));
        assert_eq!(
            translated.links[0].paint_rect(),
            PaintRect::new(PaintPoint::new(31.0, -18.0), PaintSize::new(13.0, 14.0))
        );
    }

    #[test]
    fn fragmentainer_slice_keeps_monolithic_paint_whole_at_its_block_start() {
        let bounds = PaintClip::new(0.0, 0.0, 100.0, 100.0);
        let fragment = PaintFragment::from_primitives(
            vec![PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                bounds.paint_rect(),
                Some(Color::BLACK),
            ))],
            Vec::new(),
        )
        .with_monolithic_fragmentation_scope(bounds);

        let sliced = fragment
            .with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
                PaintClip::new(0.0, 80.0, 100.0, 20.0),
                true,
            );

        let primitives = sliced.flattened_primitives();
        let [PaintPrimitive::Rect(rect)] = primitives.as_slice() else {
            panic!("expected one retained monolithic rectangle");
        };
        assert_eq!(rect.paint_rect(), bounds.paint_rect());
    }

    #[test]
    fn later_fragmentainer_slice_does_not_replay_monolithic_paint() {
        let bounds = PaintClip::new(0.0, 0.0, 100.0, 100.0);
        let fragment = PaintFragment::from_primitives(
            vec![PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                bounds.paint_rect(),
                Some(Color::BLACK),
            ))],
            Vec::new(),
        )
        .with_monolithic_fragmentation_scope(bounds);

        let sliced = fragment
            .with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
                PaintClip::new(0.0, 60.0, 100.0, 20.0),
                true,
            );

        assert!(sliced.flattened_primitives().is_empty());
    }

    #[test]
    fn cloned_images_share_pixel_storage() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(2.0, 1.0)),
            false,
            2,
            1,
            None,
            false,
            Rc::from(vec![1, 2, 3, 4, 5, 6].into_boxed_slice()),
            Some(Rc::from(vec![255, 127].into_boxed_slice())),
            Some(Rc::from("alt")),
        );
        let cloned = image.clone();

        assert!(image.pixel_storage_ptr_eq(&cloned));
    }

    #[test]
    fn paint_clip_round_trips_through_typed_rect() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(30.0, 40.0));
        let clip = PaintClip::from_paint_rect(rect);

        assert_eq!(clip, PaintClip::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(clip.paint_rect(), rect);
    }

    #[test]
    fn paint_clip_translation_and_intersection_delegate_to_typed_rects() {
        let clip = PaintClip::new(10.0, 20.0, 30.0, 40.0);

        assert_eq!(
            clip.translated(PaintTranslation::new(-5.0, 7.0))
                .paint_rect(),
            paint_rect(5.0, 27.0, 30.0, 40.0),
        );
        assert_eq!(
            clip.intersect(PaintClip::new(25.0, 30.0, 30.0, 40.0))
                .unwrap()
                .paint_rect(),
            paint_rect(25.0, 30.0, 15.0, 30.0),
        );
        assert!(
            clip.intersect(PaintClip::new(40.0, 20.0, 10.0, 10.0))
                .is_none()
        );
    }

    #[test]
    fn paint_bounds_retains_degenerate_path_geometry() {
        let mut bounds = PaintBounds::from_paint_point(PaintPoint::new(10.0, 20.0));
        bounds.include_paint_point(PaintPoint::new(30.0, 20.0));
        bounds.include_paint_rect(paint_rect(5.0, 2.0, 0.0, 5.0));

        assert_eq!(bounds.paint_rect(), paint_rect(5.0, 2.0, 25.0, 18.0));
    }

    #[test]
    fn paint_rect_path_commands_preserve_a_nonzero_origin() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(30.0, 40.0));

        assert_eq!(
            paint_rect_path_commands(rect),
            vec![
                RenderedPathCommand::move_to(PaintPoint::new(10.0, 20.0)),
                RenderedPathCommand::line_to(PaintPoint::new(40.0, 20.0)),
                RenderedPathCommand::line_to(PaintPoint::new(40.0, 60.0)),
                RenderedPathCommand::line_to(PaintPoint::new(10.0, 60.0)),
                RenderedPathCommand::Close,
            ]
        );
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
            Rc::from(Vec::new().into_boxed_slice()),
            None,
            Some(Rc::from("alt")),
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
        let transform = PaintTransform::translate(PaintTranslation::new(5.0, -2.0));

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
