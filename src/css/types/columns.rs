use super::ComputedLengthPercentage;

/// Computed CSS value for `column-width`.
///
/// CSS Multi-column Layout defines `column-width` as `auto | <length>`, with
/// the actual column count and used column width derived later by layout:
/// <https://www.w3.org/TR/css-multicol-1/#cw>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedColumnWidth {
    Auto,
    Length(ComputedLengthPercentage),
}

impl ComputedColumnWidth {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}
