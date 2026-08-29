use std::rc::Rc;

use super::super::*;
use super::InlineLineSequence;
use super::graph::{InlineLineFragment, MeasuredInlineItem, measured_inline_items};
use super::items::InlineLineSequenceSlice;
use crate::layout::assets::{
    ReplacedObjectOverflow, apply_object_fit, native_generated_gradient_primitive,
    raster_image_sampling, replaced_content_contour, svg_replaced_group_with_overflow_clip,
};
use crate::layout::text_paint::{
    TextDecorationLineGeometry, TextDecorationLineGlyphCoverage, TextDecorationLineGlyphSequence,
    TextDecorationLineKind, TextDecorationOriginFragmentGeometry, TextDecorationOriginLineGeometry,
    TextDecorationStrokeAxis, TextInlineSpan, VerticalInlineAxis,
    positioned_rendered_runs_for_writing_mode, text_decoration_positioned_glyphs,
    text_decoration_skip_self_suppresses,
};

mod atom_painting;
mod box_edges;
mod horizontal_geometry;
mod line_painting;
mod line_preparation;
mod shaping_boundaries;
mod text_group_preparation;

pub(in crate::layout) use self::box_edges::*;
pub(in crate::layout) use self::horizontal_geometry::*;
pub(in crate::layout) use self::shaping_boundaries::*;
