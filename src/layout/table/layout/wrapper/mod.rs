//! Table-wrapper captions, geometry, and fragment timeline.

use super::*;

mod captions;
mod geometry;
mod timeline;

pub(in crate::layout::table) use self::captions::*;
pub(in crate::layout::table) use self::geometry::*;
pub(in crate::layout::table) use self::timeline::*;
