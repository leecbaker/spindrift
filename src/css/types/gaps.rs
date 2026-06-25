use super::*;

/// Computed CSS value for `row-gap` and `column-gap`.
///
/// CSS Box Alignment defines gap properties as `normal | <length-percentage>`,
/// and CSS Cascade keeps `normal` as a computed keyword until the relevant
/// layout mode computes used values:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedGap {
    Normal,
    LengthPercentage(ComputedLengthPercentage),
}

impl ComputedGap {
    pub(crate) const NORMAL: Self = Self::Normal;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}
