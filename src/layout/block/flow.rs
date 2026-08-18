use super::super::*;

mod block_layout;
pub(in crate::layout) mod children;
mod fragmentation;
mod geometry;
mod intrinsic;
mod phase;
#[cfg(test)]
mod tests;

pub(in crate::layout) use self::block_layout::{
    continuous_fragmentainer_paint_slices, suppress_fragmented_box_edges,
};
pub(in crate::layout) use self::children::state::{
    BlockFlowChildTraversalState, DiscardRegionLimit,
};
pub(in crate::layout) use self::fragmentation::*;
pub(in crate::layout) use self::geometry::*;
pub(in crate::layout) use self::phase::*;
pub(in crate::layout) use crate::layout::flow_helpers::height_behaves_as_auto_for_margin_collapse;
