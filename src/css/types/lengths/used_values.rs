use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};

/// Largest coordinate representable in Spindrift's default PDF user space.
///
/// PDF limits default user-space coordinates to 200 inches. Spindrift does not
/// currently emit `/UserUnit`, so preserving larger CSS values would create
/// invalid PDF geometry and can turn one box into billions of fragmentainers.
/// Clamp at the CSS used-value boundary instead.
/// <https://www.w3.org/TR/css-values-4/#numeric-ranges>
pub(crate) const MAX_USED_LAYOUT_LENGTH_PT: f32 = 14_400.0;

pub(crate) fn clamp_used_layout_coordinate(value: LayoutLength) -> LayoutLength {
    if value.points().is_nan() {
        layout_pt(0.0)
    } else {
        layout_pt(
            value
                .points()
                .clamp(-MAX_USED_LAYOUT_LENGTH_PT, MAX_USED_LAYOUT_LENGTH_PT),
        )
    }
}

pub(crate) fn clamp_used_layout_length(value: LayoutLength) -> LayoutLength {
    layout_pt(clamp_used_layout_coordinate(value).points().max(0.0))
}
