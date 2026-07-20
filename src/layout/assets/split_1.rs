use super::*;

mod replaced_layout;
pub(in crate::layout) use replaced_layout::{BorderPaint, apply_object_fit, svg_replaced_group};
mod split_2;

/// The physical containing space supplied while measuring an automatic
/// positioned block axis. Keeping both axes together prevents a vertical
/// writing-mode measurement from accidentally replacing its logical inline
/// containing size with the unbounded horizontal block-axis sentinel.
/// <https://www.w3.org/TR/css-position-3/#abspos-layout>
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedAutoBlockMeasurementSpace {
    pub(in crate::layout) content_width: PhysicalContentWidth,
    pub(in crate::layout) available_physical_height: PhysicalContentHeight,
}
