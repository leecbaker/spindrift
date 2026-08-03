//! Retained CSS paint records and their CSS painting-order tree.
//!
//! This module is an internal namespace.  Consumers import records from the
//! domain that owns them rather than through a flat paint prelude.

pub(crate) mod annotations;
pub(crate) mod display_list;
pub(crate) mod effects;
pub(crate) mod fragments;
pub(crate) mod geometry;
pub(crate) mod images;
pub(crate) mod page;
pub(crate) mod paths;
pub(crate) mod patterns;
pub(crate) mod shapes;
pub(crate) mod stacking;
pub(crate) mod text;
