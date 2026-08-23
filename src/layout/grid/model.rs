use super::lanes::GridLanesItemPlacement;
use super::*;
use crate::layout::baseline::PhysicalBaselineSets;

#[derive(Debug, Clone)]
pub(super) struct GridLayout {
    pub(super) height: PhysicalContentHeight,
    /// Grid's exported baselines in physical content-box coordinates.  The
    /// legacy scalar pair below remains a temporary adapter for callers that
    /// are known to consume only horizontal writing-mode baselines.
    pub(super) baselines: PhysicalBaselineSets,
    pub(super) first_baseline: Option<f32>,
    pub(super) last_baseline: Option<f32>,
    pub(super) items: Vec<GridItemLayout>,
    pub(super) baseline_resolutions: Vec<GridBaselineResolution>,
    pub(super) gap_gutters: GapDecorationGridGutters,
    pub(super) column_line_offsets: Vec<f32>,
    pub(super) row_line_offsets: Vec<f32>,
    /// Final Grid line names in physical Taffy order. This carries inherited
    /// and locally-added subgrid names through nested replay.
    pub(super) column_line_names: Vec<css::GridLineNames>,
    pub(super) row_line_names: Vec<css::GridLineNames>,
    /// Used physical track sizes retained for edge-track consumers such as
    /// `margin-trim`; zero-sized auto-fit tracks are collapsed tracks.
    pub(super) column_track_sizes: Vec<f32>,
    pub(super) row_track_sizes: Vec<f32>,
}

impl GridLayout {
    /// Used physical track sizes reported by the shared Grid sizing pass.
    ///
    /// Grid Lanes uses these only while resolving its Level 3 intrinsic
    /// auto-repeat hypothesis, before it performs its distinct packing pass.
    pub(super) fn physical_track_sizes(&self, axis: GridAxis) -> &[f32] {
        match axis {
            GridAxis::Column => &self.column_track_sizes,
            GridAxis::Row => &self.row_track_sizes,
        }
    }

    /// Replace one physical axis with the final Grid Lanes topology before
    /// measuring packed children. This keeps subgrid probes and final replay
    /// on the same used tracks and line-name map.
    pub(super) fn set_physical_grid_axis_topology(
        &mut self,
        axis: GridAxis,
        line_offsets: Vec<f32>,
        track_sizes: Vec<f32>,
        line_names: Vec<css::GridLineNames>,
    ) {
        debug_assert_eq!(line_offsets.len(), track_sizes.len().saturating_add(1));
        debug_assert_eq!(line_offsets.len(), line_names.len());
        match axis {
            GridAxis::Column => {
                self.column_line_offsets = line_offsets;
                self.column_track_sizes = track_sizes;
                self.column_line_names = line_names;
            }
            GridAxis::Row => {
                self.row_line_offsets = line_offsets;
                self.row_track_sizes = track_sizes;
                self.row_line_names = line_names;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridLayoutPurpose {
    FinalLayout,
    IntrinsicProbe,
}

/// One cloned grid item fragment's destination interval and continuous
/// source-content interval.
///
/// Grid track geometry remains source geometry. This mapping is the explicit
/// boundary between that geometry and the destination fragment sequence that
/// receives repeated `box-decoration-break: clone` edges.
/// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
#[derive(Debug, Clone, Copy, PartialEq)]
struct GridClonedItemFragmentSlice {
    destination: GridFragmentItemContentSlice,
    source: GridFragmentItemContentSlice,
}

/// Taffy leaves used by one Grid sizing pass.
///
/// A contribution proxy models normal-flow content inside an inherited
/// subgrid axis. It participates in track sizing but never maps to a returned
/// grid item or a paint/replay record.
#[derive(Debug, Clone)]
pub(super) enum GridTaffyLeaf {
    Item(GridItemEstimate),
    Contribution(GridItemEstimate),
}

#[derive(Debug, Clone)]
pub(super) struct GridItemLayout {
    rect: GridRect,
    /// The item source height used by Grid's final placement. A cloned item
    /// keeps this continuous source coordinate system even when its repeated
    /// block decorations enlarge the destination fragment sequence.
    fragmentation_source_height: f32,
    /// Destination-to-source mappings for an item with cloned block edges.
    /// Grid row fragmentation selects destination intervals; replay selects
    /// the corresponding continuous source-content range from this record.
    cloned_fragment_slices: Vec<GridClonedItemFragmentSlice>,
    cloned_fragment_reservation: Option<FragmentDecorationReservation>,
    pub(super) area: Option<GridItemArea>,
    /// Whether a Grid Lanes item was placed from a definite grid-axis line or
    /// by the lanes cursor.  A final numeric area alone cannot preserve this:
    /// automatic subgrids remain track-aligned, but do not inherit parent line
    /// names. <https://drafts.csswg.org/css-grid-3/#subgrids>
    grid_lanes_placement: Option<GridLanesItemPlacement>,
    used_box_metrics: Option<UsedBoxMetrics>,
    final_percentage_axes: GridItemFinalPercentageAxes,
    /// A stretched vertical grid item retains a cyclic physical-height
    /// percentage during replay when the container's corresponding grid axis
    /// was indefinite. Its final grid area is still used for placement and
    /// painting; only its own content sizing must not be re-resolved against
    /// that area as a newly definite authored height.
    /// <https://drafts.csswg.org/css-grid-2/#grid-item-sizing>
    replay_cyclic_physical_height: bool,
}

/// Physical axes whose used size came from the post-track percentage phase.
/// Replay must retain those bounds instead of resolving the authored value
/// again against its temporary item formatting context.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GridItemFinalPercentageAxes {
    pub(super) width: bool,
    pub(super) height: bool,
}

/// The resolved Grid border-box extents replayed into an item's nested
/// formatting context.
///
/// Grid determines a physical rectangle before replay, but an orthogonal
/// item's logical inline axis is its physical height. Keeping that projection
/// beside the frozen border-box dimensions prevents Grid replay from
/// reintroducing the parent row as an unrelated inline-size basis.
/// <https://www.w3.org/TR/css-grid-1/#grid-items>
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
#[derive(Debug, Clone, Copy)]
pub(super) struct GridItemReplayDimensions {
    width: BorderBoxLength,
    height: BorderBoxLength,
}

impl GridItemReplayDimensions {
    pub(super) fn new(width: BorderBoxLength, height: BorderBoxLength) -> Self {
        Self { width, height }
    }

    pub(super) fn border_box_width(self) -> BorderBoxLength {
        self.width
    }

    pub(super) fn border_box_height(self) -> BorderBoxLength {
        self.height
    }

    /// Convert the frozen physical border-box width to the physical content
    /// width consumed by ordinary formatting-context replay.
    pub(super) fn physical_content_width_for_replay(
        self,
        style: &ComputedStyle,
    ) -> PhysicalContentWidth {
        let borders = used_border_widths(style);
        PhysicalContentWidth::new(border_box_to_content_box_length(
            self.width,
            non_content_pt(style.padding.left + style.padding.right + borders.left + borders.right),
        ))
    }

    /// Convert the frozen physical border-box height to the physical content
    /// height consumed by ordinary formatting-context replay.
    pub(super) fn physical_content_height_for_replay(
        self,
        style: &ComputedStyle,
    ) -> PhysicalContentHeight {
        let borders = used_border_widths(style);
        PhysicalContentHeight::new(border_box_to_content_box_length(
            self.height,
            non_content_pt(style.padding.top + style.padding.bottom + borders.top + borders.bottom),
        ))
    }

    /// Convert the frozen physical Grid border box to the item's logical
    /// inline content size at the replay boundary.
    ///
    /// Grid placement returns a border box, while inline layout consumes a
    /// content-box measure. Remove the used padding and borders once, then
    /// project the physical dimension through the item's writing mode.
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    /// <https://www.w3.org/TR/css-sizing-3/#box-sizing>
    pub(super) fn logical_inline_content_size_for_replay(
        self,
        style: &ComputedStyle,
    ) -> LogicalInlineContentSize {
        let borders = used_border_widths(style);
        let (border_box, extras) = if style.writing_mode.has_vertical_lines() {
            (
                self.height,
                non_content_pt(
                    style.padding.top + style.padding.bottom + borders.top + borders.bottom,
                ),
            )
        } else {
            (
                self.width,
                non_content_pt(
                    style.padding.left + style.padding.right + borders.left + borders.right,
                ),
            )
        };
        LogicalInlineContentSize::new(border_box_to_content_box_length(border_box, extras))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GridItemArea {
    pub(super) row_start: u16,
    pub(super) row_end: u16,
    pub(super) column_start: u16,
    pub(super) column_end: u16,
}

/// Replace Taffy's emulated subgrid area with the selected parent track area.
///
/// Taffy supplies placement order only: it cannot represent a borrowed Grid
/// axis, so final geometry must remain tied to the parent’s used tracks.
/// <https://www.w3.org/TR/css-grid-2/#subgrids>
pub(super) fn apply_resolved_subgrid_axis_item_geometry(
    axis: Option<&ResolvedSubgridAxis>,
    physical_axis: GridAxis,
    items: &mut [GridItemLayout],
) {
    let Some(axis) = axis else {
        return;
    };
    for item in items {
        let Some(area) = item.area else {
            continue;
        };
        let (start_line, end_line) = match physical_axis {
            GridAxis::Column => (area.column_start, area.column_end),
            GridAxis::Row => (area.row_start, area.row_end),
        };
        if let Some((start, end)) = axis.track_area_span(start_line, end_line) {
            item.set_axis_geometry(physical_axis, start, (end - start).max(0.0));
        }
    }
}

impl GridItemLayout {
    pub(super) fn new(rect: GridRect, area: Option<GridItemArea>) -> Self {
        let fragmentation_source_height = rect.size.height;
        Self {
            rect,
            fragmentation_source_height,
            cloned_fragment_slices: Vec::new(),
            cloned_fragment_reservation: None,
            area,
            grid_lanes_placement: None,
            used_box_metrics: None,
            final_percentage_axes: GridItemFinalPercentageAxes::default(),
            replay_cyclic_physical_height: false,
        }
    }

    pub(super) fn with_used_box_metrics(mut self, used_box_metrics: UsedBoxMetrics) -> Self {
        self.used_box_metrics = Some(used_box_metrics);
        self
    }

    pub(super) fn used_box_metrics(&self) -> Option<UsedBoxMetrics> {
        self.used_box_metrics
    }

    pub(super) fn final_percentage_axes(&self) -> GridItemFinalPercentageAxes {
        self.final_percentage_axes
    }

    pub(super) fn preserves_cyclic_physical_height_on_replay(&self) -> bool {
        self.replay_cyclic_physical_height
    }

    pub(super) fn preserve_cyclic_physical_height_on_replay(&mut self) {
        self.replay_cyclic_physical_height = true;
    }

    pub(super) fn mark_final_percentage_axis(&mut self, axis: GridAxis) {
        match axis {
            GridAxis::Column => self.final_percentage_axes.width = true,
            GridAxis::Row => self.final_percentage_axes.height = true,
        }
    }

    pub(super) fn grid_lanes_placement(&self) -> Option<GridLanesItemPlacement> {
        self.grid_lanes_placement
    }

    pub(super) fn set_grid_lanes_placement(&mut self, placement: GridLanesItemPlacement) {
        self.grid_lanes_placement = Some(placement);
    }

    pub(super) fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub(super) fn y(&self) -> f32 {
        self.rect.origin.y
    }

    /// Return Taffy's physical border-box geometry for this placed item.
    ///
    /// This is deliberately not the container's `PhysicalContentWidth`:
    /// converting it to a child content-box width needs the child's logical
    /// percentage basis, particularly in vertical writing modes.
    pub(super) fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub(super) fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(super) fn replay_dimensions(&self) -> GridItemReplayDimensions {
        GridItemReplayDimensions::new(
            border_box_pt(self.width().max(0.0)),
            border_box_pt(self.height().max(0.0)),
        )
    }

    /// The destination extent consumed by this item in the grid fragment
    /// plan. It differs from [`Self::height`] only for `clone`, whose border
    /// and padding occur in every occupied fragmentainer.
    pub(super) fn fragmentation_height(&self) -> f32 {
        self.cloned_fragment_slices
            .last()
            .map(|slice| slice.destination.block_end.points())
            .unwrap_or_else(|| self.height())
    }

    pub(super) fn fragmentation_source_height(&self) -> f32 {
        self.fragmentation_source_height
    }

    pub(super) fn has_cloned_fragment_projection(&self) -> bool {
        self.cloned_fragment_reservation.is_some()
    }

    /// Record the source content independently from repeated destination
    /// decoration. CSS Fragmentation applies cloned border and padding per
    /// box fragment, while the item's descendants remain one source flow.
    /// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
    pub(super) fn configure_cloned_fragment_source(
        &mut self,
        source_height: f32,
        reservation: FragmentDecorationReservation,
    ) {
        self.fragmentation_source_height = source_height.max(0.0);
        self.cloned_fragment_reservation = Some(reservation);
        self.cloned_fragment_slices.clear();
    }

    /// Build the item-local projection from source content to destination
    /// fragment geometry. A fresh cloned fragment owns both block edges.
    pub(super) fn project_cloned_fragment_destinations(
        &mut self,
        initial_raw_extent: LayoutLength,
        continuation_raw_extent: LayoutLength,
    ) -> bool {
        let Some(reservation) = self.cloned_fragment_reservation else {
            return false;
        };
        let initial_capacity = reservation.fresh_content_extent(initial_raw_extent);
        let continuation_capacity = reservation.fresh_content_extent(continuation_raw_extent);
        if initial_capacity.points() <= GRID_FRAGMENT_EPSILON
            || continuation_capacity.points() <= GRID_FRAGMENT_EPSILON
        {
            return false;
        }
        let mut remaining_source = self.fragmentation_source_height;
        let mut source_offset = 0.0;
        let mut destination_offset = 0.0;
        let mut capacity = initial_capacity;
        let mut slices = Vec::new();
        while remaining_source > GRID_FRAGMENT_EPSILON {
            let source_length = remaining_source.min(capacity.points());
            let destination_length = reservation.block_start().points()
                + source_length
                + reservation.block_end().points();
            slices.push(GridClonedItemFragmentSlice {
                destination: GridFragmentItemContentSlice {
                    block_start: GridFragmentBlockOffset::new(destination_offset),
                    block_end: GridFragmentBlockOffset::new(
                        destination_offset + destination_length,
                    ),
                },
                source: GridFragmentItemContentSlice {
                    block_start: GridFragmentBlockOffset::new(source_offset),
                    block_end: GridFragmentBlockOffset::new(source_offset + source_length),
                },
            });
            remaining_source -= source_length;
            source_offset += source_length;
            destination_offset += destination_length;
            capacity = continuation_capacity;
        }
        if slices.is_empty() {
            return false;
        }
        let changed =
            (self.fragmentation_height() - destination_offset).abs() > GRID_FRAGMENT_EPSILON;
        self.cloned_fragment_slices = slices;
        changed
    }

    /// Map a committed destination slice back into the continuous source
    /// content coordinate system used by isolated grid-item replay.
    pub(super) fn source_slice_for_destination_slice(
        &self,
        destination_slice: GridFragmentItemContentSlice,
    ) -> Option<GridFragmentItemContentSlice> {
        let reservation = self.cloned_fragment_reservation?;
        let mut source_start = None;
        let mut source_end = None;
        for fragment in &self.cloned_fragment_slices {
            let start = destination_slice
                .block_start
                .points()
                .max(fragment.destination.block_start.points());
            let end = destination_slice
                .block_end
                .points()
                .min(fragment.destination.block_end.points());
            if end <= start + GRID_FRAGMENT_EPSILON {
                continue;
            }
            let content_start =
                fragment.destination.block_start.points() + reservation.block_start().points();
            let source_extent =
                fragment.source.block_end.points() - fragment.source.block_start.points();
            let local_start = (start - content_start).clamp(0.0, source_extent);
            let local_end = (end - content_start).clamp(0.0, source_extent);
            source_start.get_or_insert(fragment.source.block_start.points() + local_start);
            source_end = Some(fragment.source.block_start.points() + local_end);
        }
        let fallback = destination_slice
            .block_start
            .points()
            .min(self.fragmentation_source_height);
        Some(GridFragmentItemContentSlice {
            block_start: GridFragmentBlockOffset::new(source_start.unwrap_or(fallback)),
            block_end: GridFragmentBlockOffset::new(source_end.unwrap_or(fallback)),
        })
    }

    pub(super) fn axis_start(&self, axis: GridAxis) -> f32 {
        match axis {
            GridAxis::Column => self.x(),
            GridAxis::Row => self.y(),
        }
    }

    pub(super) fn axis_size(&self, axis: GridAxis) -> f32 {
        match axis {
            GridAxis::Column => self.width(),
            GridAxis::Row => self.height(),
        }
    }

    pub(super) fn set_axis_geometry(&mut self, axis: GridAxis, start: f32, size: f32) {
        match axis {
            GridAxis::Column => {
                self.rect.origin.x = start;
                self.rect.size.width = size.max(0.0);
            }
            GridAxis::Row => {
                self.rect.origin.y = start;
                self.rect.size.height = size.max(0.0);
            }
        }
    }

    pub(super) fn page_top_rect(&self, container_origin: PageTopPoint) -> PageTopRect {
        grid_rect_to_page_top_rect(self.rect, container_origin)
    }

    pub(super) fn with_block_slice(&self, block_start: f32, block_end: f32) -> Self {
        let mut visible = self.clone();
        visible.set_axis_geometry(
            GridAxis::Row,
            block_start,
            (block_end - block_start).max(0.0),
        );
        visible
    }

    pub(super) fn gap_decoration_item(&self) -> GapDecorationItem {
        let rect = GapDecorationRect::new(
            GapDecorationPoint::new(self.rect.origin.x, self.rect.origin.y),
            GapDecorationSize::new(self.rect.size.width, self.rect.size.height),
        );
        if let Some(area) = self.area {
            GapDecorationItem::from_rect_with_grid_area(
                rect,
                GapDecorationGridArea {
                    row_start: area.row_start,
                    row_end: area.row_end,
                    column_start: area.column_start,
                    column_end: area.column_end,
                },
            )
        } else {
            GapDecorationItem::from_rect(rect)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_layout_retains_a_physical_content_height() {
        let layout = GridLayout {
            height: PhysicalContentHeight::new(content_box_pt(60.0)),
            baselines: PhysicalBaselineSets::default(),
            first_baseline: None,
            last_baseline: None,
            items: Vec::new(),
            baseline_resolutions: Vec::new(),
            gap_gutters: GapDecorationGridGutters::default(),
            column_line_offsets: Vec::new(),
            row_line_offsets: Vec::new(),
            column_line_names: Vec::new(),
            row_line_names: Vec::new(),
            column_track_sizes: Vec::new(),
            row_track_sizes: Vec::new(),
        };

        let _: PhysicalContentHeight = layout.height;
        assert_eq!(layout.height.points(), 60.0);
    }

    #[test]
    fn grid_replay_projects_logical_inline_size_from_final_physical_axis() {
        let replay = GridItemReplayDimensions::new(border_box_pt(16.0), border_box_pt(32.0));
        let mut style = ComputedStyle::initial();
        style.padding.left = 2.0;
        style.padding.right = 3.0;
        style.border_widths.left = 1.0;
        style.border_widths.right = 2.0;
        style.border_styles.left = BorderStyle::Solid;
        style.border_styles.right = BorderStyle::Solid;
        assert_eq!(
            replay.logical_inline_content_size_for_replay(&style),
            LogicalInlineContentSize::new(content_box_pt(8.0))
        );

        style.writing_mode = WritingMode::VerticalLr;
        style.padding.top = 4.0;
        style.padding.bottom = 6.0;
        style.border_widths.top = 3.0;
        style.border_widths.bottom = 2.0;
        style.border_styles.top = BorderStyle::Solid;
        style.border_styles.bottom = BorderStyle::Solid;
        assert_eq!(
            replay.logical_inline_content_size_for_replay(&style),
            LogicalInlineContentSize::new(content_box_pt(17.0))
        );
    }
}
