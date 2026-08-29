use super::*;

mod background_geometry;
pub(in crate::layout) use self::background_geometry::*;
mod border_shape;
pub(in crate::layout) use self::border_shape::*;
mod box_decoration;
pub(in crate::layout) use self::box_decoration::*;
mod box_shadow;
pub(in crate::layout) use self::box_shadow::*;
mod linear_gradient;
pub(in crate::layout) use self::linear_gradient::*;
mod rounded_border;
pub(in crate::layout) use self::rounded_border::*;
// This façade preserves the existing path-builder module path while the
// generic corner geometry itself lives alongside its other paint consumers.
pub(crate) use super::corners::shaped_rect_path_commands;
