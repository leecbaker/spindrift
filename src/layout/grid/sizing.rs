use super::baseline::{
    GridBaselinePlan, GridBaselineSet, apply_grid_baseline_alignment, grid_baseline_plan,
    grid_baseline_sizing_may_need_shims, grid_container_baseline,
    grid_taffy_margin_with_baseline_shim, resolve_grid_baseline_participation,
};
use super::item_adjustment::{
    GridFinalItemPercentagePlacement, apply_grid_aspect_ratio_item_size_corrections,
    apply_grid_deferred_percentage_item_size_corrections, apply_grid_self_alignment_corrections,
};
use super::lanes::GridLanesLayoutContext;
use super::model::{
    GridItemArea, GridItemLayout, GridLayout, GridLayoutPurpose, GridTaffyLeaf,
    apply_resolved_subgrid_axis_item_geometry,
};
use super::resolved::physical_grid_line_names;
use super::*;
use crate::layout::baseline::{BaselinePair, PhysicalBaselineSets, PhysicalTopBaselineOffset};

impl<'a> LayoutBuilder<'a> {
    /// Compute same-page grid item geometry with Quire-measured leaf estimates.
    ///
    /// CSS Grid track sizing consumes each item's min-content, max-content, and
    /// preferred size contributions. Taffy owns the Grid Level 1 placement and
    /// track-sizing algorithm here, while Quire supplies leaf measurements from
    /// the same inline, block, flex, table, and replaced-element paths used by
    /// other layout modes:
    /// <https://www.w3.org/TR/css-grid-1/#algo-overview> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
    ///
    /// `width` is the grid container's physical CSS content-box width.
    pub(super) fn compute_grid_layout(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        #[cfg(feature = "layout-profile")]
        let _layout_profile = crate::layout::layout_profile::grid_layout_scope(
            match purpose {
                GridLayoutPurpose::FinalLayout => {
                    crate::layout::layout_profile::GridProfilePurpose::Final
                }
                GridLayoutPurpose::IntrinsicProbe => {
                    crate::layout::layout_profile::GridProfilePurpose::Intrinsic
                }
            },
            children.len(),
            WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes(),
        );
        // A direct subgrid replay installs this context immediately before
        // entering its formatting context. Intrinsic probes may run first,
        // but they must only borrow that resolved topology: consuming it
        // there would make the subsequent final replay fall back to an
        // unresolved standalone grid. The final layout owns the one-shot
        // consumption.
        // <https://drafts.csswg.org/css-grid-2/#subgrids>
        let subgrid_context = match purpose {
            GridLayoutPurpose::IntrinsicProbe => self.resolved_subgrid_context_for_probe(),
            GridLayoutPurpose::FinalLayout => self.take_resolved_subgrid_context(),
        };
        self.compute_grid_layout_with_margin_trim(
            style,
            children,
            stylesheets,
            width,
            height,
            purpose,
            subgrid_context,
            true,
        )
    }

    /// Run Grid sizing after deriving any container-owned `margin-trim` used
    /// margins from a placement probe.  Placement is independent of an item's
    /// margins, while track sizing is not, so the probe must not become the
    /// final sizing pass.
    /// <https://drafts.csswg.org/css-box-4/#margin-trim-grid>.
    #[allow(clippy::too_many_arguments)]
    fn compute_grid_layout_with_margin_trim(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        purpose: GridLayoutPurpose,
        subgrid_context: Option<ResolvedSubgridContext>,
        derive_margin_trim: bool,
    ) -> Option<GridLayout> {
        if derive_margin_trim && grid_has_margin_trim(style) && !children.is_empty() {
            let placement_probe = self.compute_grid_layout_with_margin_trim(
                style,
                children,
                stylesheets,
                width,
                height,
                purpose,
                subgrid_context.clone(),
                false,
            )?;
            let plan = grid_margin_trim_plan(style, &placement_probe);
            if !plan.is_empty() {
                let mut trimmed_children = children.to_vec();
                for (index, child) in trimmed_children.iter_mut().enumerate() {
                    plan.apply_to_style(index, &mut child.style);
                }
                return self.compute_grid_layout_with_margin_trim(
                    style,
                    &trimmed_children,
                    stylesheets,
                    width,
                    height,
                    purpose,
                    subgrid_context,
                    false,
                );
            }
        }
        let preliminary_layout = self.compute_grid_layout_pass(
            style,
            children,
            stylesheets,
            subgrid_context.as_ref(),
            &[],
            GridLayoutPassConfig {
                width,
                root_height: height,
                item_width_basis: None,
                item_height_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                item_containing_block_bases: None,
                frozen_tracks: GridFrozenTrackTopology::default(),
                row_gap_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: None,
                item_placement_overrides: Vec::new(),
                baseline_plan: None,
            },
        )?;
        // Grid baseline alignment affects intrinsic track sizes before the
        // ordinary track-sizing algorithm runs.  Taffy does not expose an
        // item-baseline measurement channel, so first obtain its placement
        // topology, derive the spec's sizing-only baseline shims from that
        // topology, and run the real sizing pass with those shims installed.
        // The probe's placement is independent of track sizes; only the
        // resulting track contributions change in the second pass.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
        let baseline_plan = self.grid_baseline_sizing_plan(
            style,
            children,
            stylesheets,
            width,
            height,
            &preliminary_layout,
        );
        let preliminary_layout = if baseline_plan.is_empty() {
            preliminary_layout
        } else {
            self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &[],
                GridLayoutPassConfig {
                    width,
                    root_height: height,
                    item_width_basis: None,
                    item_height_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    item_containing_block_bases: None,
                    frozen_tracks: GridFrozenTrackTopology::default(),
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: None,
                    item_placement_overrides: Vec::new(),
                    baseline_plan: Some(baseline_plan.clone()),
                },
            )?
        };
        let contributions = if purpose == GridLayoutPurpose::FinalLayout
            && subgrid_context.is_none()
        {
            self.collect_subgrid_contributions(style, children, stylesheets, &preliminary_layout)
        } else {
            Vec::new()
        };
        // Descendant contribution proxies take part in parent track sizing,
        // but they are not Grid items and must not displace automatic items
        // during the proxy pass. Preserve the preliminary placement while
        // resolving the shared inherited tracks.
        // <https://drafts.csswg.org/css-grid-2/#subgrid-contributions>
        let contribution_item_placement_overrides = (!contributions.is_empty()).then(|| {
            preliminary_layout
                .items
                .iter()
                .map(|item| item.area)
                .collect::<Vec<_>>()
        });
        let intrinsic_layout = if contributions.is_empty() {
            preliminary_layout
        } else {
            self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: height,
                    item_width_basis: None,
                    item_height_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    item_containing_block_bases: None,
                    frozen_tracks: GridFrozenTrackTopology::default(),
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: None,
                    item_placement_overrides: contribution_item_placement_overrides
                        .clone()
                        .unwrap_or_default(),
                    baseline_plan: Some(baseline_plan.clone()),
                },
            )?
        };
        let intrinsic_layout = if style.display.is_grid_lanes() {
            intrinsic_layout
        } else {
            self.resolve_grid_intrinsic_feedback(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                width,
                height,
                baseline_plan.clone(),
                intrinsic_layout,
            )?
        };
        if height.is_none()
            && purpose == GridLayoutPurpose::FinalLayout
            && grid_gap_resolves_differently_with_basis(
                style.row_gap.clone(),
                intrinsic_layout.height,
            )
        {
            return self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context.as_ref(),
                &contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: Some(intrinsic_layout.height),
                    item_width_basis: None,
                    item_height_basis: PercentageBasis::indefinite(),
                    item_containing_block_bases: None,
                    frozen_tracks: GridFrozenTrackTopology::default(),
                    row_gap_basis: grid_percentage_basis(
                        Some(intrinsic_layout.height.content_box_length()),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(intrinsic_layout.height),
                    item_placement_overrides: contribution_item_placement_overrides
                        .unwrap_or_default(),
                    baseline_plan: Some(baseline_plan),
                },
            );
        }
        if style.display.is_grid_lanes() {
            return Some(self.apply_grid_lanes_placement(
                style,
                children,
                stylesheets,
                GridLanesLayoutContext {
                    width,
                    block_percentage_basis:
                        height.map_or_else(PercentageBasis::indefinite, |height| {
                            PercentageBasis::definite(layout_pt(height.points()))
                        }),
                    subgrid_context: subgrid_context.as_ref(),
                },
                intrinsic_layout,
            ));
        }
        Some(intrinsic_layout)
    }

    /// Complete CSS Grid's bounded cross-axis intrinsic-contribution feedback
    /// sequence. Taffy's public Grid API sizes both axes together, so a
    /// correction fixes the previously resolved opposite physical axis and
    /// preserves the established item placement.
    /// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
    #[allow(clippy::too_many_arguments)]
    fn resolve_grid_intrinsic_feedback(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        subgrid_context: Option<&ResolvedSubgridContext>,
        contributions: &[SubgridContribution],
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        baseline_plan: GridBaselinePlan,
        initial_layout: GridLayout,
    ) -> Option<GridLayout> {
        let mut layout = initial_layout;
        let initial_bases = grid_item_containing_block_bases(style, &layout);
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
            crate::layout::layout_profile::GridFeedbackSweep::Initial,
            children.len(),
        );
        let initial_estimates =
            self.grid_item_estimates_with_bases(children, stylesheets, width, &initial_bases);
        #[cfg(feature = "layout-profile")]
        drop(_profile);

        // Step 3: rows are now resolved, so an item's inline contribution may
        // change (notably through an aspect-ratio descendant). Re-resolve the
        // columns once while keeping rows and placement stable.
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
            crate::layout::layout_profile::GridFeedbackSweep::Container,
            children.len(),
        );
        let cyclic_estimates =
            self.grid_item_estimates_with_container_bases(children, stylesheets, width, height);
        #[cfg(feature = "layout-profile")]
        drop(_profile);
        if grid_inline_contributions_changed(&cyclic_estimates, &initial_estimates) {
            #[cfg(feature = "layout-profile")]
            crate::layout::layout_profile::record_grid_feedback_inline_correction();
            layout = self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context,
                contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: height,
                    item_width_basis: None,
                    item_height_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    item_containing_block_bases: Some(initial_bases),
                    frozen_tracks: GridFrozenTrackTopology {
                        columns: None,
                        rows: Some(layout.row_track_sizes.clone()),
                    },
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(layout.height),
                    item_placement_overrides: grid_item_placement_overrides(&layout),
                    baseline_plan: Some(baseline_plan.clone()),
                },
            )?;
        }

        // Step 4: the adjusted columns can in turn change a block-axis
        // contribution. Perform the symmetric row correction once only.
        let column_bases = grid_item_containing_block_bases(style, &layout);
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
            crate::layout::layout_profile::GridFeedbackSweep::Column,
            children.len(),
        );
        let column_estimates =
            self.grid_item_estimates_with_bases(children, stylesheets, width, &column_bases);
        #[cfg(feature = "layout-profile")]
        drop(_profile);
        // The initial track layout measured each ordinary grid item against
        // the container-wide available space. Compare that original cyclic
        // contribution with the estimate constrained by the resolved column
        // area; comparing two area-constrained probes would silently skip a
        // required row correction for padded nested grids.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
        if grid_block_contributions_changed(&cyclic_estimates, &column_estimates) {
            #[cfg(feature = "layout-profile")]
            crate::layout::layout_profile::record_grid_feedback_block_correction();
            layout = self.compute_grid_layout_pass(
                style,
                children,
                stylesheets,
                subgrid_context,
                contributions,
                GridLayoutPassConfig {
                    width,
                    root_height: height,
                    item_width_basis: None,
                    item_height_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    item_containing_block_bases: Some(column_bases),
                    frozen_tracks: GridFrozenTrackTopology {
                        columns: Some(layout.column_track_sizes.clone()),
                        rows: None,
                    },
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(layout.height),
                    item_placement_overrides: grid_item_placement_overrides(&layout),
                    baseline_plan: Some(baseline_plan.clone()),
                },
            )?;
        }

        // Grid items are finally laid out against their definite grid areas.
        // This supersedes the old fixed-row-only placement retry and covers
        // both explicit and content-sized tracks without feeding final item
        // percentages back into intrinsic track sizing.
        let final_bases = grid_item_containing_block_bases(style, &layout);
        let final_layout = self.compute_grid_layout_pass(
            style,
            children,
            stylesheets,
            subgrid_context,
            contributions,
            GridLayoutPassConfig {
                width,
                root_height: height,
                item_width_basis: None,
                item_height_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                item_containing_block_bases: Some(final_bases),
                frozen_tracks: GridFrozenTrackTopology {
                    columns: Some(layout.column_track_sizes.clone()),
                    rows: Some(layout.row_track_sizes.clone()),
                },
                row_gap_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: Some(layout.height),
                item_placement_overrides: grid_item_placement_overrides(&layout),
                baseline_plan: Some(baseline_plan),
            },
        )?;
        Some(final_layout)
    }

    fn grid_item_estimates_with_bases(
        &mut self,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        bases: &[GridItemContainingBlockBases],
    ) -> Vec<GridItemEstimate> {
        children
            .iter()
            .zip(bases)
            .map(|(child, bases)| {
                self.estimate_grid_item_size_for_parent_track_sizing(
                    child,
                    stylesheets,
                    bases.width.points().unwrap_or(width.points()),
                    bases.width,
                    bases.height,
                )
            })
            .collect()
    }

    fn grid_item_estimates_with_container_bases(
        &mut self,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
    ) -> Vec<GridItemEstimate> {
        let width_basis = grid_percentage_basis(
            Some(width.content_box_length()),
            GridAvailableSizeSource::ContainerInlineSize,
        );
        let height_basis = grid_percentage_basis(
            height.map(PhysicalContentHeight::content_box_length),
            GridAvailableSizeSource::ContainerBlockSize,
        );
        children
            .iter()
            .map(|child| {
                self.estimate_grid_item_size_for_parent_track_sizing(
                    child,
                    stylesheets,
                    width.points(),
                    width_basis,
                    height_basis,
                )
            })
            .collect()
    }

    /// Resolve the baseline sizing data from an already-placed Grid topology.
    ///
    /// Grid placement does not depend on the used track sizes, so the
    /// preliminary pass supplies stable row/column membership while this pass
    /// supplies the measured item baselines used to construct intrinsic-size
    /// shims.  The shims are installed only in the following Taffy sizing
    /// pass, never in the replay style.
    /// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
    fn grid_baseline_sizing_plan(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
        topology: &GridLayout,
    ) -> GridBaselinePlan {
        if !grid_baseline_sizing_may_need_shims(
            style,
            &topology.baseline_resolutions,
            &topology.items,
        ) {
            return GridBaselinePlan::default();
        }
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_baseline_plan_scope(children.len());
        let available_space = GridPhysicalAvailableSpace {
            width_basis: grid_percentage_basis(
                Some(width.content_box_length()),
                GridAvailableSizeSource::ContainerInlineSize,
            ),
            height_basis: grid_percentage_basis(
                height.map(PhysicalContentHeight::content_box_length),
                GridAvailableSizeSource::ContainerBlockSize,
            ),
        };
        let estimates = children
            .iter()
            .map(|child| {
                self.estimate_grid_item_size(
                    child,
                    stylesheets,
                    width.points(),
                    available_space.width_basis,
                    available_space.height_basis,
                )
            })
            .collect::<Vec<_>>();
        grid_baseline_plan(
            style,
            children,
            &estimates,
            &topology.baseline_resolutions,
            &topology.items,
        )
    }

    /// Run a subgrid contribution probe against its placed border-box
    /// geometry. The probe owns the corresponding content-box conversion so
    /// descendants are measured against the same used space as final replay.
    pub(super) fn compute_grid_layout_for_subgrid_contribution_probe(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        placed_item_dimensions: GridItemReplayDimensions,
        purpose: GridLayoutPurpose,
    ) -> Option<GridLayout> {
        // This temporary probe owns the context installed by the contribution
        // collector. Unlike an intrinsic probe encountered while replaying a
        // real subgrid item, it has no later formatting pass that must retain
        // the context.
        let subgrid_context = self.take_resolved_subgrid_context();
        self.compute_grid_layout_with_margin_trim(
            style,
            children,
            stylesheets,
            placed_item_dimensions.physical_content_width_for_replay(style),
            Some(placed_item_dimensions.physical_content_height_for_replay(style)),
            purpose,
            subgrid_context,
            true,
        )
    }

    /// Compute one Taffy grid layout pass.
    ///
    /// CSS Box Alignment makes Grid cyclic percentage gaps resolve against zero
    /// for intrinsic size contributions, but against the grid container content
    /// box when laying out contents. Callers can therefore provide a definite
    /// `root_height` for final content layout while keeping `item_height_basis`
    /// indefinite for grid item percentage block sizes:
    /// <https://www.w3.org/TR/css-align-3/#gap-percent> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-sizing>.
    pub(super) fn compute_grid_layout_pass(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        subgrid_context: Option<&ResolvedSubgridContext>,
        contributions: &[SubgridContribution],
        config: GridLayoutPassConfig,
    ) -> Option<GridLayout> {
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_track_sizing_scope(children.len());
        let content_width = config.width;
        let width = content_width.points();
        let root_height = config.root_height;
        let root_height_points = root_height.map(PhysicalContentHeight::points);
        let contained_subgrid_columns = (style.contain.layout || style.contain.paint)
            && matches!(style.grid_template_columns, css::GridTrackList::None);
        let contained_subgrid_rows = (style.contain.layout || style.contain.paint)
            && matches!(style.grid_template_rows, css::GridTrackList::None);
        let item_height_basis = if contained_subgrid_rows {
            GridPercentageBasis::indefinite()
        } else {
            config.item_height_basis
        };
        let row_gap_basis = config.row_gap_basis;
        // A contained subgrid axis resolves to `none`, leaving its automatic
        // implicit tracks to size from intrinsic contributions. Percentages on
        // those grid items are cyclic during that sizing phase and therefore
        // behave as `auto`; feeding the final stretched grid-area width back
        // here would incorrectly make `width: 100%` grow the implicit track.
        // <https://drafts.csswg.org/css-grid-2/#subgrid-listing> and
        // <https://drafts.csswg.org/css-grid-1/#percentage-sizing>
        let item_width_basis = config.item_width_basis.unwrap_or_else(|| {
            if contained_subgrid_columns {
                GridPercentageBasis::indefinite()
            } else {
                grid_percentage_basis(
                    Some(content_width.content_box_length()),
                    GridAvailableSizeSource::ContainerInlineSize,
                )
            }
        });
        let item_available_space = GridPhysicalAvailableSpace {
            width_basis: item_width_basis,
            height_basis: item_height_basis,
        };
        // Taffy's grid tracks are physical: columns run along x and rows along
        // y. CSS Grid's columns/rows are logical inline/block axes, so vertical
        // writing modes must swap the templates, auto tracks, placement lines,
        // alignment axes, and gaps at this adapter boundary:
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
        // <https://www.w3.org/TR/css-grid-2/#track-sizing>.
        let swaps_physical_grid_axes =
            WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes();
        let track_percentage_bases =
            GridTrackPercentageBases::from_grid_content_box(style, content_width, root_height);
        let physical_column_subgrid = subgrid_context
            .and_then(|context| context.physical_axis(GridAxis::Column, swaps_physical_grid_axes));
        let physical_row_subgrid = subgrid_context
            .and_then(|context| context.physical_axis(GridAxis::Row, swaps_physical_grid_axes));
        let resolved_item_placements = subgrid_context
            .map(|context| context.resolve_item_placements(children, style.grid_auto_flow));
        let mut tree: taffy_layout::TaffyTree<GridTaffyLeaf> = taffy_layout::TaffyTree::new();
        tree.disable_rounding();
        let row_adjustment = taffy_startward_implicit_row_adjustment(
            style,
            children,
            track_percentage_bases.for_axis(GridAxis::Row),
        );
        let column_adjustment = taffy_startward_implicit_column_adjustment(
            style,
            children,
            track_percentage_bases.for_axis(GridAxis::Column),
        );
        let mut nodes = Vec::with_capacity(children.len());
        let mut estimates = Vec::with_capacity(children.len());
        let mut item_box_metrics = Vec::with_capacity(children.len());
        for (index, child) in children.iter().enumerate() {
            let item_bases = config
                .item_containing_block_bases
                .as_ref()
                .and_then(|bases| bases.get(index))
                .copied();
            // A contained subgrid has already resolved this axis to a
            // standalone `none` track list. Do not reintroduce the parent
            // subgrid area through replay's per-item basis: percentages in
            // its implicit tracks are cyclic and behave as auto.
            // <https://drafts.csswg.org/css-grid-2/#subgrid-listing>
            // <https://drafts.csswg.org/css-sizing-3/#cyclic-percentage>
            let child_width_basis = if contained_subgrid_columns {
                item_width_basis
            } else {
                item_bases.map_or(item_width_basis, |bases| bases.width)
            };
            let child_height_basis = if contained_subgrid_rows {
                item_height_basis
            } else {
                item_bases.map_or(item_height_basis, |bases| bases.height)
            };
            let child_inline_basis = GridPhysicalAvailableSpace {
                width_basis: child_width_basis,
                height_basis: child_height_basis,
            }
            .logical_inline_basis(style);
            // During Grid's bounded cross-axis feedback and its final pass,
            // an item's resolved grid area is the available physical space
            // for intrinsic measurement.  The percentage basis alone is not
            // enough: nested grids must also receive this dimension as their
            // content measurement constraint, so their own padding shrinks
            // descendants before they contribute a block size.
            // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
            let child_available_width = child_width_basis.points().unwrap_or(width);
            let placement_override = config
                .item_placement_overrides
                .get(index)
                .copied()
                .flatten();
            let overridden_row = placement_override.and_then(|area| {
                taffy_grid_area_line(
                    area,
                    if swaps_physical_grid_axes {
                        GridAxis::Column
                    } else {
                        GridAxis::Row
                    },
                )
            });
            let overridden_column = placement_override.and_then(|area| {
                taffy_grid_area_line(
                    area,
                    if swaps_physical_grid_axes {
                        GridAxis::Row
                    } else {
                        GridAxis::Column
                    },
                )
            });
            let resolved_placement = resolved_item_placements
                .as_ref()
                .map(|placements| placements[index]);
            let estimate = self.estimate_grid_item_size(
                child,
                stylesheets,
                child_available_width,
                child_width_basis,
                child_height_basis,
            );
            // A subgrid has no independent intrinsic contribution in an
            // inherited axis. Its explicitly placed descendants are inserted
            // below as projected proxy leaves after preliminary placement.
            // Retain the original estimate for replay/baseline bookkeeping;
            // only the Taffy sizing leaf is made empty in that logical axis.
            // <https://drafts.csswg.org/css-grid-2/#subgrid-track-sizing>
            let sizing_estimate = grid_item_parent_sizing_estimate(estimate, &child.style);
            // Grid's intrinsic contributions are logical, while Taffy's
            // layout tree is physical. Keep the logical estimate for Grid's
            // own track and baseline calculations, and project only the
            // automatic measurement inputs supplied to Taffy.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
            let physical_estimate = sizing_estimate.physical_measurements();
            estimates.push(estimate);
            item_box_metrics.push(used_box_metrics_for_logical_inline_basis(
                &child.style,
                child_inline_basis.map_source(|_| ()),
            ));
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        box_sizing: taffy_bridge::box_sizing(child.style.box_sizing),
                        direction: taffy_bridge::direction(child.style.used_direction()),
                        size: taffy_layout::Size {
                            width: taffy_grid_item_dimension(
                                child.style.box_values.width.clone(),
                                child_width_basis,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_dimension(
                                child.style.box_values.height.value().clone(),
                                child_height_basis,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        // CSS Grid's track-sizing and item-layout phases both
                        // need the preferred ratio: a definite grid-area size
                        // in either axis transfers to the automatic opposite
                        // axis before alignment and intrinsic contribution
                        // resolution.
                        // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
                        aspect_ratio: child
                            .style
                            .aspect_ratio
                            .preferred_ratio_for_non_replaced(false),
                        min_size: taffy_layout::Size {
                            width: grid_item_taffy_min_dimension(
                                child.style.box_values.min_width.clone(),
                                GridAxis::Column,
                                style,
                                &child.style,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: grid_item_taffy_min_dimension(
                                child.style.box_values.min_height.clone(),
                                GridAxis::Row,
                                style,
                                &child.style,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_width.clone(),
                                GridPercentageBasis::indefinite(),
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_constraint_dimension(
                                child.style.box_values.max_height.clone(),
                                GridPercentageBasis::indefinite(),
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        margin: grid_taffy_margin_with_baseline_shim(
                            &child.style,
                            child_inline_basis,
                            config
                                .baseline_plan
                                .as_ref()
                                .and_then(|plan| plan.shim(index)),
                        ),
                        padding: taffy_bridge::padding(&child.style, child_inline_basis),
                        border: taffy_bridge::border_edges(used_border_widths(&child.style)),
                        align_self: if swaps_physical_grid_axes {
                            taffy_effective_grid_justify_self(&child.style, style)
                        } else {
                            taffy_effective_grid_align_self(&child.style, style)
                        },
                        justify_self: if swaps_physical_grid_axes {
                            taffy_effective_grid_align_self(&child.style, style)
                        } else {
                            taffy_effective_grid_justify_self(&child.style, style)
                        },
                        grid_row: overridden_row.unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                resolved_placement
                                    .and_then(|placement| placement.columns)
                                    .map(ResolvedSubgridPlacement::taffy_line)
                                    .unwrap_or_else(|| {
                                        physical_row_subgrid.map_or_else(
                                            || {
                                                taffy_grid_line_with_startward_adjustment(
                                                    &child.style.grid_column_start,
                                                    &child.style.grid_column_end,
                                                    &column_adjustment,
                                                )
                                            },
                                            |axis| {
                                                axis.clamped_taffy_line(
                                                    &child.style.grid_column_start,
                                                    &child.style.grid_column_end,
                                                )
                                            },
                                        )
                                    })
                            } else {
                                resolved_placement
                                    .and_then(|placement| placement.rows)
                                    .map(ResolvedSubgridPlacement::taffy_line)
                                    .unwrap_or_else(|| {
                                        physical_row_subgrid.map_or_else(
                                            || {
                                                taffy_grid_line_with_startward_adjustment(
                                                    &child.style.grid_row_start,
                                                    &child.style.grid_row_end,
                                                    &row_adjustment,
                                                )
                                            },
                                            |axis| {
                                                axis.clamped_taffy_line(
                                                    &child.style.grid_row_start,
                                                    &child.style.grid_row_end,
                                                )
                                            },
                                        )
                                    })
                            }
                        }),
                        grid_column: overridden_column.unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                resolved_placement
                                    .and_then(|placement| placement.rows)
                                    .map(ResolvedSubgridPlacement::taffy_line)
                                    .unwrap_or_else(|| {
                                        physical_column_subgrid.map_or_else(
                                            || {
                                                taffy_grid_line_with_startward_adjustment(
                                                    &child.style.grid_row_start,
                                                    &child.style.grid_row_end,
                                                    &row_adjustment,
                                                )
                                            },
                                            |axis| {
                                                axis.clamped_taffy_line(
                                                    &child.style.grid_row_start,
                                                    &child.style.grid_row_end,
                                                )
                                            },
                                        )
                                    })
                            } else {
                                resolved_placement
                                    .and_then(|placement| placement.columns)
                                    .map(ResolvedSubgridPlacement::taffy_line)
                                    .unwrap_or_else(|| {
                                        physical_column_subgrid.map_or_else(
                                            || {
                                                taffy_grid_line_with_startward_adjustment(
                                                    &child.style.grid_column_start,
                                                    &child.style.grid_column_end,
                                                    &column_adjustment,
                                                )
                                            },
                                            |axis| {
                                                axis.clamped_taffy_line(
                                                    &child.style.grid_column_start,
                                                    &child.style.grid_column_end,
                                                )
                                            },
                                        )
                                    })
                            }
                        }),
                        ..Default::default()
                    },
                    GridTaffyLeaf::Item(sizing_estimate),
                )
                .ok()?;
            nodes.push(node);
        }
        let mut contribution_nodes = Vec::with_capacity(contributions.len());
        for contribution in contributions {
            // The collector retains the descendant's content-box intrinsic
            // measurement and its accumulated subgrid edge adjustments
            // separately. Taffy owns their margin-inclusive track sizing,
            // exactly as it does for ordinary Grid items.
            let estimate = contribution.sizing_estimate().physical_measurements();
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Block,
                        margin: contribution.taffy_margin(),
                        grid_row: taffy_layout::Line {
                            start: taffy_layout::line(
                                i16::try_from(contribution.area.row_start).ok()?,
                            ),
                            end: taffy_layout::line(i16::try_from(contribution.area.row_end).ok()?),
                        },
                        grid_column: taffy_layout::Line {
                            start: taffy_layout::line(
                                i16::try_from(contribution.area.column_start).ok()?,
                            ),
                            end: taffy_layout::line(
                                i16::try_from(contribution.area.column_end).ok()?,
                            ),
                        },
                        ..Default::default()
                    },
                    GridTaffyLeaf::Contribution(estimate),
                )
                .ok()?;
            contribution_nodes.push(node);
        }
        // Taffy's empty grid currently omits otherwise-valid explicit track
        // geometry. CSS Grid still sizes an empty grid from its explicit
        // tracks and gaps, which is particularly observable when size
        // containment removes every real item from the principal sizing pass.
        // A zero-contribution probe item materializes those tracks without
        // entering Quire's returned item list. Auto-fit grids remain genuinely
        // empty so their unoccupied repeated tracks can collapse.
        // <https://www.w3.org/TR/css-grid-1/#explicit-grids>
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let mut layout_nodes = nodes.clone();
        layout_nodes.extend(contribution_nodes);
        if layout_nodes.is_empty()
            && !grid_track_list_has_auto_fit(&style.grid_template_columns)
            && !grid_track_list_has_auto_fit(&style.grid_template_rows)
        {
            let zero = taffy_layout::Dimension::length(0.0);
            let placeholder = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        min_size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        max_size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        ..Default::default()
                    },
                    GridTaffyLeaf::Contribution(GridItemEstimate::fixed(0.0, 0.0)),
                )
                .ok()?;
            layout_nodes.push(placeholder);
        }
        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Grid,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_bridge::direction(style.used_direction()),
                    size: taffy_layout::Size {
                        width: taffy_layout::Dimension::length(width),
                        height: root_height_points
                            .map(taffy_layout::Dimension::length)
                            .unwrap_or_else(taffy_layout::Dimension::auto),
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.min_width.clone()),
                        height: taffy_dimension(style.box_values.min_height.clone()),
                    },
                    max_size: taffy_layout::Size {
                        width: taffy_dimension(style.box_values.max_width.clone()),
                        height: taffy_dimension(style.box_values.max_height.clone()),
                    },
                    grid_template_columns: config
                        .frozen_tracks
                        .columns
                        .as_deref()
                        .map(taffy_fixed_grid_tracks)
                        .or_else(|| physical_column_subgrid.map(ResolvedSubgridAxis::taffy_tracks))
                        .unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                taffy_grid_template_rows_with_startward_adjustment(
                                    style,
                                    &row_adjustment,
                                    track_percentage_bases.for_axis(GridAxis::Row),
                                )
                            } else {
                                taffy_grid_template_columns_with_startward_adjustment(
                                    style,
                                    &column_adjustment,
                                    track_percentage_bases.for_axis(GridAxis::Column),
                                )
                            }
                        }),
                    grid_template_rows: config
                        .frozen_tracks
                        .rows
                        .as_deref()
                        .map(taffy_fixed_grid_tracks)
                        .or_else(|| physical_row_subgrid.map(ResolvedSubgridAxis::taffy_tracks))
                        .unwrap_or_else(|| {
                            if swaps_physical_grid_axes {
                                taffy_grid_template_columns_with_startward_adjustment(
                                    style,
                                    &column_adjustment,
                                    track_percentage_bases.for_axis(GridAxis::Column),
                                )
                            } else {
                                taffy_grid_template_rows_with_startward_adjustment(
                                    style,
                                    &row_adjustment,
                                    track_percentage_bases.for_axis(GridAxis::Row),
                                )
                            }
                        }),
                    grid_template_areas: taffy_grid_template_areas_with_startward_adjustment(
                        &style.grid_template_areas,
                        &row_adjustment,
                        &column_adjustment,
                    ),
                    grid_template_column_names: physical_column_subgrid
                        .map(|axis| axis.line_names().to_vec())
                        .unwrap_or_else(|| {
                            taffy_grid_template_column_names_with_startward_adjustment(
                                style,
                                &column_adjustment,
                            )
                        }),
                    grid_template_row_names: physical_row_subgrid
                        .map(|axis| axis.line_names().to_vec())
                        .unwrap_or_else(|| {
                            taffy_grid_template_row_names_with_startward_adjustment(
                                style,
                                &row_adjustment,
                            )
                        }),
                    grid_auto_columns: (config.frozen_tracks.columns.is_some()
                        || physical_column_subgrid.is_some())
                    .then(Vec::new)
                    .unwrap_or_else(|| {
                        if swaps_physical_grid_axes {
                            taffy_grid_auto_tracks(&style.grid_auto_rows)
                        } else {
                            taffy_grid_auto_tracks(&style.grid_auto_columns)
                        }
                    }),
                    grid_auto_rows: (config.frozen_tracks.rows.is_some()
                        || physical_row_subgrid.is_some())
                    .then(Vec::new)
                    .unwrap_or_else(|| {
                        if swaps_physical_grid_axes {
                            taffy_grid_auto_tracks(&style.grid_auto_columns)
                        } else {
                            taffy_grid_auto_tracks(&style.grid_auto_rows)
                        }
                    }),
                    grid_auto_flow: taffy_grid_auto_flow(style.grid_auto_flow),
                    justify_content: Some(if swaps_physical_grid_axes {
                        taffy_grid_align_content(style.align_content)
                    } else {
                        taffy_grid_justify_content(style.justify_content)
                    }),
                    align_content: Some(if swaps_physical_grid_axes {
                        taffy_grid_justify_content(style.justify_content)
                    } else {
                        taffy_grid_align_content(style.align_content)
                    }),
                    justify_items: Some(if swaps_physical_grid_axes {
                        taffy_grid_align_items(style.align_items)
                    } else {
                        taffy_grid_justify_items(style.justify_items)
                    }),
                    align_items: Some(if swaps_physical_grid_axes {
                        taffy_grid_justify_items(style.justify_items)
                    } else {
                        taffy_grid_align_items(style.align_items)
                    }),
                    gap: taffy_layout::Size {
                        width: physical_column_subgrid.map_or_else(
                            || {
                                taffy_bridge::gap(
                                    if swaps_physical_grid_axes {
                                        style.row_gap.clone()
                                    } else {
                                        style.column_gap.clone()
                                    },
                                    item_width_basis,
                                )
                            },
                            |axis| taffy_layout::LengthPercentage::length(axis.taffy_gap()),
                        ),
                        height: physical_row_subgrid.map_or_else(
                            || {
                                taffy_bridge::gap(
                                    if swaps_physical_grid_axes {
                                        style.column_gap.clone()
                                    } else {
                                        style.row_gap.clone()
                                    },
                                    row_gap_basis,
                                )
                            },
                            |axis| taffy_layout::LengthPercentage::length(axis.taffy_gap()),
                        ),
                    },
                    ..Default::default()
                },
                &layout_nodes,
            )
            .ok()?;
        tree.compute_layout_with_measure(
            root,
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(width),
                height: root_height_points
                    .map(taffy_layout::AvailableSpace::Definite)
                    .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
            },
            |known_dimensions, available_space, _node_id, node_context, _style| {
                let estimate = node_context.map(|context| match context {
                    GridTaffyLeaf::Item(estimate) | GridTaffyLeaf::Contribution(estimate) => {
                        estimate
                    }
                });
                measure_grid_item(known_dimensions, available_space, estimate)
            },
        )
        .ok()?;
        let root_layout = tree.layout(root).ok()?;
        let mut grid_item_areas = Vec::new();
        let mut column_line_offsets = Vec::new();
        let mut row_line_offsets = Vec::new();
        let mut column_track_sizes = Vec::new();
        let mut row_track_sizes = Vec::new();
        let mut track_corrections = GridTrackLayoutCorrections::default();
        let mut gap_gutters = match tree.detailed_layout_info(root) {
            taffy::tree::DetailedLayoutInfo::Grid(info) => {
                grid_item_areas = info
                    .items
                    .iter()
                    .map(|item| GridItemArea {
                        row_start: item.row_start,
                        row_end: item.row_end,
                        column_start: item.column_start,
                        column_end: item.column_end,
                    })
                    .collect();
                let column_correction = startward_auto_fit_track_correction(
                    style,
                    GridAxis::Column,
                    &column_adjustment,
                    &info.columns.sizes,
                    &info.columns.gutters,
                    &grid_item_areas,
                );
                let row_correction = startward_auto_fit_track_correction(
                    style,
                    GridAxis::Row,
                    &row_adjustment,
                    &info.rows.sizes,
                    &info.rows.gutters,
                    &grid_item_areas,
                );
                let column_sizes = column_correction
                    .as_ref()
                    .map(|correction| correction.sizes.as_slice())
                    .unwrap_or(&info.columns.sizes);
                let column_gutters = column_correction
                    .as_ref()
                    .map(|correction| correction.gutters.as_slice())
                    .unwrap_or(&info.columns.gutters);
                let row_sizes = row_correction
                    .as_ref()
                    .map(|correction| correction.sizes.as_slice())
                    .unwrap_or(&info.rows.sizes);
                let row_gutters = row_correction
                    .as_ref()
                    .map(|correction| correction.gutters.as_slice())
                    .unwrap_or(&info.rows.gutters);
                column_track_sizes = column_sizes.to_vec();
                row_track_sizes = row_sizes.to_vec();
                column_line_offsets = column_correction
                    .as_ref()
                    .map(|correction| correction.offsets.clone())
                    .unwrap_or_else(|| {
                        grid_line_offsets_from_track_layout(
                            &info.columns.sizes,
                            &info.columns.gutters,
                        )
                    });
                row_line_offsets = row_correction
                    .as_ref()
                    .map(|correction| correction.offsets.clone())
                    .unwrap_or_else(|| {
                        grid_line_offsets_from_track_layout(&info.rows.sizes, &info.rows.gutters)
                    });
                let gap_gutters = grid_gap_decoration_gutters_from_tracks(
                    column_sizes,
                    column_gutters,
                    row_sizes,
                    row_gutters,
                    style,
                    width,
                    root_layout.size.height,
                );
                track_corrections = GridTrackLayoutCorrections {
                    columns: column_correction,
                    rows: row_correction,
                };
                gap_gutters
            }
            taffy::tree::DetailedLayoutInfo::None => GapDecorationGridGutters::default(),
        };
        // Taffy is used to resolve placement topology, but it has no subgrid
        // model. Once placement is known, inherited axes must retain the
        // parent-owned track and gutter geometry exactly.
        // <https://www.w3.org/TR/css-grid-2/#subgrids>
        if let Some(axis) = physical_column_subgrid {
            column_line_offsets = axis.line_offsets().to_vec();
            column_track_sizes = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            gap_gutters.columns = axis.gap_gutters();
        }
        if let Some(axis) = physical_row_subgrid {
            row_line_offsets = axis.line_offsets().to_vec();
            row_track_sizes = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            gap_gutters.rows = axis.gap_gutters();
        }
        let column_line_names = physical_column_subgrid.map_or_else(
            || physical_grid_line_names(style, GridAxis::Column, column_line_offsets.len()),
            |axis| axis.physical_line_names().to_vec(),
        );
        let row_line_names = physical_row_subgrid.map_or_else(
            || physical_grid_line_names(style, GridAxis::Row, row_line_offsets.len()),
            |axis| axis.physical_line_names().to_vec(),
        );
        // Proxy nodes are appended after real child nodes and intentionally
        // have no corresponding `GridItemLayout`: they influence only Taffy's
        // track sizing, never replay, baselines, gap decoration, or fragment
        // planning.
        debug_assert!(grid_item_areas.len() >= nodes.len());
        let mut items = nodes
            .into_iter()
            .enumerate()
            .map(|(index, node)| {
                let layout = tree.layout(node).ok()?;
                Some(
                    GridItemLayout::new(
                        GridRect::new(
                            GridPoint::new(layout.location.x, layout.location.y),
                            GridSize::new(layout.size.width.max(0.0), layout.size.height.max(0.0)),
                        ),
                        grid_item_areas.get(index).cloned(),
                    )
                    .with_used_box_metrics(item_box_metrics[index]),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        debug_assert_eq!(items.len(), children.len());
        apply_resolved_subgrid_axis_item_geometry(
            physical_column_subgrid,
            GridAxis::Column,
            &mut items,
        );
        apply_resolved_subgrid_axis_item_geometry(physical_row_subgrid, GridAxis::Row, &mut items);
        let final_grid_height = physical_row_subgrid
            .map(ResolvedSubgridAxis::outer_extent)
            .unwrap_or(root_layout.size.height);
        // Stretch computes a final grid-area rectangle without converting an
        // automatic item inline size into an authored definite size. When a
        // vertical grid's physical height was indefinite during track sizing,
        // preserve that cycle at replay so the item's content uses the same
        // percentage behavior that participated in intrinsic sizing.
        // <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
        if swaps_physical_grid_axes && !item_height_basis.is_definite() {
            for (item, child) in items.iter_mut().zip(children) {
                let inline_self = effective_grid_justify_self(&child.style, style).keyword;
                if child.style.box_values.height.is_auto()
                    && matches!(
                        inline_self,
                        SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
                    )
                {
                    item.preserve_cyclic_physical_height_on_replay();
                }
            }
        }
        apply_startward_auto_fit_track_corrections(
            style,
            content_width,
            final_grid_height,
            &track_corrections,
            &mut items,
        );
        apply_grid_self_alignment_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        apply_grid_aspect_ratio_item_size_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &column_line_offsets,
            &row_line_offsets,
            &mut items,
        );
        apply_grid_replaced_item_size_corrections(style, children, &estimates, &mut items);
        apply_grid_deferred_percentage_item_size_corrections(
            GridFinalItemPercentagePlacement {
                container_style: style,
                container_width: content_width,
                container_height: final_grid_height,
                column_line_offsets: &column_line_offsets,
                row_line_offsets: &row_line_offsets,
            },
            children,
            &estimates,
            &mut items,
        );
        let baseline_resolutions =
            resolve_grid_baseline_participation(style, children, &items, item_available_space);
        // Taffy reports each item's border-box location after resolving its
        // grid-area margins. Replay suppresses those margins in the child
        // style, but must retain the reported border-box origin unchanged.
        // <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
        apply_grid_baseline_alignment(
            style,
            children,
            &estimates,
            &baseline_resolutions,
            &row_line_offsets,
            &mut items,
        );
        let first_baseline = grid_container_baseline(
            style,
            &estimates,
            &baseline_resolutions,
            &items,
            GridBaselineSet::First,
        );
        let last_baseline = grid_container_baseline(
            style,
            &estimates,
            &baseline_resolutions,
            &items,
            GridBaselineSet::Last,
        );
        let baselines = PhysicalBaselineSets {
            vertical: BaselinePair {
                first: first_baseline
                    .map(|baseline| PhysicalTopBaselineOffset::new(layout_pt(baseline))),
                last: last_baseline
                    .map(|baseline| PhysicalTopBaselineOffset::new(layout_pt(baseline))),
            },
            ..PhysicalBaselineSets::default()
        };
        Some(GridLayout {
            height: config
                .reported_height
                .unwrap_or_else(|| PhysicalContentHeight::new(content_box_pt(final_grid_height))),
            baselines,
            first_baseline,
            last_baseline,
            items,
            baseline_resolutions,
            gap_gutters,
            column_line_offsets,
            row_line_offsets,
            column_line_names,
            row_line_names,
            column_track_sizes,
            row_track_sizes,
        })
    }
}

fn grid_has_margin_trim(style: &ComputedStyle) -> bool {
    let trim = style.margin_trim;
    trim.block_start || trim.block_end || trim.inline_start || trim.inline_end
}

/// Derive a Grid item's trimmed physical margins from its placed logical area.
///
/// CSS Grid trims every item in the edge track.  Taffy reports one-indexed
/// physical row/column line numbers, so translate those back through the
/// container writing mode before comparing the outer tracks. Empty tracks are
/// retained, while only zero-sized unoccupied `auto-fit` tracks are ignored.
/// <https://drafts.csswg.org/css-box-4/#margin-trim-grid>.
fn grid_margin_trim_plan(style: &ComputedStyle, layout: &GridLayout) -> MarginTrimPlan {
    let mut plan = MarginTrimPlan::for_item_count(layout.items.len());
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let swaps_axes = axes.swaps_physical_axes();
    let physical_spans = layout
        .items
        .iter()
        .filter_map(|item| item.area)
        .collect::<Vec<_>>();
    let (inline_spans, block_spans, inline_sizes, block_sizes) = if swaps_axes {
        (
            physical_spans
                .iter()
                .map(|area| (area.row_start, area.row_end))
                .collect::<Vec<_>>(),
            physical_spans
                .iter()
                .map(|area| (area.column_start, area.column_end))
                .collect::<Vec<_>>(),
            layout.row_track_sizes.as_slice(),
            layout.column_track_sizes.as_slice(),
        )
    } else {
        (
            physical_spans
                .iter()
                .map(|area| (area.column_start, area.column_end))
                .collect::<Vec<_>>(),
            physical_spans
                .iter()
                .map(|area| (area.row_start, area.row_end))
                .collect::<Vec<_>>(),
            layout.column_track_sizes.as_slice(),
            layout.row_track_sizes.as_slice(),
        )
    };
    let (inline_first, inline_last) =
        grid_margin_trim_edge_lines(&style.grid_template_columns, inline_sizes, &inline_spans);
    let (block_first, block_last) =
        grid_margin_trim_edge_lines(&style.grid_template_rows, block_sizes, &block_spans);

    for (index, item) in layout.items.iter().enumerate() {
        let Some(area) = item.area else {
            continue;
        };
        let (inline_start, inline_end, block_start, block_end) = if swaps_axes {
            (
                area.row_start,
                area.row_end,
                area.column_start,
                area.column_end,
            )
        } else {
            (
                area.column_start,
                area.column_end,
                area.row_start,
                area.row_end,
            )
        };
        if style.margin_trim.inline_start && inline_start == inline_first {
            plan.trim(index, axes.physical_side(LogicalSide::InlineStart));
        }
        if style.margin_trim.inline_end && inline_end == inline_last {
            plan.trim(index, axes.physical_side(LogicalSide::InlineEnd));
        }
        if style.margin_trim.block_start && block_start == block_first {
            plan.trim(index, axes.physical_side(LogicalSide::BlockStart));
        }
        if style.margin_trim.block_end && block_end == block_last {
            plan.trim(index, axes.physical_side(LogicalSide::BlockEnd));
        }
    }
    plan
}

/// Return the first and last non-collapsed grid lines for one logical axis.
///
/// Outside `auto-fit`, even a zero-sized or empty track remains relevant to
/// margin adjacency. In an `auto-fit` repetition, CSS Grid collapses only
/// empty repeated tracks; an occupied zero-sized track must still be kept.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn grid_margin_trim_edge_lines(
    tracks: &css::GridTrackList,
    sizes: &[f32],
    areas: &[(u16, u16)],
) -> (u16, u16) {
    let last_line = u16::try_from(sizes.len().saturating_add(1)).unwrap_or(u16::MAX);
    if !grid_track_list_has_auto_fit(tracks) {
        return (1, last_line);
    }
    let occupied = |track_index: usize| {
        let line = u16::try_from(track_index.saturating_add(1)).unwrap_or(u16::MAX);
        areas
            .iter()
            .any(|(start, end)| *start <= line && line < *end)
    };
    let first = sizes
        .iter()
        .enumerate()
        .find(|(index, size)| size.abs() > 0.01 || occupied(*index))
        .and_then(|(index, _)| u16::try_from(index.saturating_add(1)).ok())
        .unwrap_or(1);
    let last = sizes
        .iter()
        .enumerate()
        .rev()
        .find(|(index, size)| size.abs() > 0.01 || occupied(*index))
        .and_then(|(index, _)| u16::try_from(index.saturating_add(2)).ok())
        .unwrap_or(last_line);
    (first, last)
}

/// The direct grid item representing a subgrid is empty in an inherited axis;
/// projected descendant proxy leaves supply that axis's track-sizing
/// contribution. `GridItemEstimate` is logical, so this remains independent
/// of the container's physical writing-mode adapter.
fn grid_item_parent_sizing_estimate(
    mut estimate: GridItemEstimate,
    style: &ComputedStyle,
) -> GridItemEstimate {
    if matches!(
        style.grid_template_columns,
        css::GridTrackList::Subgrid { .. }
    ) {
        estimate.metrics.width = content_box_pt(0.0);
        estimate.metrics.min_width = content_box_pt(0.0);
        estimate.metrics.content_width = content_box_pt(0.0);
    }
    if matches!(style.grid_template_rows, css::GridTrackList::Subgrid { .. }) {
        estimate.metrics.height = content_box_pt(0.0);
        estimate.metrics.min_height = content_box_pt(0.0);
        estimate.metrics.content_height = content_box_pt(0.0);
    }
    estimate
}

/// Convert a Grid item's automatic minimum for Taffy after applying Grid's
/// eligibility conditions. Taffy's generic `auto` minimum is content based,
/// but Grid explicitly zeroes that minimum for a multi-track span containing
/// a flexible track.
/// <https://www.w3.org/TR/css-grid-1/#min-size-auto>
fn grid_item_taffy_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    physical_axis: GridAxis,
    container_style: &ComputedStyle,
    item_style: &ComputedStyle,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    if value.is_auto()
        && grid_item_spans_flexible_track_on_physical_axis(
            container_style,
            item_style,
            physical_axis,
        )
    {
        return taffy_layout::Dimension::length(0.0);
    }
    taffy_grid_item_min_dimension(
        value,
        GridPercentageBasis::indefinite(),
        min_content,
        max_content,
    )
}

/// Whether an explicitly counted multi-track span crosses a flexible track.
///
/// The Grid automatic-minimum rule is deliberately based on track sizing
/// functions, not the final numeric track sizes. The full placement engine
/// remains Taffy's responsibility; this early decision is only needed for a
/// span encoded directly in the item style, before Taffy consumes the leaf's
/// minimum contribution.
fn grid_item_spans_flexible_track_on_physical_axis(
    container_style: &ComputedStyle,
    item_style: &ComputedStyle,
    physical_axis: GridAxis,
) -> bool {
    let swaps_axes = WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes();
    let (start, end, tracks) = match (physical_axis, swaps_axes) {
        (GridAxis::Column, false) => (
            &item_style.grid_column_start,
            &item_style.grid_column_end,
            &container_style.grid_template_columns,
        ),
        (GridAxis::Row, false) => (
            &item_style.grid_row_start,
            &item_style.grid_row_end,
            &container_style.grid_template_rows,
        ),
        (GridAxis::Column, true) => (
            &item_style.grid_row_start,
            &item_style.grid_row_end,
            &container_style.grid_template_rows,
        ),
        (GridAxis::Row, true) => (
            &item_style.grid_column_start,
            &item_style.grid_column_end,
            &container_style.grid_template_columns,
        ),
    };
    grid_placement_explicit_span(start, end).is_some_and(|span| span > 1)
        && grid_track_list_has_flexible_track(tracks)
}

fn grid_placement_explicit_span(
    start: &css::GridPlacement,
    end: &css::GridPlacement,
) -> Option<u16> {
    match (start, end) {
        (css::GridPlacement::Line(_), css::GridPlacement::Span(span))
        | (css::GridPlacement::Span(span), css::GridPlacement::Line(_)) => span.count(),
        _ => None,
    }
}

fn grid_track_list_has_flexible_track(tracks: &css::GridTrackList) -> bool {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return false;
    };
    components
        .iter()
        .any(grid_track_component_has_flexible_track)
}

fn grid_track_component_has_flexible_track(component: &css::GridTrackListComponent) -> bool {
    match component {
        css::GridTrackListComponent::Track(_, track) => {
            matches!(track.max, css::GridMaxTrackBreadth::Flex(_))
        }
        css::GridTrackListComponent::Repeat(_, repeat) => repeat
            .tracks
            .iter()
            .any(grid_track_component_has_flexible_track),
    }
}

fn grid_gap_resolves_differently_with_basis(
    gap: css::ComputedGap,
    container_size: PhysicalContentHeight,
) -> bool {
    let css::ComputedGap::LengthPercentage(value) = gap else {
        return false;
    };
    let intrinsic = value.length_max_zero().points();
    let used = value
        .used_length_with_percentage_basis(PercentageBasis::definite(
            container_size.content_box_length(),
        ))
        .map(layout_points)
        .unwrap_or(value.length_points())
        .max(0.0);
    (used - intrinsic).abs() > 0.01
}

pub(super) struct GridLayoutPassConfig {
    pub(super) width: PhysicalContentWidth,
    pub(super) root_height: Option<PhysicalContentHeight>,
    /// Overrides the physical-width percentage basis for an intrinsic sizing
    /// pass. Grid Lanes uses this while probing a column auto-repeat so item
    /// percentages cannot feed the container width back into track sizing.
    pub(super) item_width_basis: Option<GridPercentageBasis>,
    pub(super) item_height_basis: GridPercentageBasis,
    /// Definite physical containing-block sizes derived from resolved grid
    /// areas. These are used only by the bounded feedback and final-placement
    /// passes; the first intrinsic pass retains its cyclic bases.
    pub(super) item_containing_block_bases: Option<Vec<GridItemContainingBlockBases>>,
    /// Physical track axes held fixed while CSS Grid performs one of its
    /// bounded cross-axis intrinsic-contribution corrections.
    pub(super) frozen_tracks: GridFrozenTrackTopology,
    pub(super) row_gap_basis: GridPercentageBasis,
    pub(super) reported_height: Option<PhysicalContentHeight>,
    /// Resolved real-item areas retained while sizing-only descendant proxy
    /// leaves are included in a second parent track-sizing pass.
    pub(super) item_placement_overrides: Vec<Option<GridItemArea>>,
    /// Baseline shims derived from the placement topology. These affect only
    /// Taffy's intrinsic Grid sizing margins, never the replayed item style.
    pub(super) baseline_plan: Option<GridBaselinePlan>,
}

/// A placed grid item's physical containing-block percentage bases.
///
/// Grid item layout is performed in its grid area, not the grid container;
/// preserving both physical axes prevents an orthogonal item from resolving a
/// block percentage against the container's unrelated dimension.
/// <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
#[derive(Debug, Clone, Copy)]
pub(super) struct GridItemContainingBlockBases {
    width: GridPercentageBasis,
    height: GridPercentageBasis,
}

/// Resolved physical tracks retained while correcting the opposite logical
/// axis. Taffy's Grid interface does not expose an axis-only sizing entry
/// point, so the fixed axis is represented as exact fixed track functions.
#[derive(Debug, Clone, Default)]
pub(super) struct GridFrozenTrackTopology {
    columns: Option<Vec<f32>>,
    rows: Option<Vec<f32>>,
}

/// Derive the physical percentage containing blocks from resolved grid-area
/// boundaries. The line offsets include all crossed effective gutters.
fn grid_item_containing_block_bases(
    style: &ComputedStyle,
    layout: &GridLayout,
) -> Vec<GridItemContainingBlockBases> {
    let swaps_axes =
        WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes();
    let width_source = if swaps_axes {
        GridAvailableSizeSource::GridItemContainingBlockBlock
    } else {
        GridAvailableSizeSource::GridItemContainingBlockInline
    };
    let height_source = if swaps_axes {
        GridAvailableSizeSource::GridItemContainingBlockInline
    } else {
        GridAvailableSizeSource::GridItemContainingBlockBlock
    };
    layout
        .items
        .iter()
        .map(|item| {
            let Some(area) = item.area else {
                return GridItemContainingBlockBases {
                    width: GridPercentageBasis::indefinite(),
                    height: GridPercentageBasis::indefinite(),
                };
            };
            GridItemContainingBlockBases {
                width: grid_percentage_basis(
                    grid_area_axis_extent(
                        &layout.column_line_offsets,
                        area.column_start,
                        area.column_end,
                    )
                    .map(content_box_pt),
                    width_source,
                ),
                height: grid_percentage_basis(
                    grid_area_axis_extent(&layout.row_line_offsets, area.row_start, area.row_end)
                        .map(content_box_pt),
                    height_source,
                ),
            }
        })
        .collect()
}

fn grid_area_axis_extent(line_offsets: &[f32], start_line: u16, end_line: u16) -> Option<f32> {
    let start = line_offsets.get(usize::from(start_line).checked_sub(1)?)?;
    let end = line_offsets.get(usize::from(end_line).checked_sub(1)?)?;
    Some((end - start).max(0.0))
}

fn grid_item_placement_overrides(layout: &GridLayout) -> Vec<Option<GridItemArea>> {
    layout.items.iter().map(|item| item.area).collect()
}

/// CSS Grid's feedback decision is based on the relevant logical min-content
/// contribution. The corresponding max-content value is nevertheless passed
/// through the correction once the min-content value changed.
fn grid_inline_contributions_changed(
    previous: &[GridItemEstimate],
    next: &[GridItemEstimate],
) -> bool {
    previous.iter().zip(next).any(|(previous, next)| {
        (previous.min_width.points() - next.min_width.points()).abs() > 0.01
    })
}

fn grid_block_contributions_changed(
    previous: &[GridItemEstimate],
    next: &[GridItemEstimate],
) -> bool {
    previous.iter().zip(next).any(|(previous, next)| {
        (previous.min_height.points() - next.min_height.points()).abs() > 0.01
    })
}

/// Convert a preliminary physical Grid area into a fixed Taffy placement for
/// the proxy sizing pass. The area was produced by the preceding placement
/// pass, so its lines are valid Taffy grid lines.
fn taffy_grid_area_line(
    area: GridItemArea,
    axis: GridAxis,
) -> Option<taffy_layout::Line<taffy_layout::GridPlacement<String>>> {
    let (start, end) = match axis {
        GridAxis::Column => (area.column_start, area.column_end),
        GridAxis::Row => (area.row_start, area.row_end),
    };
    Some(taffy_layout::Line {
        start: taffy_layout::line(i16::try_from(start).ok()?),
        end: taffy_layout::line(i16::try_from(end).ok()?),
    })
}

/// Freeze a resolved physical track topology for a bounded opposite-axis
/// correction pass.
fn taffy_fixed_grid_tracks(sizes: &[f32]) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    sizes
        .iter()
        .map(|size| {
            let size = size.max(0.0);
            taffy_layout::GridTemplateComponent::Single(taffy_layout::TrackSizingFunction {
                min: taffy_layout::MinTrackSizingFunction::length(size),
                max: taffy_layout::MaxTrackSizingFunction::length(size),
            })
        })
        .collect()
}

#[derive(Default)]
struct GridTrackLayoutCorrections {
    columns: Option<GridTrackLayoutCorrection>,
    rows: Option<GridTrackLayoutCorrection>,
}

struct GridTrackLayoutCorrection {
    original_offsets: Vec<f32>,
    offsets: Vec<f32>,
    sizes: Vec<f32>,
    gutters: Vec<f32>,
}

/// Collapse empty frozen `auto-fit` tracks after startward implicit expansion.
///
/// Quire freezes a definite auto-repeat count before prepending startward
/// implicit tracks so the authored repeat count is not recomputed from the
/// enlarged Taffy template. CSS Grid still requires empty `auto-fit` repeated
/// tracks to collapse before content alignment, so Quire mirrors that part of
/// the used-track geometry after Taffy returns detailed track data:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn startward_auto_fit_track_correction(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
    sizes: &[f32],
    gutters: &[f32],
    item_areas: &[GridItemArea],
) -> Option<GridTrackLayoutCorrection> {
    let range = startward_adjusted_auto_fit_track_range(style, axis, adjustment)?;
    if range.end > sizes.len() {
        return None;
    }
    let mut collapsed = vec![false; sizes.len()];
    for track_index in range {
        if !grid_track_has_item(axis, track_index, item_areas) {
            collapsed[track_index] = true;
        }
    }
    if !collapsed.iter().any(|collapsed| *collapsed) {
        return None;
    }
    let mut corrected_sizes = sizes.to_vec();
    for (index, size) in corrected_sizes.iter_mut().enumerate() {
        if collapsed[index] {
            *size = 0.0;
        }
    }
    let mut corrected_gutters = gutters.to_vec();
    for (index, gutter) in corrected_gutters.iter_mut().enumerate() {
        if collapsed.get(index).cloned().unwrap_or(false)
            || collapsed.get(index + 1).cloned().unwrap_or(false)
        {
            *gutter = 0.0;
        }
    }
    Some(GridTrackLayoutCorrection {
        original_offsets: grid_line_offsets_from_track_layout(sizes, gutters),
        offsets: grid_line_offsets_from_track_layout(&corrected_sizes, &corrected_gutters),
        sizes: corrected_sizes,
        gutters: corrected_gutters,
    })
}

fn grid_track_has_item(axis: GridAxis, track_index: usize, item_areas: &[GridItemArea]) -> bool {
    item_areas.iter().any(|area| {
        let (start, end) = match axis {
            GridAxis::Column => (usize::from(area.column_start), usize::from(area.column_end)),
            GridAxis::Row => (usize::from(area.row_start), usize::from(area.row_end)),
        };
        let start = start.saturating_sub(1);
        let end = end.saturating_sub(1);
        start <= track_index && track_index < end
    })
}

fn apply_startward_auto_fit_track_corrections(
    style: &ComputedStyle,
    container_width: PhysicalContentWidth,
    container_height: f32,
    corrections: &GridTrackLayoutCorrections,
    items: &mut [GridItemLayout],
) {
    for item in items {
        let Some(area) = item.area else {
            continue;
        };
        if let Some(correction) = &corrections.columns {
            apply_track_layout_correction_axis(
                correction,
                style.justify_content,
                container_width.points(),
                usize::from(area.column_start).saturating_sub(1),
                usize::from(area.column_end).saturating_sub(1),
                item,
                GridAxis::Column,
            );
        }
        if let Some(correction) = &corrections.rows {
            apply_track_layout_correction_axis(
                correction,
                style.align_content,
                container_height,
                usize::from(area.row_start).saturating_sub(1),
                usize::from(area.row_end).saturating_sub(1),
                item,
                GridAxis::Row,
            );
        }
    }
}

fn apply_track_layout_correction_axis(
    correction: &GridTrackLayoutCorrection,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    start_line: usize,
    end_line: usize,
    item: &mut GridItemLayout,
    axis: GridAxis,
) {
    let Some(original_start) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.original_offsets,
        start_line,
    ) else {
        return;
    };
    let Some(original_end) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.original_offsets,
        end_line,
    ) else {
        return;
    };
    let Some(corrected_start) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.offsets,
        start_line,
    ) else {
        return;
    };
    let Some(corrected_end) = content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &correction.offsets,
        end_line,
    ) else {
        return;
    };
    let original_area_size = (original_end - original_start).max(0.0);
    let corrected_area_size = (corrected_end - corrected_start).max(0.0);
    let original_track_start = correction.original_offsets[start_line];
    let original_track_area_size =
        (correction.original_offsets[end_line] - original_track_start).max(0.0);
    let offset_in_area = if (item.axis_size(axis) - original_track_area_size).abs() >= 0.01
        && matches!(
            content_alignment.keyword,
            css::ContentAlignmentKeyword::SpaceBetween
                | css::ContentAlignmentKeyword::SpaceAround
                | css::ContentAlignmentKeyword::SpaceEvenly
        ) {
        // Distributed alignment re-centres a fixed-size item when collapsed
        // tracks shorten its source area. Positional alignment preserves the
        // Taffy offset, including the negative overflow shift of a span that
        // fills the area.
        item.axis_start(axis) - original_track_start
            + (corrected_area_size - original_track_area_size) / 2.0
    } else {
        item.axis_start(axis) - original_start
    };
    let mut size = item.axis_size(axis);
    if (size - original_area_size).abs() < 0.01 {
        size = corrected_area_size;
    }
    item.set_axis_geometry(axis, corrected_start + offset_in_area, size);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_physical_height_drives_percentage_gap_relayout() {
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.5));
        let height = PhysicalContentHeight::new(content_box_pt(80.0));

        assert!(grid_gap_resolves_differently_with_basis(gap, height));
    }

    fn track(min: css::GridMinTrackBreadth, max: css::GridMaxTrackBreadth) -> css::GridTrackSize {
        css::GridTrackSize { min, max }
    }

    fn grid_tracks(tracks: Vec<css::GridTrackSize>) -> css::GridTrackList {
        css::GridTrackList::Tracks {
            components: tracks
                .into_iter()
                .map(|track| css::GridTrackListComponent::Track(Vec::new(), track))
                .collect(),
            trailing_names: Vec::new(),
        }
    }

    fn feedback_test_layout(area: GridItemArea) -> GridLayout {
        GridLayout {
            height: PhysicalContentHeight::new(content_box_pt(80.0)),
            baselines: PhysicalBaselineSets::default(),
            first_baseline: None,
            last_baseline: None,
            items: vec![GridItemLayout::new(
                GridRect::new(GridPoint::new(0.0, 0.0), GridSize::new(0.0, 0.0)),
                Some(area),
            )],
            baseline_resolutions: Vec::new(),
            gap_gutters: GapDecorationGridGutters::default(),
            // These offsets deliberately include unequal crossed gutters.
            column_line_offsets: vec![3.0, 13.0, 29.0, 51.0],
            row_line_offsets: vec![5.0, 19.0, 40.0, 68.0],
            column_line_names: Vec::new(),
            row_line_names: Vec::new(),
            column_track_sizes: vec![10.0, 12.0, 15.0],
            row_track_sizes: vec![14.0, 16.0, 20.0],
        }
    }

    #[test]
    fn grid_area_bases_include_spans_and_crossed_gutters() {
        let layout = feedback_test_layout(GridItemArea {
            row_start: 2,
            row_end: 4,
            column_start: 1,
            column_end: 3,
        });
        let bases = grid_item_containing_block_bases(&ComputedStyle::initial(), &layout);

        assert_eq!(bases.len(), 1);
        assert_eq!(bases[0].width.points(), Some(26.0));
        assert_eq!(bases[0].height.points(), Some(49.0));
        assert!(matches!(
            bases[0].width,
            PercentageBasis::Definite {
                source: GridAvailableSizeSource::GridItemContainingBlockInline,
                ..
            }
        ));
        assert!(matches!(
            bases[0].height,
            PercentageBasis::Definite {
                source: GridAvailableSizeSource::GridItemContainingBlockBlock,
                ..
            }
        ));
    }

    #[test]
    fn vertical_grid_area_bases_retain_physical_extents_and_swap_provenance() {
        let layout = feedback_test_layout(GridItemArea {
            row_start: 2,
            row_end: 4,
            column_start: 1,
            column_end: 3,
        });
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        let bases = grid_item_containing_block_bases(&style, &layout);

        assert_eq!(bases[0].width.points(), Some(26.0));
        assert_eq!(bases[0].height.points(), Some(49.0));
        assert!(matches!(
            bases[0].width,
            PercentageBasis::Definite {
                source: GridAvailableSizeSource::GridItemContainingBlockBlock,
                ..
            }
        ));
        assert!(matches!(
            bases[0].height,
            PercentageBasis::Definite {
                source: GridAvailableSizeSource::GridItemContainingBlockInline,
                ..
            }
        ));
    }

    #[test]
    fn grid_feedback_corrections_are_tolerance_bounded_and_axis_specific() {
        let initial = [GridItemEstimate::fixed(10.0, 20.0)];
        let within_tolerance = [GridItemEstimate::fixed(10.005, 20.005)];
        let corrected = [GridItemEstimate::fixed(10.02, 20.02)];

        assert!(!grid_inline_contributions_changed(
            &initial,
            &within_tolerance
        ));
        assert!(!grid_block_contributions_changed(
            &initial,
            &within_tolerance
        ));
        assert!(grid_inline_contributions_changed(&initial, &corrected));
        assert!(grid_block_contributions_changed(&initial, &corrected));
    }

    #[test]
    fn frozen_feedback_topology_preserves_placement_for_one_correction_per_axis() {
        let layout = feedback_test_layout(GridItemArea {
            row_start: 2,
            row_end: 3,
            column_start: 2,
            column_end: 4,
        });
        let placement = grid_item_placement_overrides(&layout);
        let columns = GridFrozenTrackTopology {
            columns: Some(layout.column_track_sizes.clone()),
            rows: None,
        };
        let rows = GridFrozenTrackTopology {
            columns: None,
            rows: Some(layout.row_track_sizes.clone()),
        };

        let preserved_area = placement[0].expect("placed item retains its grid area");
        assert_eq!(preserved_area.row_start, 2);
        assert_eq!(preserved_area.row_end, 3);
        assert_eq!(preserved_area.column_start, 2);
        assert_eq!(preserved_area.column_end, 4);
        assert_eq!(
            columns.columns.as_deref(),
            Some(layout.column_track_sizes.as_slice())
        );
        assert_eq!(
            rows.rows.as_deref(),
            Some(layout.row_track_sizes.as_slice())
        );
        assert_eq!(
            taffy_fixed_grid_tracks(columns.columns.as_deref().unwrap()).len(),
            3
        );
        assert_eq!(
            taffy_fixed_grid_tracks(rows.rows.as_deref().unwrap()).len(),
            3
        );
    }

    #[test]
    fn flexible_track_span_zeros_only_the_automatic_grid_minimum() {
        let mut container = ComputedStyle::initial();
        container.grid_template_columns = grid_tracks(vec![track(
            css::GridMinTrackBreadth::Auto,
            css::GridMaxTrackBreadth::Flex(1.0),
        )]);
        let mut item = ComputedStyle::initial();
        item.grid_column_start = css::GridPlacement::Line(css::GridLinePlacement::Number(
            std::num::NonZeroI32::new(1).unwrap(),
        ));
        item.grid_column_end = css::GridPlacement::Span(css::GridSpanPlacement::Count(
            std::num::NonZeroU16::new(2).unwrap(),
        ));

        assert_eq!(
            grid_item_taffy_min_dimension(
                css::ComputedLengthPercentageOrAuto::Auto,
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::Dimension::length(0.0),
        );
        assert_eq!(
            grid_item_taffy_min_dimension(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(12.0),
                ),
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::Dimension::length(12.0),
        );
    }
}
