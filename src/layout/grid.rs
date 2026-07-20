use super::*;

mod children;
mod contributions;
mod fragmentation;
mod intrinsic;
mod lanes;
mod line_resolution;
mod replay;
mod resolved;
mod static_position;
mod taffy_adapter;

use children::*;
use contributions::*;
use fragmentation::*;
use intrinsic::*;
use line_resolution::*;
pub(in crate::layout) use resolved::ResolvedSubgridContext;
use resolved::{ResolvedSubgridAxis, ResolvedSubgridPlacement};
use static_position::*;
pub(in crate::layout) use static_position::{
    GridPositioningScope, grid_abspos_late_horizontal_static_position,
    grid_abspos_late_vertical_static_start,
};
use taffy_adapter::*;

/// Provenance for a grid available-space axis that is definite enough to
/// resolve CSS percentages.
///
/// Grid sizing often has a numeric available inline size while the block size
/// remains indefinite during intrinsic row sizing. Keeping this typed basis
/// separate from raw geometry prevents percentage heights from accidentally
/// falling back to unrelated constraints:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GridAvailableSizeSource {
    ContainerInlineSize,
    ContainerBlockSize,
}

pub(in crate::layout) type GridPercentageBasis =
    PercentageBasis<ContentBoxLength, GridAvailableSizeSource>;

pub(in crate::layout) fn grid_percentage_basis(
    value: Option<ContentBoxLength>,
    source: GridAvailableSizeSource,
) -> GridPercentageBasis {
    value
        .map(|value| PercentageBasis::definite_from(value, source))
        .unwrap_or_else(PercentageBasis::indefinite)
}

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
