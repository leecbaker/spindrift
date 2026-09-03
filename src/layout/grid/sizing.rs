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
    /// Compute same-page grid item geometry with Spindrift-measured leaf estimates.
    ///
    /// CSS Grid track sizing consumes each item's min-content, max-content, and
    /// preferred size contributions. Taffy owns the Grid Level 1 placement and
    /// track-sizing algorithm here, while Spindrift supplies leaf measurements from
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
                GridLayoutPurpose::FinalLayout | GridLayoutPurpose::FloatBlockSizeMeasurement => {
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
            GridLayoutPurpose::IntrinsicProbe | GridLayoutPurpose::FloatBlockSizeMeasurement => {
                self.resolved_subgrid_context_for_probe()
            }
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
        let preliminary_pass = self.compute_grid_layout_pass_with_estimates(
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
        let GridLayoutPassResult {
            layout: preliminary_layout,
            estimates: preliminary_estimates,
            feedback_sensitivities,
        } = preliminary_pass;
        let baseline_plan =
            self.grid_baseline_sizing_plan(style, &preliminary_layout, &preliminary_estimates);
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
                    baseline_plan: Some(&baseline_plan),
                },
            )?
        };
        let contributions = if purpose.uses_final_track_sizing() && subgrid_context.is_none() {
            self.collect_subgrid_contributions(style, children, stylesheets, &preliminary_layout)
        } else {
            Vec::new()
        };
        // Descendant contribution proxies take part in parent track sizing,
        // but they are not Grid items and must not displace automatic items
        // during the proxy pass. Preserve the preliminary placement while
        // resolving the shared inherited tracks.
        // <https://drafts.csswg.org/css-grid-2/#subgrid-contributions>
        let contribution_item_placement_overrides = (!contributions.is_empty())
            .then(|| grid_item_placement_overrides(style, &preliminary_layout));
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
                    baseline_plan: Some(&baseline_plan),
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
                &baseline_plan,
                &feedback_sensitivities,
                intrinsic_layout,
            )?
        };
        if height.is_none()
            && purpose.uses_final_track_sizing()
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
                    baseline_plan: Some(&baseline_plan),
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
        baseline_plan: &GridBaselinePlan,
        feedback_sensitivities: &[GridItemFeedbackSensitivity],
        initial_layout: GridLayout,
    ) -> Option<GridLayout> {
        let mut layout = initial_layout;
        let probe_plan = GridFeedbackProbePlan::from_sensitivities(feedback_sensitivities);
        let mut reusable_estimates = Vec::with_capacity(children.len());
        let initial_bases = probe_plan
            .needs_inline_comparison
            .then(|| grid_item_containing_block_bases(style, &layout));
        if let Some(initial_bases) = initial_bases.as_deref() {
            #[cfg(feature = "layout-profile")]
            let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
                crate::layout::layout_profile::GridFeedbackSweep::Initial,
                children.len(),
            );
            self.fill_grid_item_estimates_with_bases(
                &mut reusable_estimates,
                children,
                stylesheets,
                width,
                initial_bases,
            );
            #[cfg(feature = "layout-profile")]
            drop(_profile);
        }

        // Step 3: rows are now resolved, so an item's inline contribution may
        // change (notably through an aspect-ratio descendant). Re-resolve the
        // columns once while keeping rows and placement stable.
        let mut cyclic_estimates = Vec::with_capacity(children.len());
        if probe_plan.needs_container_comparison() {
            #[cfg(feature = "layout-profile")]
            let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
                crate::layout::layout_profile::GridFeedbackSweep::Container,
                children.len(),
            );
            self.fill_grid_item_estimates_with_container_bases(
                &mut cyclic_estimates,
                children,
                stylesheets,
                width,
                height,
            );
            #[cfg(feature = "layout-profile")]
            drop(_profile);
        }
        if probe_plan.needs_inline_comparison
            && grid_inline_contributions_changed(&cyclic_estimates, &reusable_estimates)
        {
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
                    item_containing_block_bases: initial_bases,
                    frozen_tracks: GridFrozenTrackTopology {
                        columns: None,
                        rows: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Row)),
                    },
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(layout.height),
                    item_placement_overrides: grid_item_placement_overrides(style, &layout),
                    baseline_plan: Some(baseline_plan),
                },
            )?;
        }

        // Step 4: the adjusted columns can in turn change a block-axis
        // contribution. Perform the symmetric row correction once only.
        let column_bases = probe_plan
            .needs_block_comparison
            .then(|| grid_item_containing_block_bases(style, &layout));
        if let Some(column_bases) = column_bases.as_deref() {
            #[cfg(feature = "layout-profile")]
            let _profile = crate::layout::layout_profile::grid_feedback_sweep_scope(
                crate::layout::layout_profile::GridFeedbackSweep::Column,
                children.len(),
            );
            self.fill_grid_item_estimates_with_bases(
                &mut reusable_estimates,
                children,
                stylesheets,
                width,
                column_bases,
            );
            #[cfg(feature = "layout-profile")]
            drop(_profile);
        }
        // The initial track layout measured each ordinary grid item against
        // the container-wide available space. Compare that original cyclic
        // contribution with the estimate constrained by the resolved column
        // area; comparing two area-constrained probes would silently skip a
        // required row correction for padded nested grids.
        // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
        if probe_plan.needs_block_comparison
            && grid_block_contributions_changed(&cyclic_estimates, &reusable_estimates)
        {
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
                    item_containing_block_bases: column_bases,
                    frozen_tracks: GridFrozenTrackTopology {
                        columns: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Column)),
                        rows: None,
                    },
                    row_gap_basis: grid_percentage_basis(
                        height.map(PhysicalContentHeight::content_box_length),
                        GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    reported_height: Some(layout.height),
                    item_placement_overrides: grid_item_placement_overrides(style, &layout),
                    baseline_plan: Some(baseline_plan),
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
                    columns: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Column)),
                    rows: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Row)),
                },
                row_gap_basis: grid_percentage_basis(
                    height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: Some(layout.height),
                item_placement_overrides: grid_item_placement_overrides(style, &layout),
                baseline_plan: Some(baseline_plan),
            },
        )?;
        Some(final_layout)
    }

    fn fill_grid_item_estimates_with_bases(
        &mut self,
        output: &mut Vec<GridItemEstimate>,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        bases: &[GridItemContainingBlockBases],
    ) {
        output.clear();
        output.extend(children.iter().zip(bases).map(|(child, bases)| {
            self.estimate_grid_item_size_for_parent_track_sizing(
                child,
                stylesheets,
                bases.width.points().unwrap_or(width.points()),
                bases.width,
                bases.height,
            )
        }));
    }

    fn fill_grid_item_estimates_with_container_bases(
        &mut self,
        output: &mut Vec<GridItemEstimate>,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        height: Option<PhysicalContentHeight>,
    ) {
        let width_basis = grid_percentage_basis(
            Some(width.content_box_length()),
            GridAvailableSizeSource::ContainerInlineSize,
        );
        let height_basis = grid_percentage_basis(
            height.map(PhysicalContentHeight::content_box_length),
            GridAvailableSizeSource::ContainerBlockSize,
        );
        output.clear();
        output.extend(children.iter().map(|child| {
            self.estimate_grid_item_size_for_parent_track_sizing(
                child,
                stylesheets,
                width.points(),
                width_basis,
                height_basis,
            )
        }));
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
        &self,
        style: &ComputedStyle,
        topology: &GridLayout,
        estimates: &[GridItemEstimate],
    ) -> GridBaselinePlan {
        if !grid_baseline_sizing_may_need_shims(
            style,
            &topology.baseline_resolutions,
            &topology.items,
        ) {
            return GridBaselinePlan::default();
        }
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::grid_baseline_plan_scope(estimates.len());
        grid_baseline_plan(
            style,
            estimates,
            &topology.baseline_resolutions,
            &topology.rows,
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
        config: GridLayoutPassConfig<'_>,
    ) -> Option<GridLayout> {
        self.compute_grid_layout_pass_with_estimates(
            style,
            children,
            stylesheets,
            subgrid_context,
            contributions,
            config,
        )
        .map(|pass| pass.layout)
    }

    fn compute_grid_layout_pass_with_estimates(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        subgrid_context: Option<&ResolvedSubgridContext>,
        contributions: &[SubgridContribution],
        config: GridLayoutPassConfig<'_>,
    ) -> Option<GridLayoutPassResult> {
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
        let writing_axes = WritingModeAxes::new(style.writing_mode, style.direction);
        let swaps_physical_grid_axes = writing_axes.swaps_physical_axes();
        // Taffy reverses its column sequence for horizontal RTL. Vertical
        // Grid projection remains within the separately documented incomplete
        // writing-mode surface and must not be inferred from coincident track
        // coordinates.
        let taffy_columns_are_reversed =
            !swaps_physical_grid_axes && writing_axes.is_reversed(LogicalAxis::Inline);
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
        let mut feedback_sensitivities = Vec::with_capacity(children.len());
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
            let measurement = self.measure_grid_item_size(
                child,
                stylesheets,
                child_available_width,
                child_width_basis,
                child_height_basis,
            );
            let estimate = measurement.estimate;
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
            feedback_sensitivities.push(measurement.sensitivity);
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
                            width: grid_item_taffy_min_constraint(
                                child.style.box_values.min_width.clone(),
                                GridAxis::Column,
                                style,
                                &child.style,
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: grid_item_taffy_min_constraint(
                                child.style.box_values.min_height.clone(),
                                GridAxis::Row,
                                style,
                                &child.style,
                                physical_estimate.min_height,
                                physical_estimate.content_height,
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_grid_item_constraint(
                                child.style.box_values.max_width.clone(),
                                GridPercentageBasis::indefinite(),
                                physical_estimate.min_width,
                                physical_estimate.content_width,
                            ),
                            height: taffy_grid_item_constraint(
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
        // entering Spindrift's returned item list. Auto-fit grids remain genuinely
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
            let zero_constraint = taffy_layout::LengthPercentageAuto::length(0.0);
            let placeholder = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        min_size: taffy_layout::Size {
                            width: zero_constraint,
                            height: zero_constraint,
                        },
                        max_size: taffy_layout::Size {
                            width: zero_constraint,
                            height: zero_constraint,
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
                        .as_ref()
                        .map(|axis| {
                            let mut sizes = axis.topology.track_sizes();
                            if !swaps_physical_grid_axes && style.used_direction() == Direction::Rtl
                            {
                                sizes.reverse();
                            }
                            taffy_fixed_grid_tracks(&sizes)
                        })
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
                        .as_ref()
                        .map(|axis| taffy_fixed_grid_tracks(&axis.topology.track_sizes()))
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
            |input, _node_id, node_context, _style| {
                let estimate = node_context.map(|context| match context {
                    GridTaffyLeaf::Item(estimate) | GridTaffyLeaf::Contribution(estimate) => {
                        estimate
                    }
                });
                taffy_grid_measurement(input, estimate)
            },
        )
        .ok()?;
        let root_layout = tree.layout(root).ok()?;
        let mut grid_item_areas = Vec::new();
        let mut columns = GridAxisTopology::default();
        let mut rows = GridAxisTopology::default();
        let mut taffy_physical_columns = GridAxisTopology::default();
        let mut taffy_physical_rows = GridAxisTopology::default();
        let mut track_corrections = GridAxisCorrections::default();
        match tree.detailed_layout_info(root) {
            taffy::tree::DetailedLayoutInfo::Grid(info) => {
                // Taffy 0.14 exposes final logical track positions rather
                // than the former separate size/gutter arrays.  Preserve
                // Spindrift's canonical topology by deriving a track extent and
                // each interior gutter at this sole backend boundary.
                let physical_column_gap = physical_column_subgrid.map_or_else(
                    || {
                        taffy_bridge::resolved_gap(
                            if swaps_physical_grid_axes {
                                style.row_gap.clone()
                            } else {
                                style.column_gap.clone()
                            },
                            item_width_basis,
                        )
                    },
                    |axis| axis.taffy_gap(),
                );
                let physical_row_gap = physical_row_subgrid.map_or_else(
                    || {
                        taffy_bridge::resolved_gap(
                            if swaps_physical_grid_axes {
                                style.column_gap.clone()
                            } else {
                                style.row_gap.clone()
                            },
                            row_gap_basis,
                        )
                    },
                    |axis| axis.taffy_gap(),
                );
                let (taffy_column_sizes, taffy_column_gutters) =
                    taffy_grid_track_layout_from_positions(
                        &info.columns.positions,
                        physical_column_gap,
                    );
                let (taffy_row_sizes, taffy_row_gutters) =
                    taffy_grid_track_layout_from_positions(&info.rows.positions, physical_row_gap);
                taffy_physical_columns = physical_grid_topology_from_taffy_positions(
                    &info.columns.positions,
                    taffy_columns_are_reversed,
                )?;
                taffy_physical_rows =
                    physical_grid_topology_from_taffy_positions(&info.rows.positions, false)?;
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
                // A frozen feedback axis carries the corrected auto-fit
                // topology from its preceding pass. Its later semantic
                // correction rebases the fixed Taffy result against that
                // topology, so applying a fresh startward correction here
                // would distribute the same collapse twice.
                let column_correction = config
                    .frozen_tracks
                    .columns
                    .is_none()
                    .then(|| {
                        startward_auto_fit_track_correction(
                            style,
                            GridAxis::Column,
                            &column_adjustment,
                            &taffy_column_sizes,
                            &taffy_column_gutters,
                            &grid_item_areas,
                        )
                    })
                    .flatten();
                let row_correction = config
                    .frozen_tracks
                    .rows
                    .is_none()
                    .then(|| {
                        startward_auto_fit_track_correction(
                            style,
                            GridAxis::Row,
                            &row_adjustment,
                            &taffy_row_sizes,
                            &taffy_row_gutters,
                            &grid_item_areas,
                        )
                    })
                    .flatten();
                let column_sizes = column_correction
                    .as_ref()
                    .map(|correction| correction.target.track_sizes())
                    .unwrap_or_else(|| taffy_column_sizes.clone());
                let column_gutters = column_correction
                    .as_ref()
                    .map(|correction| correction.target.interior_gutters())
                    .unwrap_or_else(|| taffy_column_gutters.clone());
                let row_sizes = row_correction
                    .as_ref()
                    .map(|correction| correction.target.track_sizes())
                    .unwrap_or_else(|| taffy_row_sizes.clone());
                let row_gutters = row_correction
                    .as_ref()
                    .map(|correction| correction.target.interior_gutters())
                    .unwrap_or_else(|| taffy_row_gutters.clone());
                let column_collapsed_tracks = auto_fit_collapsed_track_mask(
                    style,
                    GridAxis::Column,
                    &column_adjustment,
                    column_sizes.len(),
                    &grid_item_areas,
                );
                let row_collapsed_tracks = auto_fit_collapsed_track_mask(
                    style,
                    GridAxis::Row,
                    &row_adjustment,
                    row_sizes.len(),
                    &grid_item_areas,
                );
                // Both Taffy's detailed record and the startward correction
                // retain synthetic boundary gutters. Canonical Grid topology
                // retains only interior gutters, so normalize exactly once at
                // this backend boundary before any frozen or paint consumer
                // can observe the axis.
                let column_track_gutters =
                    taffy_grid_track_gutters(&column_gutters, column_sizes.len());
                let row_track_gutters = taffy_grid_track_gutters(&row_gutters, row_sizes.len());
                columns = column_correction.as_ref().map_or_else(
                    || {
                        GridAxisTopology::from_auto_fit_track_layout(
                            column_sizes,
                            column_track_gutters,
                            column_collapsed_tracks,
                        )
                    },
                    |correction| Some(correction.target.clone()),
                )?;
                rows = row_correction.as_ref().map_or_else(
                    || {
                        GridAxisTopology::from_auto_fit_track_layout(
                            row_sizes,
                            row_track_gutters,
                            row_collapsed_tracks,
                        )
                    },
                    |correction| Some(correction.target.clone()),
                )?;
                track_corrections = GridAxisCorrections {
                    columns: column_correction,
                    rows: row_correction,
                };
            }
            taffy::tree::DetailedLayoutInfo::None => {}
        };
        // Taffy is used to resolve placement topology, but it has no subgrid
        // model. Once placement is known, inherited axes must retain the
        // parent-owned track and gutter geometry exactly.
        // <https://www.w3.org/TR/css-grid-2/#subgrids>
        if let Some(axis) = physical_column_subgrid {
            let track_sizes: Vec<f32> = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            columns = GridAxisTopology::from_line_offsets(
                axis.line_offsets().to_vec(),
                track_sizes.clone(),
                vec![false; track_sizes.len()],
            )?;
        }
        if let Some(axis) = physical_row_subgrid {
            let track_sizes: Vec<f32> = axis
                .track_starts()
                .iter()
                .zip(axis.track_ends())
                .map(|(start, end)| (end - start).max(0.0))
                .collect();
            rows = GridAxisTopology::from_line_offsets(
                axis.line_offsets().to_vec(),
                track_sizes.clone(),
                vec![false; track_sizes.len()],
            )?;
        }
        // Taffy stores columns in backend-logical order even though their
        // final rectangles are already physical. Horizontal RTL therefore
        // crosses an explicit adapter boundary here: topology, collapse
        // provenance, corrections, and item placement ranges all become
        // increasing physical left-to-right order together.
        if taffy_columns_are_reversed {
            let column_track_count = columns.track_count();
            columns = columns.reversed();
            for area in &mut grid_item_areas {
                *area = area
                    .with_reversed_axis(GridAxis::Column, column_track_count)
                    .unwrap_or(*area);
            }
            if let Some(correction) = &mut track_corrections.columns {
                correction.source = correction.source.reversed();
                correction.target = correction.target.reversed();
            }
        }
        // Taffy's item rectangles are physical and use the detailed track
        // positions above. Whenever Spindrift canonicalizes collapsed auto-fit
        // geometry, retain that observed topology as the correction source so
        // item offsets can be rebased onto the canonical physical topology.
        // This also captures Taffy's content-alignment distribution without
        // trying to reconstruct it from a single generic line-offset list.
        if columns.has_collapsed_auto_fit_tracks() && track_corrections.columns.is_none() {
            track_corrections.columns = Some(GridAxisCorrection {
                source: CorrectionSource::Aligned(taffy_physical_columns),
                target: columns.clone(),
                preserved_item_geometry: None,
            });
        }
        if rows.has_collapsed_auto_fit_tracks() && track_corrections.rows.is_none() {
            track_corrections.rows = Some(GridAxisCorrection {
                source: CorrectionSource::Aligned(taffy_physical_rows),
                target: rows.clone(),
                preserved_item_geometry: None,
            });
        }
        // Startward implicit expansion already produces an explicit corrected
        // track-layout record below. Retain that established source/target
        // correction until it can share the same area model; ordinary frozen
        // axes use the semantic collapsed topology directly.
        let frozen_column_correction = track_corrections
            .columns
            .is_none()
            .then(|| {
                config
                    .frozen_tracks
                    .columns
                    .as_ref()
                    .and_then(|axis| GridAxisCorrection::from_frozen(axis, &columns))
            })
            .flatten();
        let frozen_row_correction = track_corrections
            .rows
            .is_none()
            .then(|| {
                config
                    .frozen_tracks
                    .rows
                    .as_ref()
                    .and_then(|axis| GridAxisCorrection::from_frozen(axis, &rows))
            })
            .flatten();
        if let Some(correction) = &frozen_column_correction {
            columns = correction.target.clone();
        }
        if let Some(correction) = &frozen_row_correction {
            rows = correction.target.clone();
        }
        let column_line_offsets = columns.line_offsets();
        let row_line_offsets = rows.line_offsets();
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
        apply_frozen_grid_axis_correction(
            frozen_column_correction.as_ref(),
            style.justify_content,
            content_width.points(),
            GridAxis::Column,
            style,
            Some(children),
            Some(&estimates),
            &mut items,
        );
        apply_frozen_grid_axis_correction(
            frozen_row_correction.as_ref(),
            style.align_content,
            root_layout.size.height,
            GridAxis::Row,
            style,
            Some(children),
            Some(&estimates),
            &mut items,
        );
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
        apply_auto_fit_track_corrections(
            style,
            content_width,
            final_grid_height,
            &track_corrections,
            frozen_column_correction.is_none(),
            frozen_row_correction.is_none(),
            &mut items,
        );
        // Taffy 0.14 stores Grid columns in logical order and assigns their
        // physical offsets right-to-left for `direction: rtl`.  Its item
        // rectangles are therefore already the CSS physical rectangles; do
        // not mirror them a second time before Spindrift's logical-only replay.
        apply_grid_self_alignment_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &columns,
            &rows,
            &mut items,
        );
        apply_grid_aspect_ratio_item_size_corrections(
            style,
            children,
            content_width,
            final_grid_height,
            &columns,
            &rows,
            &mut items,
        );
        apply_grid_replaced_item_size_corrections(style, children, &estimates, &mut items);
        apply_grid_deferred_percentage_item_size_corrections(
            GridFinalItemPercentagePlacement {
                container_style: style,
                container_width: content_width,
                container_height: final_grid_height,
                columns: &columns,
                rows: &rows,
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
            &rows,
            final_grid_height,
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
        Some(GridLayoutPassResult {
            layout: GridLayout {
                height: config.reported_height.unwrap_or_else(|| {
                    PhysicalContentHeight::new(content_box_pt(final_grid_height))
                }),
                baselines,
                first_baseline,
                last_baseline,
                items,
                baseline_resolutions,
                columns,
                rows,
                content_width: content_width.points(),
                content_height: final_grid_height,
                column_line_names,
                row_line_names,
            },
            estimates,
            feedback_sensitivities,
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
            layout.rows.track_sizes(),
            layout.columns.track_sizes(),
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
            layout.columns.track_sizes(),
            layout.rows.track_sizes(),
        )
    };
    let (inline_first, inline_last) =
        grid_margin_trim_edge_lines(&style.grid_template_columns, &inline_sizes, &inline_spans);
    let (block_first, block_last) =
        grid_margin_trim_edge_lines(&style.grid_template_rows, &block_sizes, &block_spans);

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
fn grid_item_taffy_min_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    physical_axis: GridAxis,
    container_style: &ComputedStyle,
    item_style: &ComputedStyle,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::LengthPercentageAuto {
    if value.is_auto()
        && grid_item_spans_flexible_track_on_physical_axis(
            container_style,
            item_style,
            physical_axis,
        )
    {
        return taffy_layout::LengthPercentageAuto::length(0.0);
    }
    taffy_grid_item_constraint(
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

pub(super) struct GridLayoutPassConfig<'a> {
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
    pub(super) baseline_plan: Option<&'a GridBaselinePlan>,
}

/// The ephemeral data produced by one Grid sizing pass.
///
/// Ordinary callers discard the item estimates with the pass, but baseline
/// shim construction consumes the preliminary pass's exact measurements.
struct GridLayoutPassResult {
    layout: GridLayout,
    estimates: Vec<GridItemEstimate>,
    feedback_sensitivities: Vec<GridItemFeedbackSensitivity>,
}

/// The subset of Grid's bounded feedback sequence that the preliminary item
/// measurements could still affect. The plan is proof-based: an omitted probe
/// is omitted only when every item reported the relevant contribution
/// invariant.
#[derive(Debug, Clone, Copy, Default)]
struct GridFeedbackProbePlan {
    needs_inline_comparison: bool,
    needs_block_comparison: bool,
}

impl GridFeedbackProbePlan {
    fn from_sensitivities(sensitivities: &[GridItemFeedbackSensitivity]) -> Self {
        Self {
            needs_inline_comparison: sensitivities
                .iter()
                .any(|sensitivity| sensitivity.inline_contribution_may_depend_on_area),
            needs_block_comparison: sensitivities
                .iter()
                .any(|sensitivity| sensitivity.block_contribution_may_depend_on_area),
        }
    }

    fn needs_container_comparison(self) -> bool {
        self.needs_inline_comparison || self.needs_block_comparison
    }
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
    columns: Option<GridFrozenTrackAxis>,
    rows: Option<GridFrozenTrackAxis>,
}

/// The final physical geometry retained while a feedback pass holds an axis
/// fixed. `collapsed_tracks` keeps CSS auto-fit topology that Taffy's fixed
/// length template cannot represent.
#[derive(Debug, Clone)]
pub(super) struct GridFrozenTrackAxis {
    topology: GridAxisTopology,
    item_geometry: Vec<FrozenGridItemAxisGeometry>,
}

/// One already-resolved physical Grid item axis retained across a bounded
/// opposite-axis feedback pass. The frozen pass must not re-run its
/// self-alignment against Taffy's lossy fixed-track representation.
#[derive(Debug, Clone, Copy)]
struct FrozenGridItemAxisGeometry {
    start: f32,
    size: f32,
}

impl GridFrozenTrackAxis {
    fn from_layout(layout: &GridLayout, axis: GridAxis) -> Self {
        Self {
            topology: layout.axis_topology(axis).clone(),
            item_geometry: layout
                .items
                .iter()
                .map(|item| FrozenGridItemAxisGeometry {
                    start: item.axis_start(axis),
                    size: item.axis_size(axis),
                })
                .collect(),
        }
    }

    fn has_collapsed_tracks(&self) -> bool {
        self.topology.has_collapsed_auto_fit_tracks()
    }
}

impl GridAxisCorrection {
    fn from_frozen(axis: &GridFrozenTrackAxis, source: &GridAxisTopology) -> Option<Self> {
        (axis.has_collapsed_tracks() && source.track_count() == axis.topology.track_count()).then(
            || Self {
                source: CorrectionSource::Unaligned(source.clone()),
                target: axis.topology.clone(),
                preserved_item_geometry: Some(axis.item_geometry.clone()),
            },
        )
    }
}

/// Rebase final Taffy item geometry from its fixed-track alignment subject to
/// the CSS auto-fit subject. Taffy cannot mark a fixed zero-length track as
/// collapsed, so it distributes space through every frozen track; CSS Grid
/// distributes only through the non-collapsed tracks.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>
#[allow(clippy::too_many_arguments)] // The two feedback axes share this exact placement adapter.
fn apply_frozen_grid_axis_correction(
    correction: Option<&GridAxisCorrection>,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    axis: GridAxis,
    container_style: &ComputedStyle,
    children: Option<&[GridChild<'_>]>,
    estimates: Option<&[GridItemEstimate]>,
    items: &mut [GridItemLayout],
) {
    let Some(correction) = correction else {
        return;
    };
    for (index, item) in items.iter_mut().enumerate() {
        if let Some(geometry) = correction
            .preserved_item_geometry
            .as_deref()
            .and_then(|geometry| geometry.get(index))
        {
            item.set_axis_geometry(axis, geometry.start, geometry.size);
            continue;
        }
        let Some(area) = item.area else {
            continue;
        };
        let (start_line, end_line) = match axis {
            GridAxis::Column => (area.column_start, area.column_end),
            GridAxis::Row => (area.row_start, area.row_end),
        };
        let Some((source_start, source_end)) =
            correction
                .source
                .area_bounds(content_alignment, container_size, start_line, end_line)
        else {
            continue;
        };
        let Some((target_start, target_end)) = correction.target.aligned_area_bounds(
            content_alignment,
            container_size,
            start_line,
            end_line,
        ) else {
            continue;
        };
        let source_area_size = (source_end - source_start).max(0.0);
        let target_area_size = (target_end - target_start).max(0.0);
        let size = item.axis_size(axis);
        let source_offset = item.axis_start(axis) - source_start;
        let source_free_space = source_area_size - size;
        let target_free_space = target_area_size - size;
        let stretches = children
            .zip(estimates)
            .and_then(|(children, estimates)| children.get(index).zip(estimates.get(index)))
            .map(|(child, estimate)| {
                frozen_grid_item_stretches_axis(container_style, child, estimate, axis)
            })
            // Unit-level topology checks have no item styles. Their source
            // geometry intentionally describes an already-stretched item.
            .unwrap_or((size - source_area_size).abs() < 0.01);
        let (start, size) = if stretches {
            (target_start, target_area_size)
        } else if source_free_space.abs() >= 0.01 {
            (
                target_start + source_offset / source_free_space * target_free_space,
                size,
            )
        } else {
            // A zero-free-space source cannot reveal a self-alignment
            // fraction. Preserve its start-side offset; later Spindrift-owned
            // self-alignment corrections handle writing-mode-specific cases.
            (target_start + source_offset, size)
        };
        item.set_axis_geometry(axis, start, size);
    }
}

/// Whether the final grid-area sizing step stretches this physical item axis.
/// A frozen pass has already converted collapsed tracks to numeric zeroes, so
/// its reported source area can be wider than the actual CSS area. Determine
/// stretch from the item style instead of inferring it from that source size.
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
fn frozen_grid_item_stretches_axis(
    container_style: &ComputedStyle,
    child: &GridChild<'_>,
    estimate: &GridItemEstimate,
    axis: GridAxis,
) -> bool {
    let swaps_axes = WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes();
    let (alignment, size_is_auto) = match (axis, swaps_axes) {
        (GridAxis::Column, false) => (
            effective_grid_justify_self(&child.style, container_style).keyword,
            child.style.box_values.width.is_auto(),
        ),
        (GridAxis::Row, false) => (
            effective_grid_align_self(&child.style, container_style).keyword,
            child.style.box_values.height.value().is_auto(),
        ),
        (GridAxis::Column, true) => (
            effective_grid_align_self(&child.style, container_style).keyword,
            child.style.box_values.width.is_auto(),
        ),
        (GridAxis::Row, true) => (
            effective_grid_justify_self(&child.style, container_style).keyword,
            child.style.box_values.height.value().is_auto(),
        ),
    };
    if !size_is_auto {
        return false;
    }
    if alignment == SelfAlignmentKeyword::Stretch {
        return true;
    }
    alignment == SelfAlignmentKeyword::Normal
        && estimate.replaced_used_size.is_none()
        && child
            .style
            .aspect_ratio
            .preferred_ratio_for_non_replaced(false)
            .is_none()
}

/// Convert Taffy 0.14's final track positions into Spindrift's canonical track
/// extents and interior gutters.
///
/// The positions include content alignment. Spindrift's subsequent static-position
/// and replay paths deliberately apply content distribution from the canonical
/// unaligned topology, so retain Taffy's used track sizes but the pre-alignment
/// CSS gutter at this boundary.
fn taffy_grid_track_layout_from_positions(
    positions: &[taffy_layout::Line<f32>],
    gap: f32,
) -> (Vec<f32>, Vec<f32>) {
    let sizes = positions
        .iter()
        .map(|track| (track.end - track.start).max(0.0))
        .collect();
    let gutters = vec![gap.max(0.0); positions.len().saturating_sub(1)];
    (sizes, gutters)
}

/// Preserve Taffy's already-physical track bounds while converting its track
/// vector from backend-logical to increasing physical order.
fn physical_grid_topology_from_taffy_positions(
    positions: &[taffy_layout::Line<f32>],
    reversed: bool,
) -> Option<GridAxisTopology> {
    if reversed {
        GridAxisTopology::from_track_geometry(
            positions
                .iter()
                .rev()
                .map(|track| (track.start, track.end, false)),
        )
    } else {
        GridAxisTopology::from_track_geometry(
            positions
                .iter()
                .map(|track| (track.start, track.end, false)),
        )
    }
}

/// Convert Taffy's historical alternating grid-track representation to the
/// interior gutters used by Spindrift's line geometry. Taffy 0.14 already yields
/// interior gutters through `taffy_grid_track_layout_from_positions`, while
/// this normalizer retains compatibility with frozen topology corrections.
fn taffy_grid_track_gutters(taffy_gutters: &[f32], track_count: usize) -> Vec<f32> {
    let interior_gutter_count = track_count.saturating_sub(1);
    if taffy_gutters.len() == track_count.saturating_add(1) {
        taffy_gutters
            .iter()
            .skip(1)
            .take(interior_gutter_count)
            .copied()
            .collect()
    } else {
        // Preserve compatibility with the older detailed-layout shape, which
        // already exposed only interior gutters.
        taffy_gutters
            .iter()
            .take(interior_gutter_count)
            .copied()
            .collect()
    }
}

fn auto_fit_collapsed_track_mask(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
    track_count: usize,
    item_areas: &[GridItemArea],
) -> Vec<bool> {
    let mut collapsed = vec![false; track_count];
    let Some(range) = auto_fit_track_range_with_startward_adjustment(style, axis, adjustment)
    else {
        return collapsed;
    };
    for track_index in range.filter(|index| *index < track_count) {
        collapsed[track_index] = !grid_track_has_item(axis, track_index, item_areas);
    }
    collapsed
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
                    grid_area_axis_extent(&layout.columns, area.column_start, area.column_end)
                        .map(content_box_pt),
                    width_source,
                ),
                height: grid_percentage_basis(
                    grid_area_axis_extent(&layout.rows, area.row_start, area.row_end)
                        .map(content_box_pt),
                    height_source,
                ),
            }
        })
        .collect()
}

fn grid_area_axis_extent(
    topology: &GridAxisTopology,
    start_line: u16,
    end_line: u16,
) -> Option<f32> {
    let (start, end) = topology.area_bounds(start_line, end_line)?;
    Some((end - start).max(0.0))
}

fn grid_item_placement_overrides(
    style: &ComputedStyle,
    layout: &GridLayout,
) -> Vec<Option<GridItemArea>> {
    let reverse_columns = !WritingModeAxes::new(style.writing_mode, style.used_direction())
        .swaps_physical_axes()
        && style.used_direction() == Direction::Rtl;
    let column_track_count = layout.columns.track_count();
    layout
        .items
        .iter()
        .map(|item| {
            item.area.and_then(|area| {
                if reverse_columns {
                    area.with_reversed_axis(GridAxis::Column, column_track_count)
                } else {
                    Some(area)
                }
            })
        })
        .collect()
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
struct GridAxisCorrections {
    columns: Option<GridAxisCorrection>,
    rows: Option<GridAxisCorrection>,
}

struct GridAxisCorrection {
    source: CorrectionSource,
    target: GridAxisTopology,
    preserved_item_geometry: Option<Vec<FrozenGridItemAxisGeometry>>,
}

enum CorrectionSource {
    Aligned(GridAxisTopology),
    Unaligned(GridAxisTopology),
}

impl CorrectionSource {
    fn reversed(&self) -> Self {
        match self {
            Self::Aligned(topology) => Self::Aligned(topology.reversed()),
            Self::Unaligned(topology) => Self::Unaligned(topology.reversed()),
        }
    }

    fn area_bounds(
        &self,
        content_alignment: css::ContentAlignment,
        container_size: f32,
        start_line: u16,
        end_line: u16,
    ) -> Option<(f32, f32)> {
        match self {
            Self::Aligned(topology) => topology.area_bounds(start_line, end_line),
            Self::Unaligned(topology) => topology.aligned_area_bounds(
                content_alignment,
                container_size,
                start_line,
                end_line,
            ),
        }
    }
}

/// Collapse empty frozen `auto-fit` tracks after startward implicit expansion.
///
/// Spindrift freezes a definite auto-repeat count before prepending startward
/// implicit tracks so the authored repeat count is not recomputed from the
/// enlarged Taffy template. CSS Grid still requires empty `auto-fit` repeated
/// tracks to collapse before content alignment, so Spindrift mirrors that part of
/// the used-track geometry after Taffy returns detailed track data:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn startward_auto_fit_track_correction(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
    sizes: &[f32],
    gutters: &[f32],
    item_areas: &[GridItemArea],
) -> Option<GridAxisCorrection> {
    if !adjustment.has_startward_tracks() {
        return None;
    }
    let range = auto_fit_track_range_with_startward_adjustment(style, axis, adjustment)?;
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
    Some(GridAxisCorrection {
        source: CorrectionSource::Unaligned(GridAxisTopology::from_track_layout(
            sizes.to_vec(),
            gutters.to_vec(),
            vec![false; sizes.len()],
        )?),
        target: GridAxisTopology::from_auto_fit_track_layout(
            corrected_sizes,
            gutters.to_vec(),
            collapsed,
        )?,
        preserved_item_geometry: None,
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

fn apply_auto_fit_track_corrections(
    style: &ComputedStyle,
    container_width: PhysicalContentWidth,
    container_height: f32,
    corrections: &GridAxisCorrections,
    apply_column_correction: bool,
    apply_row_correction: bool,
    items: &mut [GridItemLayout],
) {
    for item in items {
        let Some(area) = item.area else {
            continue;
        };
        if apply_column_correction && let Some(correction) = &corrections.columns {
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
        if apply_row_correction && let Some(correction) = &corrections.rows {
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
    correction: &GridAxisCorrection,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    start_line: usize,
    end_line: usize,
    item: &mut GridItemLayout,
    axis: GridAxis,
) {
    let Ok(start_line) = u16::try_from(start_line + 1) else {
        return;
    };
    let Ok(end_line) = u16::try_from(end_line + 1) else {
        return;
    };
    let original_bounds =
        correction
            .source
            .area_bounds(content_alignment, container_size, start_line, end_line);
    let Some((original_start, original_end)) = original_bounds else {
        return;
    };
    let Some((corrected_start, corrected_end)) = correction.target.aligned_area_bounds(
        content_alignment,
        container_size,
        start_line,
        end_line,
    ) else {
        return;
    };
    let original_area_size = (original_end - original_start).max(0.0);
    let corrected_area_size = (corrected_end - corrected_start).max(0.0);
    let offset_in_area = if (item.axis_size(axis) - original_area_size).abs() >= 0.01
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
        item.axis_start(axis) - original_start + (corrected_area_size - original_area_size) / 2.0
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
            // These topologies deliberately include unequal crossed gutters.
            columns: GridAxisTopology::from_line_offsets(
                vec![3.0, 13.0, 29.0, 51.0],
                vec![10.0, 12.0, 15.0],
                vec![false; 3],
            )
            .unwrap(),
            rows: GridAxisTopology::from_line_offsets(
                vec![5.0, 19.0, 40.0, 68.0],
                vec![14.0, 16.0, 20.0],
                vec![false; 3],
            )
            .unwrap(),
            content_width: 48.0,
            content_height: 63.0,
            column_line_names: Vec::new(),
            row_line_names: Vec::new(),
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
        assert_eq!(bases[0].width.points(), Some(22.0));
        assert_eq!(bases[0].height.points(), Some(41.0));
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

        assert_eq!(bases[0].width.points(), Some(22.0));
        assert_eq!(bases[0].height.points(), Some(41.0));
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
        let placement = grid_item_placement_overrides(&ComputedStyle::initial(), &layout);
        let columns = GridFrozenTrackTopology {
            columns: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Column)),
            rows: None,
        };
        let rows = GridFrozenTrackTopology {
            columns: None,
            rows: Some(GridFrozenTrackAxis::from_layout(&layout, GridAxis::Row)),
        };

        let preserved_area = placement[0].expect("placed item retains its grid area");
        assert_eq!(preserved_area.row_start, 2);
        assert_eq!(preserved_area.row_end, 3);
        assert_eq!(preserved_area.column_start, 2);
        assert_eq!(preserved_area.column_end, 4);
        assert_eq!(
            columns
                .columns
                .as_ref()
                .map(|axis| axis.topology.track_sizes()),
            Some(layout.columns.track_sizes())
        );
        assert_eq!(
            rows.rows.as_ref().map(|axis| axis.topology.track_sizes()),
            Some(layout.rows.track_sizes())
        );
        assert_eq!(
            taffy_fixed_grid_tracks(&columns.columns.as_ref().unwrap().topology.track_sizes())
                .len(),
            3
        );
        assert_eq!(
            taffy_fixed_grid_tracks(&rows.rows.as_ref().unwrap().topology.track_sizes()).len(),
            3
        );
    }

    #[test]
    fn frozen_auto_fit_rebases_space_around_using_only_active_tracks() {
        let correction = GridAxisCorrection::from_frozen(
            &GridFrozenTrackAxis {
                topology: GridAxisTopology::from_track_layout(
                    vec![15.0, 15.0, 0.0, 0.0, 15.0, 0.0, 15.0, 0.0, 0.0, 0.0],
                    vec![0.0; 9],
                    vec![
                        false, false, true, true, false, true, false, true, true, true,
                    ],
                )
                .unwrap(),
                item_geometry: Vec::new(),
            },
            &GridAxisTopology::from_line_offsets(
                vec![
                    0.0, 15.0, 30.0, 30.0, 30.0, 45.0, 45.0, 60.0, 60.0, 60.0, 60.0,
                ],
                vec![15.0, 15.0, 0.0, 0.0, 15.0, 0.0, 15.0, 0.0, 0.0, 0.0],
                vec![false; 10],
            )
            .unwrap(),
        )
        .expect("collapsed auto-fit tracks need a frozen correction");
        let mut items = [
            GridItemLayout::new(
                GridRect::new(GridPoint::new(4.5, 0.0), GridSize::new(15.0, 15.0)),
                Some(GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                }),
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(28.5, 0.0), GridSize::new(15.0, 15.0)),
                Some(GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                }),
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(70.5, 0.0), GridSize::new(15.0, 15.0)),
                Some(GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 5,
                    column_end: 6,
                }),
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(103.5, 0.0), GridSize::new(15.0, 15.0)),
                Some(GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 7,
                    column_end: 8,
                }),
            ),
        ];

        apply_frozen_grid_axis_correction(
            Some(&correction),
            css::ContentAlignment::new(css::ContentAlignmentKeyword::SpaceAround),
            150.0,
            GridAxis::Column,
            &ComputedStyle::initial(),
            None,
            None,
            &mut items,
        );

        assert_eq!(
            items.iter().map(GridItemLayout::x).collect::<Vec<_>>(),
            vec![11.25, 48.75, 86.25, 123.75]
        );
    }

    #[test]
    fn frozen_topology_uses_canonical_auto_fit_gutter_geometry() {
        let collapsed_tracks = vec![false, true, false, false, true];
        let axis = GridFrozenTrackAxis {
            topology: GridAxisTopology::from_auto_fit_track_layout(
                vec![0.0, 0.0, 15.0, 15.0, 0.0],
                vec![7.0, 8.0, 9.0, 10.0],
                collapsed_tracks,
            )
            .unwrap(),
            item_geometry: Vec::new(),
        };
        assert_eq!(
            axis.topology.line_offsets(),
            vec![0.0, 7.0, 7.0, 31.0, 46.0, 46.0]
        );
    }

    #[test]
    fn taffy_gutter_adapter_discards_boundary_gutters() {
        assert_eq!(
            taffy_grid_track_gutters(&[0.0, 7.5, 0.0, 7.5, 0.0], 4),
            vec![7.5, 0.0, 7.5]
        );
        assert_eq!(taffy_grid_track_gutters(&[7.5, 0.0], 3), vec![7.5, 0.0]);
    }

    #[test]
    fn taffy_track_positions_preserve_rtl_ordered_track_and_gutter_geometry() {
        // Taffy reports physical final positions, including an RTL grid's
        // visual reversal. The bridge retains that exact track order and
        // derives only the interior gaps needed by Spindrift's logical replay.
        let positions = [
            taffy_layout::Line {
                start: 70.0,
                end: 100.0,
            },
            taffy_layout::Line {
                start: 55.0,
                end: 65.0,
            },
            taffy_layout::Line {
                start: 30.0,
                end: 50.0,
            },
        ];

        let (sizes, gutters) = taffy_grid_track_layout_from_positions(&positions, 5.0);

        assert_eq!(sizes, vec![30.0, 10.0, 20.0]);
        // Content alignment can expand the final physical distance between
        // RTL tracks. The topology retains the authored gutter and lets the
        // existing logical replay apply alignment once.
        assert_eq!(gutters, vec![5.0, 5.0]);

        let physical = physical_grid_topology_from_taffy_positions(&positions, true).unwrap();
        assert_eq!(physical.track_sizes(), vec![20.0, 10.0, 30.0]);
        assert_eq!(physical.interior_gutters(), vec![5.0, 5.0]);
        assert_eq!(physical.area_bounds(1, 2), Some((30.0, 50.0)));
        assert_eq!(physical.area_bounds(3, 4), Some((70.0, 100.0)));
    }

    #[test]
    fn frozen_topology_distributes_only_non_collapsed_tracks() {
        let offsets = vec![0.0, 22.5, 22.5, 45.0, 60.0, 60.0, 60.0, 60.0];
        let collapsed = vec![false, true, false, false, true, true, true];
        let positions = |keyword| {
            [0, 2, 3]
                .into_iter()
                .map(|line| {
                    content_aligned_grid_line_offset_with_collapsed_tracks(
                        css::ContentAlignment::new(keyword),
                        150.0,
                        &offsets,
                        line,
                        Some(&collapsed),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            positions(css::ContentAlignmentKeyword::SpaceBetween),
            vec![0.0, 67.5, 135.0]
        );
        assert_eq!(
            positions(css::ContentAlignmentKeyword::SpaceAround),
            vec![15.0, 67.5, 120.0]
        );
        assert_eq!(
            positions(css::ContentAlignmentKeyword::SpaceEvenly),
            vec![22.5, 67.5, 112.5]
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
            grid_item_taffy_min_constraint(
                css::ComputedLengthPercentageOrAuto::Auto,
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::LengthPercentageAuto::length(0.0),
        );
        assert_eq!(
            grid_item_taffy_min_constraint(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(12.0),
                ),
                GridAxis::Column,
                &container,
                &item,
                content_box_pt(20.0),
                content_box_pt(40.0),
            ),
            taffy_layout::LengthPercentageAuto::length(12.0),
        );
    }

    #[test]
    fn feedback_probe_plan_runs_only_the_required_comparisons() {
        let invariant = GridItemFeedbackSensitivity {
            inline_contribution_may_depend_on_area: false,
            block_contribution_may_depend_on_area: false,
        };
        let inline_only = GridItemFeedbackSensitivity {
            inline_contribution_may_depend_on_area: true,
            block_contribution_may_depend_on_area: false,
        };
        let block_only = GridItemFeedbackSensitivity {
            inline_contribution_may_depend_on_area: false,
            block_contribution_may_depend_on_area: true,
        };

        let none = GridFeedbackProbePlan::from_sensitivities(&[invariant]);
        assert!(!none.needs_inline_comparison);
        assert!(!none.needs_block_comparison);
        assert!(!none.needs_container_comparison());

        let inline = GridFeedbackProbePlan::from_sensitivities(&[invariant, inline_only]);
        assert!(inline.needs_inline_comparison);
        assert!(!inline.needs_block_comparison);
        assert!(inline.needs_container_comparison());

        let block = GridFeedbackProbePlan::from_sensitivities(&[invariant, block_only]);
        assert!(!block.needs_inline_comparison);
        assert!(block.needs_block_comparison);
        assert!(block.needs_container_comparison());
    }
}
