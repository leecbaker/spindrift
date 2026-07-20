use super::*;

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
    AfterText,
    All,
}

impl TextDecorationPaintPhase {
    pub(in crate::layout) fn paints_before_text(self) -> bool {
        matches!(self, Self::BeforeText | Self::All)
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
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) decoration: TextDecoration,
    pub(in crate::layout) phase: TextDecorationPaintPhase,
    pub(in crate::layout) color: CssColor,
    pub(in crate::layout) color_override: Option<CssColor>,
    pub(in crate::layout) metrics: TextDecorationFontMetrics,
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
