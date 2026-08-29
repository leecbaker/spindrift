//! Table sizing implementation façade.

use super::*;

mod intrinsic;
mod rows;
mod tracks;
mod wrapper;

pub(in crate::layout::table) use self::intrinsic::*;
pub(in crate::layout) use self::rows::*;
pub(in crate::layout::table) use self::tracks::*;
pub(in crate::layout) use self::wrapper::*;
