mod exclusions;
mod fragments;
mod layout;
mod model;
mod placement;
mod sizing;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::layout) use self::exclusions::ClearedFloatOuterBlockEnd;
pub(in crate::layout) use self::exclusions::{
    BlockClearance, BlockClearanceRequest, BlockStartMarginArrangement, FLOAT_EPSILON,
    HypotheticalClearBorderEdge, ParentStartClearanceHypothesis,
};
pub(in crate::layout) use self::model::*;
pub(in crate::layout) use self::placement::{
    float_avoiding_auto_border_box_width, vertical_physical_inline_span,
};
pub(in crate::layout) use self::sizing::{
    AutoFloatMeasurementKey, freeze_float_replay_height, freeze_float_replay_width,
};
