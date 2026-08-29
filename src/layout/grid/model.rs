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
    /// Final physical Grid geometry. Every consumer, including frozen
    /// feedback, subgrid, Grid Lanes, and gap decoration painting, derives
    /// its axis-specific view from these canonical records.
    pub(super) columns: GridAxisTopology,
    pub(super) rows: GridAxisTopology,
    /// The resolved physical content-box extents used to project the axis
    /// topology into aligned gap-decoration bands.
    pub(super) content_width: f32,
    pub(super) content_height: f32,
    /// Final Grid line names in physical Taffy order. This carries inherited
    /// and locally-added subgrid names through nested replay.
    pub(super) column_line_names: Vec<css::GridLineNames>,
    pub(super) row_line_names: Vec<css::GridLineNames>,
}

/// Final topology of one physical Grid axis.
///
/// Numeric zero is not sufficient to describe CSS Grid participation: an
/// occupied zero-sized track remains in the alignment and gap sequence, while
/// an empty `auto-fit` track is collapsed. Keep that provenance beside the
/// canonical interior gutters so no consumer can accidentally reconstruct a
/// paint topology from Taffy's lossy boundary-gutter representation.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GridTrackGeometry {
    start: f32,
    end: f32,
    collapsed_auto_fit: bool,
}

impl GridTrackGeometry {
    fn new(start: f32, end: f32, collapsed_auto_fit: bool) -> Self {
        debug_assert!(start <= end);
        Self {
            start,
            end,
            collapsed_auto_fit,
        }
    }

    pub(in crate::layout) fn start(self) -> f32 {
        self.start
    }

    pub(in crate::layout) fn end(self) -> f32 {
        self.end
    }

    pub(in crate::layout) fn size(self) -> f32 {
        (self.end - self.start).max(0.0)
    }

    pub(in crate::layout) fn is_active(self) -> bool {
        !self.collapsed_auto_fit
    }
}

#[derive(Debug, Clone, Copy)]
struct GridTrackLayoutInput {
    size: f32,
    gutter_after: f32,
    collapsed_auto_fit: bool,
}

#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct GridAxisTopology {
    /// Tracks are stored in increasing physical order. Their distinct start
    /// and end edges preserve the thickness of a Grid line (its gutter), so a
    /// grid area cannot accidentally absorb the gutter after its final track.
    tracks: Vec<GridTrackGeometry>,
}

impl GridAxisTopology {
    /// Build topology from geometry that is already in its intended form.
    ///
    /// This deliberately preserves raw backend and correction-source
    /// geometry. Target geometry for `repeat(auto-fit, ...)` must use
    /// [`Self::from_auto_fit_track_layout`] so collapsed gutters are
    /// canonicalized exactly once.
    pub(in crate::layout) fn from_track_layout(
        track_sizes: Vec<f32>,
        interior_gutters: Vec<f32>,
        collapsed_auto_fit_tracks: Vec<bool>,
    ) -> Option<Self> {
        if interior_gutters.len() != track_sizes.len().saturating_sub(1)
            || collapsed_auto_fit_tracks.len() != track_sizes.len()
            || track_sizes.iter().any(|value| !value.is_finite())
            || interior_gutters.iter().any(|value| !value.is_finite())
        {
            return None;
        }
        let inputs = track_sizes
            .into_iter()
            .zip(collapsed_auto_fit_tracks)
            .enumerate()
            .map(|(index, (size, collapsed_auto_fit))| GridTrackLayoutInput {
                size,
                gutter_after: interior_gutters.get(index).copied().unwrap_or(0.0),
                collapsed_auto_fit,
            });
        let mut offset = 0.0;
        let tracks = inputs
            .map(|input| {
                let start = offset;
                let end = start + input.size.max(0.0);
                offset = end + input.gutter_after.max(0.0);
                GridTrackGeometry::new(start, end, input.collapsed_auto_fit)
            })
            .collect();
        Some(Self { tracks })
    }

    /// Build canonical used geometry after `repeat(auto-fit, ...)` has
    /// collapsed its empty tracks.
    ///
    /// The gutters bordering an interior collapsed run overlap into one
    /// gutter between its bounding active tracks. A run touching a Grid edge
    /// has no outer gutter. Keeping this normalization at the topology
    /// boundary ensures sizing, alignment, and Grid Lanes consume the same
    /// CSS used geometry.
    /// <https://drafts.csswg.org/css-grid-1/#auto-repeat>
    pub(in crate::layout) fn from_auto_fit_track_layout(
        mut track_sizes: Vec<f32>,
        interior_gutters: Vec<f32>,
        collapsed_auto_fit_tracks: Vec<bool>,
    ) -> Option<Self> {
        if interior_gutters.len() != track_sizes.len().saturating_sub(1)
            || collapsed_auto_fit_tracks.len() != track_sizes.len()
        {
            return None;
        }

        for (size, &collapsed) in track_sizes.iter_mut().zip(&collapsed_auto_fit_tracks) {
            if collapsed {
                *size = 0.0;
            }
        }
        let interior_gutters =
            collapsed_auto_fit_gutters(&interior_gutters, &collapsed_auto_fit_tracks);
        Self::from_track_layout(track_sizes, interior_gutters, collapsed_auto_fit_tracks)
    }

    pub(in crate::layout) fn from_line_offsets(
        line_offsets: Vec<f32>,
        track_sizes: Vec<f32>,
        collapsed_auto_fit_tracks: Vec<bool>,
    ) -> Option<Self> {
        if line_offsets.len() != track_sizes.len().saturating_add(1)
            || collapsed_auto_fit_tracks.len() != track_sizes.len()
        {
            return None;
        }
        let starts = &line_offsets[..track_sizes.len()];
        let ends = starts
            .iter()
            .zip(&track_sizes)
            .map(|(start, size)| *start + size.max(0.0))
            .collect::<Vec<_>>();
        Self::from_track_bounds(starts, &ends, collapsed_auto_fit_tracks)
    }

    /// Build an axis from the final physical bounds of each track.
    ///
    /// This is a lossless physical-bounds constructor: it does not apply
    /// target `auto-fit` gutter normalization. Grid Lanes retains each
    /// track's start and end separately so that a
    /// lane item's grid area never absorbs the gutter after it.  Converting
    /// that representation back to the canonical Grid topology must use the
    /// space *between* adjacent bounds as the interior gutter; treating the
    /// track-end sequence as Grid line offsets loses gutters before
    /// zero-sized tracks.
    /// <https://www.w3.org/TR/css-grid-1/#gutters>
    pub(in crate::layout) fn from_track_bounds(
        track_starts: &[f32],
        track_ends: &[f32],
        collapsed_auto_fit_tracks: Vec<bool>,
    ) -> Option<Self> {
        if track_starts.len() != track_ends.len()
            || collapsed_auto_fit_tracks.len() != track_starts.len()
        {
            return None;
        }
        Self::from_track_geometry(
            track_starts
                .iter()
                .zip(track_ends)
                .zip(collapsed_auto_fit_tracks)
                .map(|((start, end), collapsed)| (*start, *end, collapsed)),
        )
    }

    pub(in crate::layout) fn from_track_geometry(
        tracks: impl IntoIterator<Item = (f32, f32, bool)>,
    ) -> Option<Self> {
        let mut previous_end = None;
        let tracks = tracks
            .into_iter()
            .map(|(start, end, collapsed)| {
                if !start.is_finite()
                    || !end.is_finite()
                    || start > end
                    || previous_end.is_some_and(|previous| previous > start)
                {
                    return None;
                }
                previous_end = Some(end);
                Some(GridTrackGeometry::new(start, end, collapsed))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { tracks })
    }

    pub(in crate::layout) fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub(in crate::layout) fn track(&self, index: usize) -> Option<GridTrackGeometry> {
        self.tracks.get(index).copied()
    }

    pub(in crate::layout) fn extent(&self) -> f32 {
        match (self.tracks.first(), self.tracks.last()) {
            (Some(first), Some(last)) => (last.end - first.start).max(0.0),
            _ => 0.0,
        }
    }

    pub(in crate::layout) fn track_sizes_iter(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.tracks.iter().map(|track| track.size())
    }

    pub(in crate::layout) fn collapsed_tracks_iter(
        &self,
    ) -> impl ExactSizeIterator<Item = bool> + '_ {
        self.tracks.iter().map(|track| !track.is_active())
    }

    pub(in crate::layout) fn track_sizes(&self) -> Vec<f32> {
        self.track_sizes_iter().collect()
    }

    pub(in crate::layout) fn interior_gutters(&self) -> Vec<f32> {
        self.tracks
            .windows(2)
            .map(|tracks| (tracks[1].start - tracks[0].end).max(0.0))
            .collect()
    }

    pub(in crate::layout) fn collapsed_auto_fit_tracks(&self) -> Vec<bool> {
        self.tracks
            .iter()
            .map(|track| track.collapsed_auto_fit)
            .collect()
    }

    pub(in crate::layout) fn line_offsets(&self) -> Vec<f32> {
        let mut offsets = self
            .tracks
            .iter()
            .map(|track| track.start)
            .collect::<Vec<_>>();
        if let Some(last) = self.tracks.last() {
            offsets.push(last.end);
        }
        offsets
    }

    /// Return the unaligned physical bounds of a one-based Grid line range.
    /// The final edge is the preceding track's end, never the following
    /// track's start across a gutter.
    pub(in crate::layout) fn area_bounds(
        &self,
        start_line: u16,
        end_line: u16,
    ) -> Option<(f32, f32)> {
        let start_track = usize::from(start_line).checked_sub(1)?;
        let end_track = usize::from(end_line).checked_sub(2)?;
        let start = self.tracks.get(start_track)?.start;
        let end = self.tracks.get(end_track)?.end;
        (start_track <= end_track).then_some((start, end.max(start)))
    }

    pub(in crate::layout) fn aligned_area_bounds(
        &self,
        content_alignment: css::ContentAlignment,
        container_size: f32,
        start_line: u16,
        end_line: u16,
    ) -> Option<(f32, f32)> {
        let start_track = usize::from(start_line).checked_sub(1)?;
        let end_track = usize::from(end_line).checked_sub(2)?;
        if start_track > end_track {
            return None;
        }
        let line_offsets = self.line_offsets();
        let collapsed = self.collapsed_auto_fit_tracks();
        let start = content_aligned_grid_line_offset_with_collapsed_tracks(
            content_alignment,
            container_size,
            &line_offsets,
            start_track,
            Some(&collapsed),
        )?;
        let end_start = content_aligned_grid_line_offset_with_collapsed_tracks(
            content_alignment,
            container_size,
            &line_offsets,
            end_track,
            Some(&collapsed),
        )?;
        let end = end_start + self.tracks.get(end_track)?.size();
        Some((start, end.max(start)))
    }

    pub(in crate::layout) fn has_collapsed_auto_fit_tracks(&self) -> bool {
        self.tracks.iter().any(|track| track.collapsed_auto_fit)
    }

    /// Reverse backend-logical track order into increasing physical order.
    pub(in crate::layout) fn reversed(&self) -> Self {
        let Some(first) = self.tracks.first() else {
            return Self::default();
        };
        let outer_start = first.start;
        let outer_end = self
            .tracks
            .last()
            .map(|track| track.end)
            .unwrap_or(outer_start);
        let tracks = self
            .tracks
            .iter()
            .rev()
            .map(|track| {
                GridTrackGeometry::new(
                    outer_start + outer_end - track.end,
                    outer_start + outer_end - track.start,
                    track.collapsed_auto_fit,
                )
            })
            .collect();
        Self { tracks }
    }
}

impl GridLayout {
    /// Used physical track sizes reported by the shared Grid sizing pass.
    ///
    /// Grid Lanes uses these only while resolving its Level 3 intrinsic
    /// auto-repeat hypothesis, before it performs its distinct packing pass.
    pub(super) fn physical_track_sizes(&self, axis: GridAxis) -> Vec<f32> {
        self.axis_topology(axis).track_sizes()
    }

    pub(super) fn axis_topology(&self, axis: GridAxis) -> &GridAxisTopology {
        match axis {
            GridAxis::Column => &self.columns,
            GridAxis::Row => &self.rows,
        }
    }

    pub(super) fn gap_decoration_gutters(&self, style: &ComputedStyle) -> GapDecorationGridGutters {
        grid_gap_decoration_gutters_from_topologies(
            &self.columns,
            &self.rows,
            style,
            self.content_width,
            self.content_height,
        )
    }

    /// Replace one physical axis with the final Grid Lanes topology before
    /// measuring packed children. This keeps subgrid probes and final replay
    /// on the same used tracks and line-name map.
    pub(super) fn set_physical_grid_axis_topology(
        &mut self,
        axis: GridAxis,
        topology: GridAxisTopology,
        line_names: Vec<css::GridLineNames>,
    ) {
        debug_assert_eq!(topology.line_offsets().len(), line_names.len());
        match axis {
            GridAxis::Column => {
                self.columns = topology;
                self.column_line_names = line_names;
            }
            GridAxis::Row => {
                self.rows = topology;
                self.row_line_names = line_names;
            }
        }
    }
}

/// Canonicalize gutters around collapsed `auto-fit` tracks.
///
/// Each interior collapsed run merges its two adjacent gutters by overlap.
/// The canonical physical ordering keeps that merged gutter immediately after
/// the active track preceding the run. Runs at either outer edge have no
/// gutter because one of the two sides is absent. An occupied zero-sized track
/// remains active and therefore keeps its normal gutters.
/// <https://drafts.csswg.org/css-grid-1/#auto-repeat>
fn collapsed_auto_fit_gutters(gutters: &[f32], collapsed_tracks: &[bool]) -> Vec<f32> {
    debug_assert_eq!(gutters.len(), collapsed_tracks.len().saturating_sub(1));
    let Some(last_active_track) = collapsed_tracks.iter().rposition(|&collapsed| !collapsed) else {
        return vec![0.0; gutters.len()];
    };

    gutters
        .iter()
        .enumerate()
        .map(|(index, &gutter)| {
            // An active track followed by any later active track contributes
            // exactly one gutter, whether the next active track is adjacent
            // or separated by a collapsed run.
            if !collapsed_tracks[index] && index < last_active_track {
                gutter.max(0.0)
            } else {
                0.0
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridLayoutPurpose {
    FinalLayout,
    /// Final track sizing used only to determine a floated Grid's automatic
    /// block size. It borrows any installed subgrid context so final replay
    /// remains its one-shot owner.
    FloatBlockSizeMeasurement,
    IntrinsicProbe,
}

impl GridLayoutPurpose {
    pub(super) const fn uses_final_track_sizing(self) -> bool {
        matches!(self, Self::FinalLayout | Self::FloatBlockSizeMeasurement)
    }
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

impl GridItemArea {
    fn reverse_line_range(start: u16, end: u16, track_count: usize) -> Option<(u16, u16)> {
        let boundary = u16::try_from(track_count).ok()?.checked_add(2)?;
        Some((boundary.checked_sub(end)?, boundary.checked_sub(start)?))
    }

    pub(super) fn with_reversed_axis(self, axis: GridAxis, track_count: usize) -> Option<Self> {
        let mut area = self;
        match axis {
            GridAxis::Column => {
                (area.column_start, area.column_end) =
                    Self::reverse_line_range(area.column_start, area.column_end, track_count)?;
            }
            GridAxis::Row => {
                (area.row_start, area.row_end) =
                    Self::reverse_line_range(area.row_start, area.row_end, track_count)?;
            }
        }
        Some(area)
    }
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
    fn grid_axis_area_bounds_exclude_the_following_gutter() {
        let topology = GridAxisTopology::from_track_layout(
            vec![100.0, 40.0, 60.0],
            vec![20.0, 10.0],
            vec![false; 3],
        )
        .unwrap();

        assert_eq!(topology.area_bounds(1, 2), Some((0.0, 100.0)));
        assert_eq!(topology.area_bounds(1, 3), Some((0.0, 160.0)));
        assert_eq!(topology.area_bounds(2, 4), Some((120.0, 230.0)));
    }

    #[test]
    fn grid_axis_topology_rejects_mismatched_and_overlapping_geometry() {
        assert!(
            GridAxisTopology::from_track_layout(vec![10.0, 20.0], Vec::new(), vec![false; 2],)
                .is_none()
        );
        assert!(
            GridAxisTopology::from_line_offsets(vec![0.0, 10.0], vec![10.0, 20.0], vec![false; 2],)
                .is_none()
        );
        assert!(
            GridAxisTopology::from_track_bounds(&[0.0, 5.0], &[10.0, 20.0], vec![false; 2],)
                .is_none()
        );
        assert!(
            GridAxisTopology::from_track_layout(vec![f32::NAN], Vec::new(), vec![false]).is_none()
        );
    }

    #[test]
    fn empty_and_zero_sized_topologies_remain_valid_when_reversed() {
        let empty = GridAxisTopology::from_track_layout(Vec::new(), Vec::new(), Vec::new())
            .expect("an empty axis is valid topology");
        assert_eq!(empty.track_count(), 0);
        assert_eq!(empty.reversed().track_count(), 0);

        let zero = GridAxisTopology::from_track_layout(vec![0.0], Vec::new(), vec![false])
            .expect("an occupied zero-sized track is valid topology")
            .reversed();
        assert_eq!(zero.area_bounds(1, 2), Some((0.0, 0.0)));
        assert_eq!(zero.collapsed_auto_fit_tracks(), vec![false]);
    }

    #[test]
    fn auto_fit_topology_merges_gutters_around_an_interior_collapsed_run() {
        let topology = GridAxisTopology::from_auto_fit_track_layout(
            vec![10.0, 10.0, 10.0, 10.0, 10.0],
            vec![3.0, 4.0, 5.0, 6.0],
            vec![false, true, true, false, false],
        )
        .unwrap();

        assert_eq!(topology.track_sizes(), vec![10.0, 0.0, 0.0, 10.0, 10.0]);
        assert_eq!(topology.interior_gutters(), vec![3.0, 0.0, 0.0, 6.0]);
        assert_eq!(
            topology.line_offsets(),
            vec![0.0, 13.0, 13.0, 13.0, 29.0, 39.0]
        );
    }

    #[test]
    fn auto_fit_topology_removes_outer_collapsed_run_gutters() {
        let leading = GridAxisTopology::from_auto_fit_track_layout(
            vec![10.0, 10.0, 10.0, 10.0],
            vec![3.0, 4.0, 5.0],
            vec![true, true, false, false],
        )
        .unwrap();
        let trailing = GridAxisTopology::from_auto_fit_track_layout(
            vec![10.0, 10.0, 10.0, 10.0],
            vec![3.0, 4.0, 5.0],
            vec![false, false, true, true],
        )
        .unwrap();
        let all = GridAxisTopology::from_auto_fit_track_layout(
            vec![10.0, 10.0, 10.0],
            vec![3.0, 4.0],
            vec![true; 3],
        )
        .unwrap();

        assert_eq!(leading.interior_gutters(), vec![0.0, 0.0, 5.0]);
        assert_eq!(trailing.interior_gutters(), vec![3.0, 0.0, 0.0]);
        assert_eq!(all.interior_gutters(), vec![0.0, 0.0]);
        assert_eq!(all.line_offsets(), vec![0.0; 4]);
    }

    #[test]
    fn auto_fit_topology_does_not_collapse_an_occupied_zero_sized_track() {
        let topology = GridAxisTopology::from_auto_fit_track_layout(
            vec![10.0, 0.0, 10.0],
            vec![3.0, 4.0],
            vec![false, false, false],
        )
        .unwrap();

        assert_eq!(topology.track_sizes(), vec![10.0, 0.0, 10.0]);
        assert_eq!(topology.interior_gutters(), vec![3.0, 4.0]);
    }

    #[test]
    fn grid_axis_reversal_keeps_bounds_and_area_projection_in_sync() {
        let topology = GridAxisTopology::from_track_layout(
            vec![30.0, 10.0, 20.0],
            vec![5.0, 7.0],
            vec![false, true, false],
        )
        .unwrap()
        .reversed();
        assert_eq!(topology.track_sizes(), vec![20.0, 10.0, 30.0]);
        assert_eq!(topology.interior_gutters(), vec![7.0, 5.0]);
        assert_eq!(
            topology.collapsed_auto_fit_tracks(),
            vec![false, true, false]
        );

        let area = GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: 1,
            column_end: 2,
        }
        .with_reversed_axis(GridAxis::Column, 3)
        .expect("valid area projects into physical lines");
        assert_eq!((area.column_start, area.column_end), (3, 4));
        assert_eq!(
            area.with_reversed_axis(GridAxis::Column, 3)
                .map(|area| (area.column_start, area.column_end)),
            Some((1, 2))
        );
    }

    #[test]
    fn grid_layout_retains_a_physical_content_height() {
        let layout = GridLayout {
            height: PhysicalContentHeight::new(content_box_pt(60.0)),
            baselines: PhysicalBaselineSets::default(),
            first_baseline: None,
            last_baseline: None,
            items: Vec::new(),
            baseline_resolutions: Vec::new(),
            columns: GridAxisTopology::default(),
            rows: GridAxisTopology::default(),
            content_width: 0.0,
            content_height: 60.0,
            column_line_names: Vec::new(),
            row_line_names: Vec::new(),
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
