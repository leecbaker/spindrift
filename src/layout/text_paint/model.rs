use super::*;
use crate::css::TextDecorationLayer;
use std::rc::Rc;

/// A non-empty logical inline range in page-local paint coordinates.
///
/// This is deliberately not a Euclid point or size: text decoration ranges
/// are one-dimensional and their physical axis depends on writing mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct TextInlineSpan {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
}

impl TextInlineSpan {
    pub(in crate::layout) fn new(start: f32, end: f32) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    pub(in crate::layout) fn from_start_and_length(start: f32, length: f32) -> Self {
        Self::new(start, start + length.max(0.0))
    }

    pub(in crate::layout) fn length(self) -> f32 {
        self.end - self.start
    }
}

/// Used values for one CSS text-decoration stroke.
///
/// CSS Text Decoration resolves line style, color, thickness, offset, and
/// skip-ink before painting each decoration line:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PreparedTextDecorationStroke {
    pub(in crate::layout) axis: TextDecorationStrokeAxis,
    /// The physical baseline location of the decorated text run.
    pub(in crate::layout) baseline: PaintPoint,
    /// The resolved logical inline extent to paint.
    pub(in crate::layout) inline_span: TextInlineSpan,
    pub(in crate::layout) block_position: f32,
    pub(in crate::layout) thickness: f32,
    pub(in crate::layout) color: CssColor,
    pub(in crate::layout) style: TextDecorationStyle,
    pub(in crate::layout) skip_ink: TextDecorationSkipInk,
    pub(in crate::layout) skip_spaces: TextDecorationSkipSpaces,
}

/// The used geometry for one decorating origin on one selected line.
///
/// The originating box owns declared decoration values, while the text reached
/// by that origin supplies the per-line considered-text metrics.  Keeping the
/// two percentage bases explicit prevents a propagated `auto` line from
/// accidentally using its ancestor's em box, while fixed and percentage
/// origin values remain attached to their declaration.
///
/// CSS Text Decoration Level 4 § 2.5 and § 2.9.
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-line-uniformity>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationLineGeometry {
    /// The font size resolving a decorating origin's percentage values.
    pub(in crate::layout) origin_font_size: f32,
    /// The font size of eligible considered text on this selected line.
    pub(in crate::layout) considered_font_size: f32,
    /// Selected-font metrics for the considered text.
    pub(in crate::layout) considered_metrics: TextDecorationFontMetrics,
}

impl TextDecorationLineGeometry {
    /// Establish the two CSS percentage bases at the decoration-origin / line
    /// considered-text boundary.
    pub(in crate::layout) fn from_origin_and_considered_text(
        origin_style: &ComputedStyle,
        considered_style: &ComputedStyle,
        considered_metrics: TextDecorationFontMetrics,
    ) -> Self {
        Self {
            origin_font_size: origin_style.font_size,
            considered_font_size: considered_style.font_size,
            considered_metrics,
        }
    }
}

/// The physical selected-glyph coverage contributed by one text group to a
/// decoration origin on a prepared line.
///
/// This deliberately records a line-relative span rather than a fitted line
/// width: preserved white space can remain paintable after its advance has
/// been excluded from line fitting.  The per-origin line adapter intersects
/// this coverage with shared skip ranges before emitting each group's stroke.
///
/// CSS Text Decoration Level 4 § 2.10.3.
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationLineGlyphCoverage {
    pub(in crate::layout) span: TextInlineSpan,
}

/// Visual glyph clusters selected by one decoration origin on one line.
///
/// The line painter collects this once before any individual descendant span
/// emits decoration paint.  This keeps `skip-spaces` independent of DOM/text
/// group boundaries while preserving the shaped physical cluster geometry.
/// CSS Text Decoration Level 4 § 2.10.3.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct TextDecorationLineGlyphSequence {
    pub(in crate::layout) glyphs: Vec<TextDecorationPositionedGlyph>,
}

/// The shared considered-text selection for one decoration origin and one
/// prepared line fragment.
///
/// A propagated decoration paints each eligible text span separately, but its
/// position and thickness must be uniform for the decorating origin across a
/// line.  `line_reference` carries the physical line baseline selected while
/// collecting the line; a text group replaces only its own inline coordinate
/// when it emits a stroke.
///
/// CSS Text Decoration Level 4 § 2.5, "Line Decoration", and § 2.9,
/// "Text Decoration Line: the text-decoration-line property".
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-line-uniformity>
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextDecorationOriginLineGeometry {
    /// The declaration this job paints.  Its `origin_style` `Rc` is the
    /// identity of the box which declared the line; declaration equality is
    /// deliberately not an origin test, because nested equal-looking origins
    /// remain independent in the propagation model.
    pub(in crate::layout) layer: TextDecorationLayer,
    pub(in crate::layout) geometry: TextDecorationLineGeometry,
    /// Complete physical selected-line coverage across eligible contributors.
    pub(in crate::layout) selected_inline_span: Option<TextInlineSpan>,
    /// Receiver spans in inline paint order.  A span is contributed only by a
    /// text group eligible to receive this origin; gaps consequently preserve
    /// atomic-inline boundaries and `text-decoration-skip-self`.
    pub(in crate::layout) receiver_spans: Vec<TextInlineSpan>,
    pub(in crate::layout) glyph_sequence: TextDecorationLineGlyphSequence,
    /// The physical baseline/reference point shared by every selected span.
    pub(in crate::layout) line_reference: PaintPoint,
    /// The decorating box fragment that owns endpoint percentage resolution.
    /// Receiver spans identify descendant text; this owner geometry identifies
    /// the box whose `text-decoration-inset` percentages are resolved.
    pub(in crate::layout) origin_fragment: TextDecorationOriginFragmentGeometry,
}

/// Used inline geometry for one decorating-box fragment.
///
/// CSS Text Decoration resolves `text-decoration-inset` percentages against
/// the complete decorating box for `slice`, or against this fragment for
/// `clone`; the outer fragment edges also determine which endpoints exist.
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextDecorationOriginFragmentGeometry {
    pub(in crate::layout) origin_style: Rc<ComputedStyle>,
    pub(in crate::layout) total_inline_extent: LayoutLength,
    pub(in crate::layout) fragment_inline_extent: LayoutLength,
    /// Inline extent in earlier fragments of this decorating box.
    pub(in crate::layout) preceding_inline_extent: LayoutLength,
    /// Inline extent in later fragments of this decorating box.
    pub(in crate::layout) following_inline_extent: LayoutLength,
    pub(in crate::layout) is_first_fragment: bool,
    pub(in crate::layout) is_last_fragment: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationStrokeAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationSide {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedTextEmphasisMark {
    pub(in crate::layout) mark: String,
    // Test-only verification metadata for Unicode text-emphasis grouping.
    #[cfg(test)]
    pub(in crate::layout) source_text: String,
    pub(in crate::layout) position: PaintPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationPaintPhase {
    BeforeText,
    Underlines,
    Overlines,
    AfterText,
    All,
}

impl TextDecorationPaintPhase {
    pub(in crate::layout) fn paints_underlines(self) -> bool {
        matches!(self, Self::BeforeText | Self::Underlines | Self::All)
    }

    pub(in crate::layout) fn paints_overlines(self) -> bool {
        matches!(self, Self::BeforeText | Self::Overlines | Self::All)
    }

    pub(in crate::layout) fn paints_after_text(self) -> bool {
        matches!(self, Self::AfterText | Self::All)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationLineKind {
    Underline,
    Overline,
    LineThrough,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct TextDecorationPreparationInput<'a> {
    pub(in crate::layout) baseline: PaintPoint,
    pub(in crate::layout) inline_span: TextInlineSpan,
    pub(in crate::layout) inset_start: f32,
    pub(in crate::layout) inset_end: f32,
    /// The descendant text style supplies line geometry and skip-self.
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) decoration: TextDecoration,
    pub(in crate::layout) phase: TextDecorationPaintPhase,
    pub(in crate::layout) color: CssColor,
    pub(in crate::layout) color_override: Option<CssColor>,
    pub(in crate::layout) geometry: TextDecorationLineGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TextDecorationPreparedLineKind {
    Underline,
    Overline,
    LineThrough,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationSegmentInputs {
    pub(in crate::layout) axis: TextDecorationStrokeAxis,
    pub(in crate::layout) line_x: f32,
    pub(in crate::layout) line_y: f32,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_length: f32,
    pub(in crate::layout) block_position: f32,
    pub(in crate::layout) thickness: f32,
    pub(in crate::layout) skip_ink: TextDecorationSkipInk,
    pub(in crate::layout) skip_spaces: TextDecorationSkipSpaces,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextDecorationSegment {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) length: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct TextDecorationPositionedGlyph {
    pub(in crate::layout) unicode: String,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_end: f32,
    pub(in crate::layout) extra_spacing: f32,
}
