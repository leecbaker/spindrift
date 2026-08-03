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
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct ContributionEdges {
    pub(super) start: f32,
    pub(super) end: f32,
}

impl ContributionEdges {
    fn total(self) -> f32 {
        (self.start + self.end).max(0.0)
    }

    fn add_assign(&mut self, other: Self) {
        self.start += other.start;
        self.end += other.end;
    }
}

impl SubgridContribution {
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

    /// Produce the logical intrinsic contribution consumed by the parent
    /// track-sizing adapter. A subgrid's box edges are included exactly once,
    /// at the outer edges of the projected descendant span.
    pub(super) fn adjusted_estimate(&self) -> GridItemEstimate {
        let mut estimate = self.estimate;
        if self.contributes_columns {
            let adjustment = self.column_edges.total();
            estimate.metrics.width += content_box_pt(adjustment);
            estimate.metrics.min_width += content_box_pt(adjustment);
            estimate.metrics.content_width += content_box_pt(adjustment);
        } else {
            estimate.metrics.width = content_box_pt(0.0);
            estimate.metrics.min_width = content_box_pt(0.0);
            estimate.metrics.content_width = content_box_pt(0.0);
        }
        if self.contributes_rows {
            let adjustment = self.row_edges.total();
            estimate.metrics.height += content_box_pt(adjustment);
            estimate.metrics.min_height += content_box_pt(adjustment);
            estimate.metrics.content_height += content_box_pt(adjustment);
        } else {
            estimate.metrics.height = content_box_pt(0.0);
            estimate.metrics.min_height = content_box_pt(0.0);
            estimate.metrics.content_height = content_box_pt(0.0);
        }
        estimate.swaps_physical_axes = self.swaps_physical_axes;
        estimate
    }
}

#[derive(Debug, Clone, Copy)]
struct ContributionProjection {
    /// The enclosing subgrid's area already projected into the root grid.
    area: GridItemArea,
    contributes_columns: bool,
    contributes_rows: bool,
    column_edges: ContributionEdges,
    row_edges: ContributionEdges,
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
        let local_edges = subgrid_box_edges(style);
        if nested_context.columns.is_some() {
            column_edges.add_assign(local_edges.0);
        }
        if nested_context.rows.is_some() {
            row_edges.add_assign(local_edges.1);
        }
        Self {
            area: self.project(local),
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
            let Some(context) = ResolvedSubgridContext::from_parent(
                parent_style,
                preliminary,
                &child.style,
                parent_area,
            ) else {
                continue;
            };
            let (column_edges, row_edges) = subgrid_box_edges(&child.style);
            self.collect_subgrid_contributions_from_context(
                child,
                context,
                ContributionProjection {
                    area: parent_area,
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
                parent_item.width().max(0.0),
                parent_item.height().max(0.0),
                &mut contributions,
            );
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
        child_width: f32,
        child_height: f32,
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
                child_width,
                Some(child_height),
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
                    let estimate = self.estimate_grid_item_size(
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
                    });
                }
                self.collect_subgrid_contributions_from_context(
                    grandchild,
                    nested_context,
                    nested_projection,
                    stylesheets,
                    item.width().max(0.0),
                    item.height().max(0.0),
                    contributions,
                );
                continue;
            }
            let estimate = self.estimate_grid_item_size(
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
            });
        }
    }
}

/// Convert a subgrid's physical box edges to its logical column/row axes.
/// The values are applied only to the outer ends of a descendant projection;
/// inner tracks continue to use the parent gutter geometry.
fn subgrid_box_edges(style: &ComputedStyle) -> (ContributionEdges, ContributionEdges) {
    let borders = used_border_widths(style);
    let edge = |side| {
        let physical = |edges: css::Edges| match side {
            PhysicalSide::Top => edges.top,
            PhysicalSide::Right => edges.right,
            PhysicalSide::Bottom => edges.bottom,
            PhysicalSide::Left => edges.left,
        };
        physical(style.margin) + physical(style.padding) + physical(borders)
    };
    let inline = ContributionEdges {
        start: edge(inline_start_side(
            style.writing_mode,
            style.used_direction(),
        )),
        end: edge(inline_end_side(style.writing_mode, style.used_direction())),
    };
    let block = ContributionEdges {
        start: edge(block_start_side(style.writing_mode)),
        end: edge(block_end_side(style.writing_mode)),
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
        }) {
            existing.merge(contribution.estimate);
        } else {
            merged.push(contribution);
        }
    }
    merged
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
            },
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(20.0, 3.0),
                contributes_columns: true,
                contributes_rows: false,
                column_edges: ContributionEdges::default(),
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
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
                    start: 2.0,
                    end: 0.0,
                },
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
            },
            SubgridContribution {
                area,
                estimate: GridItemEstimate::fixed(20.0, 20.0),
                contributes_columns: true,
                contributes_rows: true,
                column_edges: ContributionEdges::default(),
                row_edges: ContributionEdges::default(),
                swaps_physical_axes: false,
            },
        ]);
        assert_eq!(merged.len(), 2);
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
}
