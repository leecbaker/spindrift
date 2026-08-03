//! OpenType glyph extraction for non-outline and supplemental paint paths.

mod color;
mod outline;
mod raster;

pub(in crate::text) use color::*;
pub(in crate::text) use outline::*;
pub(in crate::text) use raster::*;
