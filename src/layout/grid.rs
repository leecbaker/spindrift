use super::*;

mod baseline;
mod children;
mod container;
mod contributions;
mod fragmentation;
mod gap_decorations;
mod intrinsic;
mod item_adjustment;
mod lanes;
mod line_resolution;
mod model;
mod replay;
mod resolved;
mod sizing;
mod static_position;
mod taffy_adapter;

use baseline::{GridBaselineResolution, GridBaselineSet};
use children::*;
use contributions::*;
use fragmentation::*;
use intrinsic::*;
use item_adjustment::{
    apply_grid_replaced_item_size_corrections, grid_subject_self_end_side,
    grid_subject_self_start_side, resolve_grid_item_final_percentage_size,
};
use line_resolution::*;
pub(in crate::layout) use model::GridAxisTopology;
use model::{
    GridItemArea, GridItemLayout, GridItemReplayDimensions, GridLayout, GridLayoutPurpose,
};
pub(in crate::layout) use resolved::ResolvedSubgridContext;
use resolved::{ResolvedSubgridAxis, ResolvedSubgridPlacement};
use sizing::{GridFrozenTrackTopology, GridLayoutPassConfig};
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
    /// A grid item's definite inline containing-block size, derived from its
    /// resolved grid area.
    GridItemContainingBlockInline,
    /// A grid item's definite block containing-block size, derived from its
    /// resolved grid area.
    GridItemContainingBlockBlock,
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

/// Percentage bases for authored Grid track lists after projecting their
/// logical axes onto the physical Taffy grid axes.
///
/// Track breadth percentages resolve against their own grid axis, unlike box
/// edges which always use the logical inline axis. Keeping this projection
/// distinct prevents a vertical-writing row track from accidentally using the
/// container height, or an indefinite block axis from becoming definite.
/// <https://drafts.csswg.org/css-grid-2/#track-percentages>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::grid) struct GridTrackPercentageBases {
    columns: GridPercentageBasis,
    rows: GridPercentageBasis,
}

impl GridTrackPercentageBases {
    pub(in crate::layout::grid) fn from_grid_content_box(
        style: &ComputedStyle,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
    ) -> Self {
        let swaps_physical_axes =
            WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes();
        let physical_width = grid_percentage_basis(
            Some(width.content_box_length()),
            if swaps_physical_axes {
                GridAvailableSizeSource::ContainerBlockSize
            } else {
                GridAvailableSizeSource::ContainerInlineSize
            },
        );
        let physical_height = grid_percentage_basis(
            height.map(PhysicalContentHeight::content_box_length),
            if swaps_physical_axes {
                GridAvailableSizeSource::ContainerInlineSize
            } else {
                GridAvailableSizeSource::ContainerBlockSize
            },
        );
        if swaps_physical_axes {
            Self {
                columns: physical_height,
                rows: physical_width,
            }
        } else {
            Self {
                columns: physical_width,
                rows: physical_height,
            }
        }
    }

    pub(in crate::layout::grid) fn for_axis(self, axis: GridAxis) -> GridPercentageBasis {
        match axis {
            GridAxis::Column => self.columns,
            GridAxis::Row => self.rows,
        }
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

    #[test]
    fn track_percentage_bases_follow_logical_grid_axes() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        let bases = GridTrackPercentageBases::from_grid_content_box(
            &style,
            PhysicalContentWidth::new(content_box_pt(80.0)),
            Some(PhysicalContentHeight::new(content_box_pt(100.0))),
        );

        assert_eq!(bases.for_axis(GridAxis::Column).points(), Some(100.0));
        assert_eq!(bases.for_axis(GridAxis::Row).points(), Some(80.0));
    }
}
