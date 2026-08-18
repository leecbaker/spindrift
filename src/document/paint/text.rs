use std::ops::Deref;
use std::ops::Range;
use std::rc::Rc;

use crate::CssColor;
use crate::css::FontPalette;

use super::geometry::{
    PaintClip, PaintDisplacement, PaintPoint, PaintRect, PaintSize, PaintTranslation,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub text: String,
    pub(in crate::document) origin: PaintPoint,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub color: CssColor,
    pub runs: Vec<RenderedTextRun>,
    pub(crate) glyph_ink_bounds: Option<PaintClip>,
    /// Displacement applied when converting the CSS layout baseline to the
    /// embedded font program's glyph origin. This remains private renderer
    /// metadata so later layout code can recover the original CSS baseline
    /// without guessing from a fallback run's font metrics.
    pub(crate) glyph_origin_adjustment: PaintDisplacement,
    pub(crate) source: RenderedLineSource,
    /// Source identity carried only by text prepared from inline layout.
    ///
    /// It allows public line metadata to join explicit tracking units from a
    /// single authored run while keeping separately painted artifacts such as
    /// text-emphasis marks distinct.
    pub(crate) source_run: Option<Rc<()>>,
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
        color: CssColor,
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
        color: CssColor,
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
        color: CssColor,
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
            glyph_ink_bounds: None,
            glyph_origin_adjustment: PaintDisplacement::zero(),
            source,
            source_run: None,
        }
    }

    /// Attach the conversion used to map this line's CSS baseline into PDF
    /// glyph-program coordinates.
    pub(crate) fn with_glyph_origin_adjustment(
        mut self,
        glyph_origin_adjustment: PaintDisplacement,
    ) -> Self {
        self.glyph_origin_adjustment = glyph_origin_adjustment;
        self
    }

    pub(crate) fn with_glyph_ink_bounds(mut self, bounds: Option<PaintClip>) -> Self {
        self.glyph_ink_bounds = bounds;
        self
    }

    pub(crate) fn with_source_run(mut self, source_run: Rc<()>) -> Self {
        self.source_run = Some(source_run);
        self
    }

    pub(crate) fn glyph_origin_adjustment(&self) -> PaintDisplacement {
        self.glyph_origin_adjustment
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
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
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
        glyph_ink_bounds: source.glyph_ink_bounds,
        runs: runs
            .into_iter()
            .map(|mut run| {
                run.x_offset -= x_offset;
                run.y_offset -= y_offset;
                run
            })
            .collect(),
        glyph_origin_adjustment: source.glyph_origin_adjustment,
        source: source.source,
        source_run: source.source_run.clone(),
    }
}

/// Return whether two adjacent visible fragments are explicit letter-spacing
/// units from the same authored text run.
///
/// Graph layout splits tracked text into typographic units to apply CSS Text's
/// inter-character spacing.  The public paint summary may join those units
/// again only when the opaque source identity proves they came from one
/// authored run; matching paint attributes alone would wrongly join generated
/// emphasis marks or unrelated adjacent inline content.
pub(in crate::document) fn rendered_lines_can_merge_as_tracking_continuation(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    let (Some(left_source_run), Some(right_source_run)) = (&left.source_run, &right.source_run)
    else {
        return false;
    };
    if !Rc::ptr_eq(left_source_run, right_source_run) {
        return false;
    }
    let y_tolerance = left.font_size.min(right.font_size) * 0.25;
    if (left.origin.y - right.origin.y).abs() > y_tolerance
        || right.origin.x < left.origin.x
        || left.source != right.source
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
        || (left.font_size - right.font_size).abs() >= 0.01
        || rendered_line_first_significant_font_id(left)
            != rendered_line_first_significant_font_id(right)
        || left.color != right.color
        || left.text.chars().last().is_some_and(char::is_whitespace)
        || right.text.chars().next().is_some_and(char::is_whitespace)
    {
        return false;
    }
    let gap = rendered_line_visual_start(right) - rendered_line_visual_end(left);
    gap > 0.1 && gap <= left.font_size
}

pub(in crate::document) fn rendered_lines_can_merge_with_word_gap(
    left: &RenderedLine,
    right: &RenderedLine,
) -> bool {
    if (left.origin.y - right.origin.y).abs() >= 0.01
        || right.origin.x < left.origin.x
        || left.glyph_origin_adjustment != right.glyph_origin_adjustment
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
    /// The CSS palette selected while shaping this run. This is retained only
    /// until color-glyph extraction has converted COLR glyphs to paint paths.
    pub(crate) font_palette: FontPalette,
    pub glyphs: Option<RenderedGlyphs>,
    /// Original line-local ranges for the rendered glyphs, retained while
    /// layout applies vertical writing-mode placement. PDF emission does not
    /// consume this provenance.
    pub(crate) glyph_source_ranges: Option<Rc<[Option<Range<usize>>]>>,
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

/// How one shaped text record contributes to PDF text output.
///
/// CSS preserved tabs participate in inline layout but are not glyphs.  PDF
/// emission must advance the text cursor for them without encoding a
/// substitute `.notdef` glyph from whichever font happened to shape the
/// control character:
/// <https://drafts.csswg.org/css-text-3/#tab-size-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedGlyphKind {
    /// An ordinary paintable glyph from the selected font program.
    Paint(u16),
    VectorPath(u16),
    /// A source-owned layout advance with no glyph to paint or subset.
    AdvanceOnly,
}

/// Shaped glyph data kept with painted text for PDF emission.
///
/// CSS Fonts requires text to be shaped with the selected font face before
/// glyph emission; PDF text objects then encode glyph IDs with positioning and
/// ToUnicode extraction data.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyph {
    pub kind: RenderedGlyphKind,
    pub x_advance: f32,
    pub nominal_x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub unicode: String,
}

impl RenderedGlyph {
    pub const fn painted_id(&self) -> Option<u16> {
        match self.kind {
            RenderedGlyphKind::Paint(id) | RenderedGlyphKind::VectorPath(id) => Some(id),
            RenderedGlyphKind::AdvanceOnly => None,
        }
    }

    pub const fn is_advance_only(&self) -> bool {
        matches!(self.kind, RenderedGlyphKind::AdvanceOnly)
    }

    pub const fn is_painted_by_vector_path(&self) -> bool {
        matches!(self.kind, RenderedGlyphKind::VectorPath(_))
    }
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

    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RenderedGlyph> {
        self.0.iter()
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{
        RenderedGlyph, RenderedGlyphKind, RenderedLine, RenderedTextMatrix, RenderedTextRun,
    };
    use crate::CssColor;
    use crate::document::paint::geometry::PaintPoint;

    fn test_rendered_glyph(unicode: &str) -> RenderedGlyph {
        RenderedGlyph {
            kind: RenderedGlyphKind::Paint(42),
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
            font_palette: crate::css::FontPalette::Normal,
            glyphs: Some(vec![test_rendered_glyph("A")].into()),
            glyph_source_ranges: None,
        }
    }

    fn test_rendered_line() -> RenderedLine {
        RenderedLine::from_paint_origin(
            "A".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
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
    fn line_exposes_a_typed_paint_origin() {
        let line = RenderedLine::from_paint_origin(
            "text".to_string(),
            PaintPoint::new(5.0, 6.0),
            10.0,
            None,
            CssColor::BLACK,
            Vec::new(),
        );
        assert_eq!(line.origin(), PaintPoint::new(5.0, 6.0));
    }
}
