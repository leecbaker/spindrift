//! Table source-structure implementation façade.

use super::*;

mod cell_content;
mod metrics;
mod paint;
mod tree;

pub(in crate::layout::table) use self::cell_content::*;
pub(in crate::layout::table) use self::metrics::*;
pub(in crate::layout::table) use self::paint::*;
pub(in crate::layout) use self::tree::*;
