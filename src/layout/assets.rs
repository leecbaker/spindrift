use super::paint_helpers::{
    FixedGradientStop, angled_gradient_line, fixed_gradient_is_hard_stop, fixed_gradient_stops,
    gradient_axis_position, linear_gradient_can_paint_as_vector, rounded_background_clip_for_box,
};
use super::*;

mod split_1;
mod split_2;
pub(in crate::layout) use self::split_2::*;
mod split_3;
pub(in crate::layout) use self::split_3::*;
