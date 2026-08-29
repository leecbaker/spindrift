//! Table layout implementation façade.

use super::*;

mod cells;
mod fragmentation;
mod grid;
mod paint;
mod wrapper;

pub(in crate::layout) use self::cells::*;
pub(in crate::layout::table) use self::fragmentation::*;
pub(in crate::layout::table) use self::paint::*;
pub(in crate::layout::table) use self::wrapper::*;
pub(in crate::layout::table) use super::CollapsedTableGeometry;
