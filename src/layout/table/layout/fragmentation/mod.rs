//! Fragmented table-layout orchestration.

use super::*;

mod body;
mod breaks;
mod empty;
mod entry;
mod intrinsic;
mod model;
mod projection;

pub(in crate::layout::table) use self::breaks::*;
pub(in crate::layout::table) use self::model::*;
pub(in crate::layout::table) use self::projection::*;

#[cfg(test)]
mod support;
