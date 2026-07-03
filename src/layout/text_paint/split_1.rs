use super::*;

/// Used values for one CSS text-decoration stroke.
///
/// CSS Text Decoration resolves line style, color, thickness, offset, and
/// skip-ink before painting each decoration line:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PreparedTextDecorationStroke {
    pub(in crate::layout) axis: TextDecorationStrokeAxis,
    pub(in crate::layout) line_x: f32,
    pub(in crate::layout) line_y: f32,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_length: f32,
    pub(in crate::layout) block_position: f32,
    pub(in crate::layout) thickness: f32,
    pub(in crate::layout) color: Color,
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
    #[allow(dead_code)]
    pub(in crate::layout) source_text: String,
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
}
