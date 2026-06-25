/// Computed CSS value for `column-width`.
///
/// CSS Multi-column Layout defines `column-width` as `auto | <length>`, with
/// the actual column count and used column width derived later by layout:
/// <https://www.w3.org/TR/css-multicol-1/#cw>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedColumnWidth {
    Auto,
    Length(f32),
}

impl ComputedColumnWidth {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn resolve_font_metric_lengths(&mut self, _ch_advance: f32) {}
}
