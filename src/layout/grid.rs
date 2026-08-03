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
pub(in crate::layout) use static_position::GridPositioningScope;
use static_position::*;
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

pub(in crate::layout) type GridLogicalInlinePercentageBasis =
    LogicalInlinePercentageBasis<GridAvailableSizeSource>;

pub(in crate::layout) fn grid_percentage_basis(
    value: Option<ContentBoxLength>,
    source: GridAvailableSizeSource,
) -> GridPercentageBasis {
    value
        .map(|value| PercentageBasis::definite_from(value, source))
        .unwrap_or_else(PercentageBasis::indefinite)
}

/// Definite percentage bases for Grid's physical Taffy axes.
///
/// Grid maps its logical axes onto Taffy's physical columns and rows at one
/// adapter boundary. CSS Box edge percentages remain tied to the container's
/// logical inline axis, so callers must select that axis explicitly rather
/// than treating physical width as a universal percentage basis.
/// <https://www.w3.org/TR/css-box-3/#margin-physical>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy)]
pub(super) struct GridPhysicalAvailableSpace {
    pub(super) width_basis: GridPercentageBasis,
    pub(super) height_basis: GridPercentageBasis,
}

impl GridPhysicalAvailableSpace {
    pub(super) fn logical_inline_basis(
        self,
        style: &ComputedStyle,
    ) -> GridLogicalInlinePercentageBasis {
        let physical_basis = if WritingModeAxes::new(style.writing_mode, style.used_direction())
            .swaps_physical_axes()
        {
            self.height_basis
        } else {
            self.width_basis
        };
        physical_basis.map_value(LogicalInlineContentSize::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_inline_basis_projects_physical_grid_space_once() {
        let available = GridPhysicalAvailableSpace {
            width_basis: grid_percentage_basis(
                Some(content_box_pt(80.0)),
                GridAvailableSizeSource::ContainerInlineSize,
            ),
            height_basis: grid_percentage_basis(
                Some(content_box_pt(100.0)),
                GridAvailableSizeSource::ContainerBlockSize,
            ),
        };
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;

        let inline_basis: GridLogicalInlinePercentageBasis = available.logical_inline_basis(&style);
        assert_eq!(inline_basis.points(), Some(100.0));
    }
}

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
