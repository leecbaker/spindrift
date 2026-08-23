use super::lanes::{GridLanesItemPlacement, grid_lanes_item_placement};
use super::*;

/// An intrinsic descendant contribution projected into an enclosing parent
/// grid's shared axis.
///
/// A subgrid does not contribute an independent track size in an inherited
/// axis. Its normal-flow descendants instead contribute at their projected
/// parent span during the parent track-sizing pass.
/// <https://drafts.csswg.org/css-grid-2/#subgrid-contributions>
#[derive(Debug, Clone)]
pub(super) struct SubgridContribution {
    pub(super) area: GridItemArea,
    pub(super) estimate: GridItemEstimate,
    pub(super) contributes_columns: bool,
    pub(super) contributes_rows: bool,
    /// Box-model and inherited-gutter space at the two logical ends of the
    /// projected span. These are deliberately part of the contribution key:
    /// two otherwise-identical spans can impose different spanning
    /// constraints when they arrive through different subgrid edges.
    pub(super) column_edges: ContributionEdges,
    pub(super) row_edges: ContributionEdges,
    /// The estimate remains in this writing mode's logical coordinates until
    /// the Taffy adapter projects it to physical x/y coordinates.
    pub(super) swaps_physical_axes: bool,
    /// The logical axes in which the projected contribution edges were
    /// accumulated. The final proxy leaf maps those edges to physical Taffy
    /// margins at the sole logical-to-physical boundary.
    pub(super) axes: WritingModeAxes,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct ContributionEdges {
    pub(super) start: LayoutLength,
    pub(super) end: LayoutLength,
}

impl ContributionEdges {
    fn total(self) -> LayoutLength {
        layout_pt(self.start.points() + self.end.points())
    }

    fn add_assign(&mut self, other: Self) {
        self.start = layout_pt(self.start.points() + other.start.points());
        self.end = layout_pt(self.end.points() + other.end.points());
    }
}

impl SubgridContribution {
    /// Whether this proxy can affect an inherited parent track.
    ///
    /// A zero contribution is not a Grid item and must not trigger the
    /// sizing replay that introduces a proxy leaf into placement. This is
    /// especially important for a standalone axis whose descendant has no
    /// intrinsic size in the other, inherited axis.
    fn affects_inherited_track_sizing(&self) -> bool {
        const EPSILON: f32 = 0.01;
        let column_size = self
            .estimate
            .metrics
            .width
            .points()
            .max(self.estimate.metrics.min_width.points())
            .max(self.estimate.metrics.content_width.points())
            + self.column_edges.total().points();
        let row_size = self
            .estimate
            .metrics
            .height
            .points()
            .max(self.estimate.metrics.min_height.points())
            .max(self.estimate.metrics.content_height.points())
            + self.row_edges.total().points();
        (self.contributes_columns && column_size > EPSILON)
            || (self.contributes_rows && row_size > EPSILON)
    }

    fn merge(&mut self, other: GridItemEstimate) {
        self.estimate.metrics.width = self.estimate.metrics.width.max(other.metrics.width);
        self.estimate.metrics.height = self.estimate.metrics.height.max(other.metrics.height);
        self.estimate.metrics.min_width =
            self.estimate.metrics.min_width.max(other.metrics.min_width);
        self.estimate.metrics.min_height = self
            .estimate
            .metrics
            .min_height
            .max(other.metrics.min_height);
        self.estimate.metrics.content_width = self
            .estimate
            .metrics
            .content_width
            .max(other.metrics.content_width);
        self.estimate.metrics.content_height = self
            .estimate
            .metrics
            .content_height
            .max(other.metrics.content_height);
    }

    /// Remove non-inherited axes before passing a projected descendant to the
    /// parent track-sizing adapter. Its subgrid edge adjustments remain
    /// physical Taffy margins, so the adapter can reduce the item's available
    /// track area before measuring it.
    pub(super) fn sizing_estimate(&self) -> GridItemEstimate {
        let mut estimate = self.estimate;
        if !self.contributes_columns {
            estimate.metrics.width = content_box_pt(0.0);
            estimate.metrics.min_width = content_box_pt(0.0);
            estimate.metrics.content_width = content_box_pt(0.0);
        }
        if !self.contributes_rows {
            estimate.metrics.height = content_box_pt(0.0);
            estimate.metrics.min_height = content_box_pt(0.0);
            estimate.metrics.content_height = content_box_pt(0.0);
        }
        estimate.swaps_physical_axes = self.swaps_physical_axes;
        estimate
    }

    /// Project logical inherited subgrid edge adjustments into the physical
    /// margin rectangle consumed by Taffy's Grid track-sizing algorithm.
    /// CSS Grid treats these values as an extra layer of margin on the
    /// projected descendant contribution:
    /// <https://drafts.csswg.org/css-grid-2/#subgrid-item-contribution>.
    pub(super) fn taffy_margin(&self) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
        let mut margin = taffy_layout::Rect {
            top: taffy_layout::LengthPercentageAuto::length(0.0),
            right: taffy_layout::LengthPercentageAuto::length(0.0),
            bottom: taffy_layout::LengthPercentageAuto::length(0.0),
            left: taffy_layout::LengthPercentageAuto::length(0.0),
        };
        let mut set = |side: PhysicalSide, value: LayoutLength| {
            let value = taffy_layout::LengthPercentageAuto::length(value.points());
            match side {
                PhysicalSide::Top => margin.top = value,
                PhysicalSide::Right => margin.right = value,
                PhysicalSide::Bottom => margin.bottom = value,
                PhysicalSide::Left => margin.left = value,
            }
        };
        if self.contributes_columns {
            set(
                self.axes.physical_side(LogicalSide::InlineStart),
                self.column_edges.start,
            );
            set(
                self.axes.physical_side(LogicalSide::InlineEnd),
                self.column_edges.end,
            );
        }
        if self.contributes_rows {
            set(
                self.axes.physical_side(LogicalSide::BlockStart),
                self.row_edges.start,
            );
            set(
                self.axes.physical_side(LogicalSide::BlockEnd),
                self.row_edges.end,
            );
        }
        margin
    }
}

#[derive(Debug, Clone, Copy)]
struct ContributionProjection {
    /// The enclosing subgrid's area already projected into the root grid.
    area: GridItemArea,
    /// The root grid's logical axes. Every nested subgrid's physical box
    /// edges are normalized into these axes before they are accumulated.
    axes: WritingModeAxes,
    contributes_columns: bool,
    contributes_rows: bool,
    column_edges: ContributionEdges,
    row_edges: ContributionEdges,
}

/// An automatic Grid Lanes subgrid has no known final parent range while
/// intrinsic tracks are sized. Its descendants therefore contribute through a
/// virtual copy of every parent span the subgrid could occupy.
/// <https://drafts.csswg.org/css-grid-3/#subgrid-item-contributions>
fn grid_lanes_subgrid_contribution_areas(
    layout: &GridLayout,
    fallback: GridItemArea,
    placement: Option<GridLanesItemPlacement>,
) -> Vec<GridItemArea> {
    let Some(GridLanesItemPlacement::Automatic { grid_axis, span }) = placement else {
        return vec![fallback];
    };
    let track_count = layout.physical_track_sizes(grid_axis).len();
    let span = span.min(track_count);
    if span == 0 {
        return vec![fallback];
    }
    (0..=track_count - span)
        .map(|start| {
            let start = u16::try_from(start.saturating_add(1)).unwrap_or(u16::MAX);
            let end = start.saturating_add(u16::try_from(span).unwrap_or(u16::MAX));
            match grid_axis {
                GridAxis::Column => GridItemArea {
                    column_start: start,
                    column_end: end,
                    ..fallback
                },
                GridAxis::Row => GridItemArea {
                    row_start: start,
                    row_end: end,
                    ..fallback
                },
            }
        })
        .collect()
}

impl ContributionProjection {
    fn project(self, local: GridItemArea) -> GridItemArea {
        GridItemArea {
            row_start: self.area.row_start + local.row_start - 1,
            row_end: self.area.row_start + local.row_end - 1,
            column_start: self.area.column_start + local.column_start - 1,
            column_end: self.area.column_start + local.column_end - 1,
        }
    }

    fn nested(
        self,
        local: GridItemArea,
        nested_context: &ResolvedSubgridContext,
        style: &ComputedStyle,
    ) -> Self {
        let mut column_edges = self.column_edges;
        let mut row_edges = self.row_edges;
        let local_edges = subgrid_box_edges(style, self.axes);
        if nested_context.columns.is_some() {
            column_edges.add_assign(local_edges.0);
        }
        if nested_context.rows.is_some() {
            row_edges.add_assign(local_edges.1);
        }
        Self {
            area: self.project(local),
            axes: self.axes,
            contributes_columns: self.contributes_columns && nested_context.columns.is_some(),
            contributes_rows: self.contributes_rows && nested_context.rows.is_some(),
            column_edges,
            row_edges,
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Collect normal-flow descendants of explicitly placed subgrids.
    ///
    /// The preliminary parent pass supplies the used inherited span. Each
    /// child subgrid is then measured as an intrinsic probe using that shared
    /// geometry. Nested subgrids recurse with their areas projected into the
    /// original parent coordinate system, so only the root parent ever sees a
    /// proxy leaf. This follows the descendant contribution rule in CSS Grid
    /// Level 2 §9.2.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect_subgrid_contributions(
        &mut self,
        parent_style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        preliminary: &GridLayout,
    ) -> Vec<SubgridContribution> {
        let mut contributions = Vec::new();
        for (child, parent_item) in children.iter().zip(&preliminary.items) {
            let Some(parent_area) = parent_item.area else {
                continue;
            };
            if child.element_parts().is_some_and(|(element, _, _)| {
                layout_containment_applies_to_element(element, &child.style)
                    || paint_containment_applies_to_element(element, &child.style)
            }) {
                continue;
            }
            let placement = grid_lanes_item_placement(parent_style, child);
            let axes =
                WritingModeAxes::new(parent_style.writing_mode, parent_style.used_direction());
            let (column_edges, row_edges) = subgrid_box_edges(&child.style, axes);
            for parent_area in
                grid_lanes_subgrid_contribution_areas(preliminary, parent_area, placement)
            {
                let Some(context) = ResolvedSubgridContext::from_parent(
                    parent_style,
                    preliminary,
                    &child.style,
                    parent_area,
                    placement,
                ) else {
                    continue;
                };
                self.collect_subgrid_contributions_from_context(
                    child,
                    context,
                    ContributionProjection {
                        area: parent_area,
                        axes,
                        contributes_columns: matches!(
                            child.style.grid_template_columns,
                            css::GridTrackList::Subgrid { .. }
                        ),
                        contributes_rows: matches!(
                            child.style.grid_template_rows,
                            css::GridTrackList::Subgrid { .. }
                        ),
                        column_edges,
                        row_edges,
                    },
                    stylesheets,
                    parent_item.replay_dimensions(),
                    &mut contributions,
                );
            }
        }
        merge_subgrid_contributions(contributions)
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_subgrid_contributions_from_context(
        &mut self,
        child: &GridChild<'_>,
        context: ResolvedSubgridContext,
        projection: ContributionProjection,
        stylesheets: &Stylesheets<'_>,
        child_dimensions: GridItemReplayDimensions,
        contributions: &mut Vec<SubgridContribution>,
    ) {
        if !projection.contributes_columns && !projection.contributes_rows {
            return;
        }
        let Some((_, _, Some(child_boxes))) = child.element_parts() else {
            return;
        };
        let (grandchildren, _) = grid_child_lists_from_boxes(child_boxes);
        let grandchildren = self.prepare_grid_children(grandchildren);
        let child_width = child_dimensions
            .physical_content_width_for_replay(&child.style)
            .points();
        let child_height = child_dimensions
            .physical_content_height_for_replay(&child.style)
            .points();
        let column_line_count = context
            .columns
            .as_ref()
            .map_or(1, |axis| axis.track_count() as u16 + 1);
        let row_line_count = context
            .rows
            .as_ref()
            .map_or(1, |axis| axis.track_count() as u16 + 1);
        let child_layout = self.with_resolved_subgrid_context(context, |layout| {
            layout.compute_grid_layout_for_subgrid_contribution_probe(
                &child.style,
                &grandchildren,
                stylesheets,
                child_dimensions,
                GridLayoutPurpose::IntrinsicProbe,
            )
        });
        let Some(child_layout) = child_layout else {
            return;
        };
        for (grandchild, item) in grandchildren.iter().zip(&child_layout.items) {
            let Some(area) = item.area else {
                continue;
            };
            // This milestone deliberately leaves automatic placement in a
            // shared axis for a later pass. It must not synthesize an implicit
            // parent track while measuring explicit descendants.
            if (projection.contributes_columns && area.column_end > column_line_count)
                || (projection.contributes_rows && area.row_end > row_line_count)
            {
                continue;
            }
            let projected = projection.project(area);
            if grandchild.element_parts().is_some_and(|(element, _, _)| {
                layout_containment_applies_to_element(element, &grandchild.style)
                    || paint_containment_applies_to_element(element, &grandchild.style)
            }) {
                continue;
            }
            if let Some(nested_context) = ResolvedSubgridContext::from_parent(
                &child.style,
                &child_layout,
                &grandchild.style,
                area,
                item.grid_lanes_placement(),
            ) {
                let nested_projection = projection.nested(area, &nested_context, &grandchild.style);
                // A nested grid which is standalone in one root-shared axis
                // contributes its own intrinsic size in that axis. Only the
                // axes it also inherits are replaced by recursively projected
                // descendants.
                let standalone_columns =
                    projection.contributes_columns && nested_context.columns.is_none();
                let standalone_rows = projection.contributes_rows && nested_context.rows.is_none();
                if standalone_columns || standalone_rows {
                    let estimate = self.estimate_grid_item_size_for_parent_track_sizing(
                        grandchild,
                        stylesheets,
                        child_width,
                        grid_percentage_basis(
                            Some(content_box_pt(child_width)),
                            GridAvailableSizeSource::ContainerInlineSize,
                        ),
                        grid_percentage_basis(
                            Some(content_box_pt(child_height)),
                            GridAvailableSizeSource::ContainerBlockSize,
                        ),
                    );
                    contributions.push(SubgridContribution {
                        area: projected,
                        estimate,
                        contributes_columns: standalone_columns,
                        contributes_rows: standalone_rows,
                        column_edges: projection.column_edges,
                        row_edges: projection.row_edges,
                        swaps_physical_axes: estimate.swaps_physical_axes,
                        axes: projection.axes,
                    });
                }
                self.collect_subgrid_contributions_from_context(
                    grandchild,
                    nested_context,
                    nested_projection,
                    stylesheets,
                    item.replay_dimensions(),
                    contributions,
                );
                continue;
            }
            let estimate = self.estimate_grid_item_size_for_parent_track_sizing(
                grandchild,
                stylesheets,
                child_width,
                grid_percentage_basis(
                    Some(content_box_pt(child_width)),
                    GridAvailableSizeSource::ContainerInlineSize,
                ),
                grid_percentage_basis(
                    Some(content_box_pt(child_height)),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
            );
            contributions.push(SubgridContribution {
                area: projected,
                estimate,
                contributes_columns: projection.contributes_columns,
                contributes_rows: projection.contributes_rows,
                column_edges: projection.column_edges,
                row_edges: projection.row_edges,
                swaps_physical_axes: estimate.swaps_physical_axes,
                axes: projection.axes,
            });
        }
    }
}

/// Convert a subgrid's physical box edges to the root grid's logical
/// column/row axes.
/// The values are applied only to the outer ends of a descendant projection;
/// inner tracks continue to use the parent gutter geometry.
fn subgrid_box_edges(
    style: &ComputedStyle,
    root_axes: WritingModeAxes,
) -> (ContributionEdges, ContributionEdges) {
    let borders = used_border_widths(style);
    let edge = |side| {
        let physical = |edges: css::Edges| match side {
            PhysicalSide::Top => edges.top,
            PhysicalSide::Right => edges.right,
            PhysicalSide::Bottom => edges.bottom,
            PhysicalSide::Left => edges.left,
        };
        layout_pt(physical(style.margin) + physical(style.padding) + physical(borders))
    };
    let inline = ContributionEdges {
        start: edge(root_axes.physical_side(LogicalSide::InlineStart)),
        end: edge(root_axes.physical_side(LogicalSide::InlineEnd)),
    };
    let block = ContributionEdges {
        start: edge(root_axes.physical_side(LogicalSide::BlockStart)),
        end: edge(root_axes.physical_side(LogicalSide::BlockEnd)),
    };
    (inline, block)
}

fn merge_subgrid_contributions(
    contributions: Vec<SubgridContribution>,
) -> Vec<SubgridContribution> {
    let mut merged = Vec::<SubgridContribution>::new();
    for contribution in contributions {
        if let Some(existing) = merged.iter_mut().find(|existing| {
            existing.area.row_start == contribution.area.row_start
                && existing.area.row_end == contribution.area.row_end
                && existing.area.column_start == contribution.area.column_start
                && existing.area.column_end == contribution.area.column_end
                && existing.contributes_columns == contribution.contributes_columns
                && existing.contributes_rows == contribution.contributes_rows
                && existing.column_edges == contribution.column_edges
                && existing.row_edges == contribution.row_edges
                && existing.swaps_physical_axes == contribution.swaps_physical_axes
                && existing.axes == contribution.axes
        }) {
            existing.merge(contribution.estimate);
        } else {
            merged.push(contribution);
        }
    }
    merged
        .into_iter()
        .filter(SubgridContribution::affects_inherited_track_sizing)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_equal_projected_spans_by_largest_intrinsic_contribution() {
        let area = GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: 1,
            column_end: 2,
        };
        let merged = merge_subgrid_contributions(vec![
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(10.0, 5.0),
                contributes_columns: true,
                contributes_rows: false,
                column_edges: ContributionEdges::default(),
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
                axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
            },
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(20.0, 3.0),
                contributes_columns: true,
                contributes_rows: false,
                column_edges: ContributionEdges::default(),
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
                axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
            },
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].estimate.width.points(), 20.0);
    }

    #[test]
    fn preserves_distinct_outer_edge_constraints_when_merging() {
        let area = GridItemArea {
            row_start: 2,
            row_end: 4,
            column_start: 3,
            column_end: 5,
        };
        let merged = merge_subgrid_contributions(vec![
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(10.0, 10.0),
                contributes_columns: true,
                contributes_rows: true,
                column_edges: ContributionEdges {
                    start: layout_pt(2.0),
                    end: layout_pt(0.0),
                },
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
                axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
            },
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(20.0, 20.0),
                contributes_columns: true,
                contributes_rows: true,
                column_edges: ContributionEdges::default(),
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
                axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
            },
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn discards_a_zero_contribution_in_the_inherited_axis() {
        let area = GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: 1,
            column_end: 2,
        };
        let merged = merge_subgrid_contributions(vec![SubgridContribution {
            area,
            // A standalone inline contribution does not size an inherited
            // row; retaining it as a proxy would incorrectly affect parent
            // placement without changing track sizing.
            estimate: GridItemEstimate::fixed(100.0, 0.0),
            contributes_columns: false,
            contributes_rows: true,
            column_edges: ContributionEdges::default(),
            row_edges: ContributionEdges::default(),
            swaps_physical_axes: false,
            axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
        }]);
        assert!(merged.is_empty());
    }

    #[test]
    fn projects_nested_areas_directly_into_the_root_grid() {
        let root = ContributionProjection {
            area: GridItemArea {
                row_start: 3,
                row_end: 8,
                column_start: 2,
                column_end: 7,
            },
            axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr),
            contributes_columns: true,
            contributes_rows: true,
            column_edges: ContributionEdges::default(),
            row_edges: ContributionEdges::default(),
        };
        let nested = root.project(GridItemArea {
            row_start: 2,
            row_end: 4,
            column_start: 3,
            column_end: 5,
        });
        let descendant = ContributionProjection {
            area: nested,
            ..root
        }
        .project(GridItemArea {
            row_start: 2,
            row_end: 3,
            column_start: 1,
            column_end: 2,
        });
        assert_eq!(descendant.row_start, 5);
        assert_eq!(descendant.row_end, 6);
        assert_eq!(descendant.column_start, 4);
        assert_eq!(descendant.column_end, 5);
    }

    #[test]
    fn accumulates_nested_edges_in_the_root_axes_without_double_counting() {
        // The outer RTL subgrid's physical left edge is its inline end, but
        // the root LTR grid owns physical-left as its inline start. Normalize
        // each nested layer before accumulating so the two physical-left
        // edges contribute to the same inherited parent-track edge.
        let root_axes = WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Ltr);
        let mut outer = ContributionEdges {
            start: layout_pt(11.0),
            end: layout_pt(1.0),
        };
        let nested = ContributionEdges {
            start: layout_pt(1.0),
            end: layout_pt(1.0),
        };
        outer.add_assign(nested);
        let contribution = SubgridContribution {
            area: GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            estimate: GridItemEstimate::fixed(10.0, 10.0),
            contributes_columns: true,
            contributes_rows: false,
            column_edges: outer,
            row_edges: ContributionEdges::default(),
            swaps_physical_axes: false,
            axes: root_axes,
        };
        let margin = contribution.taffy_margin();
        assert_eq!(
            margin.left,
            taffy_layout::LengthPercentageAuto::length(12.0)
        );
        assert_eq!(
            margin.right,
            taffy_layout::LengthPercentageAuto::length(2.0)
        );
    }

    #[test]
    fn projects_contribution_edges_to_rtl_physical_margins() {
        let contribution = SubgridContribution {
            area: GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            estimate: GridItemEstimate::fixed(10.0, 10.0),
            contributes_columns: true,
            contributes_rows: true,
            column_edges: ContributionEdges {
                start: layout_pt(3.0),
                end: layout_pt(5.0),
            },
            row_edges: ContributionEdges {
                start: layout_pt(7.0),
                end: layout_pt(11.0),
            },
            swaps_physical_axes: false,
            axes: WritingModeAxes::new(css::WritingMode::HorizontalTb, css::Direction::Rtl),
        };
        let margin = contribution.taffy_margin();
        assert_eq!(margin.top, taffy_layout::LengthPercentageAuto::length(7.0));
        assert_eq!(
            margin.right,
            taffy_layout::LengthPercentageAuto::length(3.0)
        );
        assert_eq!(
            margin.bottom,
            taffy_layout::LengthPercentageAuto::length(11.0)
        );
        assert_eq!(margin.left, taffy_layout::LengthPercentageAuto::length(5.0));
    }

    #[test]
    fn projects_contribution_edges_to_vertical_rl_physical_margins() {
        let contribution = SubgridContribution {
            area: GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            estimate: GridItemEstimate::fixed(10.0, 10.0),
            contributes_columns: true,
            contributes_rows: true,
            column_edges: ContributionEdges {
                start: layout_pt(3.0),
                end: layout_pt(5.0),
            },
            row_edges: ContributionEdges {
                start: layout_pt(7.0),
                end: layout_pt(11.0),
            },
            swaps_physical_axes: true,
            axes: WritingModeAxes::new(css::WritingMode::VerticalRl, css::Direction::Ltr),
        };
        let margin = contribution.taffy_margin();
        assert_eq!(margin.top, taffy_layout::LengthPercentageAuto::length(3.0));
        assert_eq!(
            margin.right,
            taffy_layout::LengthPercentageAuto::length(7.0)
        );
        assert_eq!(
            margin.bottom,
            taffy_layout::LengthPercentageAuto::length(5.0)
        );
        assert_eq!(
            margin.left,
            taffy_layout::LengthPercentageAuto::length(11.0)
        );
    }
}
