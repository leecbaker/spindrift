use super::*;

/// Which physical axis contains the fixed Grid Lanes tracks.
///
/// Grid Lanes Layout keeps Grid's track model in one axis and packs items in
/// the perpendicular stacking axis. The orientation follows the Level 3
/// `normal` rule: an authored row template with no column template creates
/// row lanes; all other cases create column lanes.
/// <https://drafts.csswg.org/css-grid-3/#orienting-grid-lanes-layout>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridLanesAxis {
    Columns,
    Rows,
}

/// Placement provenance retained after Grid Lanes has converted an item's
/// authored grid-axis placement into a final numeric area. Automatic subgrids
/// keep the parent tracks but deliberately do not inherit parent line names.
/// <https://drafts.csswg.org/css-grid-3/#subgrids>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridLanesItemPlacement {
    Definite { grid_axis: GridAxis },
    Automatic { grid_axis: GridAxis, span: usize },
}

impl GridLanesItemPlacement {
    pub(super) fn grid_axis(self) -> GridAxis {
        match self {
            Self::Definite { grid_axis } | Self::Automatic { grid_axis, .. } => grid_axis,
        }
    }

    pub(super) fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic { .. })
    }
}

/// Whether Grid Lanes packs its items along the physical block axis.
///
/// This is used by the atomic-inline overflow adapter; baseline export stays
/// in the ordinary physical-baseline pipeline.
/// <https://drafts.csswg.org/css-grid-3/#grid-lanes-baseline-alignment>
pub(super) fn grid_lanes_stacking_axis_is_block(style: &ComputedStyle) -> bool {
    matches!(GridLanesAxis::from_style(style), GridLanesAxis::Columns)
}

pub(super) fn grid_lanes_item_placement(
    parent_style: &ComputedStyle,
    child: &GridChild<'_>,
) -> Option<GridLanesItemPlacement> {
    parent_style.display.is_grid_lanes().then(|| {
        let axis = GridLanesAxis::from_style(parent_style);
        let (start, end) = axis.placements(child);
        let grid_axis = match axis {
            GridLanesAxis::Columns => GridAxis::Column,
            GridLanesAxis::Rows => GridAxis::Row,
        };
        if matches!(start, css::GridPlacement::Line(_))
            || matches!(end, css::GridPlacement::Line(_))
        {
            GridLanesItemPlacement::Definite { grid_axis }
        } else {
            GridLanesItemPlacement::Automatic {
                grid_axis,
                span: grid_lanes_span(axis, child, usize::MAX),
            }
        }
    })
}

type GridLanesPercentageBasis = PercentageBasis<LayoutLength>;

/// Container geometry shared by the Grid Lanes placement and its intrinsic
/// auto-repeat probe. Keeping these inputs together prevents the grid-axis
/// percentage basis from being separated from the physical width and resolved
/// subgrid axis that establish it.
pub(super) struct GridLanesLayoutContext<'a> {
    pub(super) width: PhysicalContentWidth,
    pub(super) block_percentage_basis: GridLanesPercentageBasis,
    pub(super) subgrid_context: Option<&'a ResolvedSubgridContext>,
}

/// Inputs that bind a packed item's intrinsic measurement to the final parent
/// Grid Lanes topology. Keeping them together prevents probe-only subgrid
/// geometry from diverging from replay.
struct GridLanesItemMeasurementContext<'a> {
    stylesheets: &'a Stylesheets<'a>,
    parent_style: &'a ComputedStyle,
    parent_layout: &'a GridLayout,
    area: GridItemArea,
    placement: GridLanesItemPlacement,
    inline_percentage_basis: GridLanesPercentageBasis,
    axis: GridLanesAxis,
}

fn grid_lanes_basis_points(basis: GridLanesPercentageBasis) -> f32 {
    basis.points().unwrap_or(0.0)
}

/// Intrinsic maximum used while deriving an auto-repeat's hypothetical track
/// size. The full Grid sizing algorithm continues to own mixed repeat lists;
/// this is the single-track subset needed before an intrinsic auto-repeat has
/// a concrete repetition count.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum GridLanesIntrinsicAutoRepeatTrack {
    Auto,
    MinContent,
    MaxContent,
    FitContent(css::ComputedLengthPercentage),
}

/// Provenance of a final Grid Lanes track after an intrinsic auto-repeat has
/// been materialized. Keeping this separate from its used size ensures that
/// `auto-fit` can collapse only repeated tracks while explicit line placement
/// and implicit-grid formation retain their normal Grid semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridLanesAutoRepeatTrackSource {
    FixedPrefix,
    Repeated { repetition: usize, slot: usize },
    FixedSuffix,
    ImplicitEnd,
}

#[derive(Debug, Clone)]
struct GridLanesResolvedAutoRepeatTrack {
    source: GridLanesAutoRepeatTrackSource,
    used_size: f32,
    auto_sized: bool,
    collapsed: bool,
}

/// Final grid-axis topology for a Grid Lanes intrinsic auto-repeat.
///
/// The hypothetical pass determines the repeated slots; this record then
/// materializes the actual explicit and implicit grid before Grid Lanes
/// placement. It is deliberately the sole source of both line resolution and
/// packing availability.
/// <https://drafts.csswg.org/css-grid-3/#track-sizing>
#[derive(Clone)]
struct GridLanesAutoRepeatResolution {
    tracks: Vec<GridLanesResolvedAutoRepeatTrack>,
    explicit_line_count: usize,
    line_names: Vec<Vec<String>>,
    repeat_range: std::ops::Range<usize>,
    gap: f32,
    materialized_geometry: Option<GridLanesTrackGeometry>,
}

impl GridLanesAutoRepeatResolution {
    fn geometry(&self, alignment: css::ContentAlignment, available: f32) -> GridLanesTrackGeometry {
        if let Some(geometry) = &self.materialized_geometry {
            return geometry.clone();
        }
        debug_assert!(self.repeat_range.clone().all(|index| matches!(
            self.tracks[index].source,
            GridLanesAutoRepeatTrackSource::Repeated { .. }
        )));
        let sizes = self
            .tracks
            .iter()
            .map(|track| track.used_size)
            .collect::<Vec<_>>();
        let active = self
            .tracks
            .iter()
            .map(|track| !track.collapsed)
            .collect::<Vec<_>>();
        let auto_sized = self
            .tracks
            .iter()
            .map(|track| track.auto_sized && !track.collapsed)
            .collect::<Vec<_>>();
        GridLanesTrackGeometry::from_track_sizes_with_active(&sizes, self.gap, &active)
            .expect("an intrinsic auto-repeat resolution always has tracks")
            .with_auto_track_stretch(&auto_sized, alignment, available)
    }

    fn resolved_range(
        &self,
        axis: GridLanesAxis,
        child: &GridChild<'_>,
    ) -> Option<std::ops::Range<usize>> {
        let span = grid_lanes_span(axis, child, self.tracks.len());
        let (start, end) = axis.placements(child);
        let start_line = self.line_index(start)?;
        let end_line = self.line_index(end)?;
        let start = match (start_line, end_line) {
            (Some(start), Some(end)) if end > start => start,
            (Some(start), _) => start,
            (_, Some(end)) => end.checked_sub(span)?,
            _ => return None,
        };
        (start.checked_add(span)? <= self.tracks.len()).then_some(start..start + span)
    }

    fn line_index(&self, placement: &css::GridPlacement) -> Option<Option<usize>> {
        let css::GridPlacement::Line(line) = placement else {
            return Some(None);
        };
        let index = if let Some(name) = line.name() {
            named_grid_line_index(&self.line_names, name, line.index().unwrap_or(1))?
        } else {
            let index = line.index()?;
            if index > 0 {
                index
            } else {
                i32::try_from(self.explicit_line_count)
                    .ok()?
                    .checked_add(index)?
                    .checked_add(1)?
            }
        };
        usize::try_from(index.checked_sub(1)?).ok().map(Some)
    }

    fn append_end_implicit_tracks(&mut self, count: usize, size: f32, auto_sized: bool) {
        self.tracks
            .extend((0..count).map(|_| GridLanesResolvedAutoRepeatTrack {
                source: GridLanesAutoRepeatTrackSource::ImplicitEnd,
                used_size: size,
                auto_sized,
                collapsed: false,
            }));
        self.line_names.resize_with(self.tracks.len() + 1, Vec::new);
    }

    fn apply_auto_fit(
        &mut self,
        axis: GridLanesAxis,
        children: &[GridChild<'_>],
        items: &[GridItemLayout],
        flow_tolerance: f32,
    ) {
        let mut occupied = vec![false; self.tracks.len()];
        let active = vec![true; self.tracks.len()];
        let mut lane_ends = vec![0.0; self.tracks.len()];
        let mut cursor = 0;
        for (child, item) in children.iter().zip(items) {
            let span = grid_lanes_span(axis, child, self.tracks.len());
            let (range, automatic) = self
                .resolved_range(axis, child)
                .map(|range| (range, false))
                .unwrap_or_else(|| {
                    (
                        grid_lanes_shortest_range(
                            &lane_ends,
                            &active,
                            span,
                            cursor,
                            flow_tolerance,
                        ),
                        true,
                    )
                });
            if automatic {
                cursor = range.end;
            }
            // A one-track automatic item is replayed after `auto-fit` has
            // removed empty repeated tracks. A spanning automatic item has a
            // concrete candidate area during the occupancy pass and keeps
            // every repeated track it spans occupied.
            if !automatic || span > 1 {
                for index in range.clone() {
                    occupied[index] = true;
                }
            }
            let stacking_size = match axis {
                GridLanesAxis::Columns => item.height(),
                GridLanesAxis::Rows => item.width(),
            }
            .max(0.0);
            let end = lane_ends[range.clone()].iter().copied().fold(0.0, f32::max) + stacking_size;
            for lane_end in &mut lane_ends[range] {
                *lane_end = end;
            }
        }
        for index in self.repeat_range.clone() {
            if !occupied[index] {
                self.tracks[index].collapsed = true;
            }
        }
    }
}

/// Physical start/end geometry for the fixed grid axis of a Grid Lanes box.
///
/// Grid's line-offset record stores the end of each track, while gutters own
/// the interval before the following track. Keeping both edges explicit makes
/// it impossible for lane item sizing to accidentally include a gutter before
/// or after its grid area.
#[derive(Clone)]
struct GridLanesTrackGeometry {
    starts: Vec<f32>,
    ends: Vec<f32>,
    /// A collapsed `repeat(auto-fit, ...)` track keeps its grid lines but
    /// cannot receive automatic Grid Lanes placement.
    active: Vec<bool>,
}

impl GridLanesTrackGeometry {
    fn from_resolved_subgrid_axis(axis: &ResolvedSubgridAxis) -> Option<Self> {
        debug_assert_eq!(axis.line_offsets().len(), axis.track_count() + 1);
        debug_assert_eq!(
            axis.gutter_sizes().len(),
            axis.track_count().saturating_sub(1)
        );
        (!axis.track_starts().is_empty()).then(|| Self {
            starts: axis.track_starts().to_vec(),
            ends: axis.track_ends().to_vec(),
            active: vec![true; axis.track_count()],
        })
    }

    fn from_grid_layout_offsets(
        line_offsets: &[f32],
        gutters: &[GapDecorationGutter],
    ) -> Option<Self> {
        let track_count = line_offsets.len().checked_sub(1)?;
        if track_count == 0 {
            return None;
        }
        let mut starts = Vec::with_capacity(track_count);
        let mut ends = Vec::with_capacity(track_count);
        for index in 0..track_count {
            let start = if index == 0 {
                line_offsets[0]
            } else {
                gutters
                    .get(index - 1)
                    .map(|gutter| gutter.span.end)
                    .unwrap_or(line_offsets[index])
            };
            starts.push(start);
            ends.push(line_offsets[index + 1].max(start));
        }
        Some(Self {
            active: vec![true; track_count],
            starts,
            ends,
        })
    }

    fn from_track_sizes(sizes: &[f32], gap: f32) -> Option<Self> {
        Self::from_track_sizes_with_active(sizes, gap, &vec![true; sizes.len()])
    }

    /// Construct final line geometry after `auto-fit` has collapsed its empty
    /// tracks. Collapsed tracks and the gutters on either side of them have
    /// zero used size, but their lines remain addressable by explicit
    /// placement.
    /// <https://drafts.csswg.org/css-grid-3/#auto-repeat>
    fn from_track_sizes_with_active(sizes: &[f32], gap: f32, active: &[bool]) -> Option<Self> {
        if sizes.len() != active.len() {
            return None;
        }
        if sizes.is_empty() {
            return None;
        }
        let mut starts = Vec::with_capacity(sizes.len());
        let mut ends = Vec::with_capacity(sizes.len());
        let mut offset = 0.0;
        for (index, size) in sizes.iter().enumerate() {
            starts.push(offset);
            if active[index] {
                offset += size.max(0.0);
            }
            ends.push(offset);
            // A run of collapsed auto-fit tracks has no breadth, but the
            // active tracks on either side remain separate grid tracks and
            // therefore retain their single intervening gutter. This avoids
            // turning an empty repeated slot into an extra visual gap.
            if active[index] && active[index + 1..].iter().any(|&track| track) {
                offset += gap;
            }
        }
        Some(Self {
            starts,
            ends,
            active: active.to_vec(),
        })
    }

    fn track_count(&self) -> usize {
        self.starts.len()
    }

    fn area_start(&self, range: &std::ops::Range<usize>) -> f32 {
        self.starts[range.start]
    }

    fn area_size(&self, range: &std::ops::Range<usize>) -> f32 {
        (self.ends[range.end - 1] - self.starts[range.start]).max(0.0)
    }

    fn line_offsets(&self) -> Vec<f32> {
        let mut offsets = Vec::with_capacity(self.track_count() + 1);
        offsets.push(self.starts[0]);
        offsets.extend(self.ends.iter().cloned());
        offsets
    }

    fn track_sizes(&self) -> Vec<f32> {
        self.starts
            .iter()
            .zip(&self.ends)
            .map(|(start, end)| (end - start).max(0.0))
            .collect()
    }

    /// Distribute a definite grid-axis container's remaining free space
    /// between its resolved tracks according to content alignment.
    ///
    /// This operates on the explicit start/end representation, so distributed
    /// gaps never become part of an item's grid area:
    /// <https://www.w3.org/TR/css-align-3/#content-distribution>.
    fn with_content_alignment(
        mut self,
        alignment: css::ContentAlignment,
        container_size: f32,
    ) -> Self {
        if self.track_count() == 0 {
            return self;
        }
        let occupied_size = self.ends.last().cloned().unwrap_or(0.0) - self.starts[0];
        let free_space = container_size - occupied_size;
        let free_space = if alignment.safety == AlignmentSafety::Unsafe {
            free_space
        } else {
            free_space.max(0.0)
        };
        let count = self.track_count();
        let (initial_offset, between_offset) = match alignment.keyword {
            css::ContentAlignmentKeyword::End
            | css::ContentAlignmentKeyword::FlexEnd
            | css::ContentAlignmentKeyword::Right => (free_space, 0.0),
            css::ContentAlignmentKeyword::Center => (free_space / 2.0, 0.0),
            css::ContentAlignmentKeyword::SpaceBetween if count > 1 => {
                (0.0, free_space / (count - 1) as f32)
            }
            css::ContentAlignmentKeyword::SpaceAround => {
                let between = free_space / count as f32;
                (between / 2.0, between)
            }
            css::ContentAlignmentKeyword::SpaceEvenly => {
                let between = free_space / (count + 1) as f32;
                (between, between)
            }
            _ => (0.0, 0.0),
        };
        for (index, (start, end)) in self.starts.iter_mut().zip(&mut self.ends).enumerate() {
            let offset = initial_offset + between_offset * index as f32;
            *start += offset;
            *end += offset;
        }
        self
    }

    /// Stretch auto-sized tracks before applying positional content alignment.
    ///
    /// In a grid container, `normal` computes to `stretch` for content
    /// distribution.  Stretching consumes positive free space by increasing
    /// each auto-sized track equally; it is not equivalent to inserting space
    /// between the tracks.  Keeping it on the explicit start/end geometry
    /// preserves the distinction between a widened track and a gutter.
    /// <https://www.w3.org/TR/css-align-3/#valdef-justify-content-stretch>
    fn with_auto_track_stretch(
        mut self,
        auto_sized_tracks: &[bool],
        alignment: css::ContentAlignment,
        container_size: f32,
    ) -> Self {
        if self.track_count() != auto_sized_tracks.len()
            || !matches!(
                alignment.keyword,
                css::ContentAlignmentKeyword::Normal | css::ContentAlignmentKeyword::Stretch
            )
        {
            return self;
        }
        let auto_track_count = auto_sized_tracks.iter().filter(|&&track| track).count();
        if auto_track_count == 0 {
            return self;
        }
        let occupied_size = self.ends.last().cloned().unwrap_or(0.0) - self.starts[0];
        let additional_size =
            ((container_size - occupied_size).max(0.0) / auto_track_count as f32).max(0.0);
        if additional_size == 0.0 {
            return self;
        }

        let mut preceding_growth = 0.0;
        for ((start, end), &is_auto_sized) in self
            .starts
            .iter_mut()
            .zip(&mut self.ends)
            .zip(auto_sized_tracks)
        {
            *start += preceding_growth;
            *end += preceding_growth;
            if is_auto_sized {
                *end += additional_size;
                preceding_growth += additional_size;
            }
        }
        self
    }
}

impl GridLanesAxis {
    fn from_style(style: &ComputedStyle) -> Self {
        match style.grid_lanes_direction {
            css::GridLanesDirection::Axis {
                axis: css::GridLanesAxis::Column,
                ..
            } => Self::Columns,
            css::GridLanesDirection::Axis {
                axis: css::GridLanesAxis::Row,
                ..
            } => Self::Rows,
            css::GridLanesDirection::Normal => {
                match (&style.grid_template_columns, &style.grid_template_rows) {
                    (css::GridTrackList::None, css::GridTrackList::Tracks { .. }) => Self::Rows,
                    _ => Self::Columns,
                }
            }
        }
    }

    fn placements<'a>(
        self,
        child: &'a GridChild<'_>,
    ) -> (&'a css::GridPlacement, &'a css::GridPlacement) {
        match self {
            Self::Columns => (&child.style.grid_column_start, &child.style.grid_column_end),
            Self::Rows => (&child.style.grid_row_start, &child.style.grid_row_end),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Convert Grid Level 1 track geometry into Grid Lanes item placement.
    ///
    /// The existing Taffy-backed Grid pass remains the owner of grid-axis
    /// track sizing, line names, auto-repeat expansion, and self-sizing. This
    /// pass replaces only the perpendicular two-dimensional auto-placement
    /// with Grid Lanes' shortest-available-track algorithm. Retaining the
    /// shared track pipeline keeps the two layout modes aligned as Grid track
    /// sizing expands:
    /// <https://drafts.csswg.org/css-grid-3/#grid-lanes-layout-and-placement-algorithm>.
    pub(super) fn apply_grid_lanes_placement(
        &mut self,
        style: &ComputedStyle,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        context: GridLanesLayoutContext<'_>,
        mut layout: GridLayout,
    ) -> GridLayout {
        let GridLanesLayoutContext {
            width,
            block_percentage_basis,
            subgrid_context,
        } = context;
        let inline_percentage_basis = PercentageBasis::definite(layout_pt(width.points()));
        let axis = GridLanesAxis::from_style(style);
        let swaps_physical_grid_axes =
            WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes();
        let resolved_grid_axis = subgrid_context.and_then(|context| {
            context.physical_axis(
                match axis {
                    GridLanesAxis::Columns => GridAxis::Column,
                    GridLanesAxis::Rows => GridAxis::Row,
                },
                swaps_physical_grid_axes,
            )
        });
        let grid_axis_tracks = match axis {
            GridLanesAxis::Columns => &style.grid_template_columns,
            GridLanesAxis::Rows => &style.grid_template_rows,
        };
        let taffy_line_offsets = match axis {
            GridLanesAxis::Columns => &layout.column_line_offsets,
            GridLanesAxis::Rows => &layout.row_line_offsets,
        };
        // Taffy's two-dimensional placement can introduce an implicit axis
        // before reporting its track details.  In Grid Lanes that axis is not
        // populated by ordinary row/column auto-placement, so preserve the
        // authored geometry for the common fully-definite track case instead
        // of inheriting those unrelated implicit-track offsets.
        // The post-layout height of an auto-sized grid is not a percentage
        // basis for its row axis. In particular, treating it as definite
        // makes a cyclic percentage child create an intrinsic row that CSS
        // must instead size as `auto`.
        // <https://www.w3.org/TR/css-grid/#track-sizing>
        let grid_axis_percentage_basis = match axis {
            GridLanesAxis::Columns => PercentageBasis::definite(layout_pt(width.points())),
            GridLanesAxis::Rows => block_percentage_basis,
        };
        let grid_axis_size = grid_lanes_basis_points(grid_axis_percentage_basis);
        let grid_axis_gap = resolved_grid_axis.map_or_else(
            || {
                used_grid_lanes_gap(
                    match axis {
                        GridLanesAxis::Columns => style.column_gap.clone(),
                        GridLanesAxis::Rows => style.row_gap.clone(),
                    },
                    grid_axis_percentage_basis,
                )
            },
            ResolvedSubgridAxis::taffy_gap,
        );
        let grid_axis_content_alignment = match axis {
            GridLanesAxis::Columns => style.justify_content,
            GridLanesAxis::Rows => style.align_content,
        };
        let fixed_track_sizes =
            grid_lanes_definite_track_sizes(grid_axis_tracks, grid_axis_percentage_basis);
        let auto_repeat_resolution = self.grid_lanes_resolve_intrinsic_auto_repeat(
            style,
            axis,
            grid_axis_tracks,
            children,
            &layout.items,
            stylesheets,
            width,
            grid_axis_percentage_basis
                .value()
                .map(|basis| PhysicalContentHeight::new(content_box_pt(basis.points()))),
            inline_percentage_basis,
            grid_axis_percentage_basis,
            grid_axis_gap,
            grid_axis_content_alignment,
        );
        let auto_repeat_geometry = auto_repeat_resolution
            .as_ref()
            .map(|resolution| resolution.geometry(grid_axis_content_alignment, grid_axis_size));
        let auto_repeat_geometry_resolves_item_percentages = auto_repeat_geometry.is_some();
        let auto_row_geometry = (axis == GridLanesAxis::Rows)
            .then(|| {
                grid_lanes_auto_row_offsets(
                    grid_axis_tracks,
                    children,
                    &layout.items,
                    inline_percentage_basis,
                    style.row_gap.clone(),
                )
            })
            .flatten();
        let has_auto_row_geometry = auto_row_geometry.is_some();
        let auto_column_geometry = (axis == GridLanesAxis::Columns)
            .then(|| {
                self.grid_lanes_auto_column_offsets(
                    grid_axis_tracks,
                    children,
                    stylesheets,
                    inline_percentage_basis,
                    style.column_gap.clone(),
                    style.justify_content,
                )
            })
            .flatten();
        let taffy_gutters = match axis {
            GridLanesAxis::Columns => &layout.gap_gutters.columns,
            GridLanesAxis::Rows => &layout.gap_gutters.rows,
        };
        let geometry = resolved_grid_axis
            .and_then(GridLanesTrackGeometry::from_resolved_subgrid_axis)
            .or_else(|| {
                fixed_track_sizes
                    .as_deref()
                    .and_then(|sizes| {
                        GridLanesTrackGeometry::from_track_sizes(sizes, grid_axis_gap)
                    })
                    .map(|geometry| {
                        geometry.with_content_alignment(grid_axis_content_alignment, grid_axis_size)
                    })
                    .or(auto_repeat_geometry)
                    .or(auto_row_geometry)
                    .or(auto_column_geometry)
                    .or_else(|| {
                        GridLanesTrackGeometry::from_grid_layout_offsets(
                            taffy_line_offsets,
                            taffy_gutters,
                        )
                    })
            });
        let Some(geometry) = geometry else {
            return layout;
        };
        let lane_count = geometry.track_count();
        if lane_count == 0 || layout.items.len() != children.len() {
            return layout;
        }

        let stacking_gap = match axis {
            GridLanesAxis::Columns => used_grid_lanes_gap(
                style.row_gap.clone(),
                PercentageBasis::definite(layout_pt(layout.height.points())),
            ),
            GridLanesAxis::Rows => {
                used_grid_lanes_gap(style.column_gap.clone(), inline_percentage_basis)
            }
        };
        // The Grid probe intentionally treats cyclic item percentages as
        // automatic while sizing intrinsic tracks.  Grid Lanes needs those
        // same values again before it packs the perpendicular axis: the
        // packed extent is a final item-size input, not an intrinsic track
        // contribution.  Keep the estimate and the correction in the shared
        // Grid sizing layer, then consume its result below for both axes.
        // <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
        let final_estimates = children
            .iter()
            .map(|child| {
                self.estimate_grid_item_size(
                    child,
                    stylesheets,
                    width.points(),
                    grid_percentage_basis(
                        Some(content_box_pt(width.points())),
                        GridAvailableSizeSource::ContainerInlineSize,
                    ),
                    grid_percentage_basis(None, GridAvailableSizeSource::ContainerBlockSize),
                )
            })
            .collect::<Vec<_>>();
        let mut lane_ends = vec![0.0_f32; lane_count];
        let mut placed = layout.items.clone();
        let mut order = (0..children.len()).collect::<Vec<_>>();
        order.sort_by_key(|&index| (children[index].style.order, index));
        // The cursor makes equally-good choices progress in grid order rather
        // than repeatedly returning to the first track.
        let mut auto_placement_cursor = 0;
        let flow_tolerance = grid_lanes_flow_tolerance(style, inline_percentage_basis);
        let uses_dense_packing = matches!(
            style.grid_auto_flow,
            css::GridAutoFlow::RowDense | css::GridAutoFlow::ColumnDense
        );
        let mut occupied = vec![Vec::<GridLanesOccupiedInterval>::new(); lane_count];
        let grid_axis_line_names = resolved_grid_axis
            .map(|axis| axis.line_names().to_vec())
            .or_else(|| {
                auto_repeat_resolution
                    .as_ref()
                    .map(|resolution| resolution.line_names.clone())
            })
            .or_else(|| grid_lanes_explicit_line_names(grid_axis_tracks));
        let mut final_grid_axis_line_names =
            grid_axis_line_names.clone().unwrap_or_else(|| match axis {
                GridLanesAxis::Columns => layout.column_line_names.clone(),
                GridLanesAxis::Rows => layout.row_line_names.clone(),
            });
        let final_grid_axis_offsets = geometry.line_offsets();
        final_grid_axis_line_names.resize_with(final_grid_axis_offsets.len(), Vec::new);
        final_grid_axis_line_names.truncate(final_grid_axis_offsets.len());
        layout.set_physical_grid_axis_topology(
            match axis {
                GridLanesAxis::Columns => GridAxis::Column,
                GridLanesAxis::Rows => GridAxis::Row,
            },
            final_grid_axis_offsets,
            geometry.track_sizes(),
            final_grid_axis_line_names,
        );

        for index in order {
            let child = &children[index];
            let placement_provenance = grid_lanes_item_placement(style, child)
                .expect("Grid Lanes placement is only queried for a Grid Lanes container");
            // The ordinary Grid probe has already measured this item using
            // the shared intrinsic-sizing pipeline.  Grid Lanes changes its
            // placement, not its grid-axis intrinsic size, so retain that
            // measurement for non-stretch self-alignment.
            let measured_item = &placed[index];
            let span = grid_lanes_span(axis, child, lane_count);
            let (mut range, auto_placed) = match resolved_grid_axis
                .and_then(|resolved_axis| {
                    let (start, end) = axis.placements(child);
                    matches!(
                        placement_provenance,
                        GridLanesItemPlacement::Definite { .. }
                    )
                    .then(|| resolved_axis.resolved_range(start, end, 1))
                    .and_then(|range| {
                        let range = range.track_range();
                        let start = range.start;
                        let end = range.end;
                        (start < end && end <= lane_count).then_some(start..end)
                    })
                })
                .or_else(|| {
                    auto_repeat_resolution
                        .as_ref()
                        .and_then(|resolution| resolution.resolved_range(axis, child))
                })
                .or_else(|| {
                    grid_lanes_fixed_range(
                        axis,
                        child,
                        lane_count,
                        span,
                        grid_axis_line_names.as_deref(),
                    )
                }) {
                Some(range) => (range, false),
                None => (
                    grid_lanes_shortest_range(
                        &lane_ends,
                        &geometry.active,
                        span,
                        auto_placement_cursor,
                        flow_tolerance,
                    ),
                    true,
                ),
            };
            if auto_placed {
                auto_placement_cursor = range.end;
                if matches!(
                    style.grid_lanes_direction,
                    css::GridLanesDirection::Axis {
                        track_reverse: true,
                        ..
                    }
                ) {
                    range = grid_lanes_reverse_range(range, lane_count);
                }
            }
            let normal_range = range.clone();
            let normal_area = grid_lanes_item_area(axis, &normal_range);
            let measurement_context = GridLanesItemMeasurementContext {
                stylesheets,
                parent_style: style,
                parent_layout: &layout,
                area: normal_area,
                placement: placement_provenance,
                inline_percentage_basis,
                axis,
            };
            let stacking_start = lane_ends[normal_range.clone()]
                .iter()
                .cloned()
                .fold(0.0_f32, f32::max);
            let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
            let final_percentage_size = match axis {
                GridLanesAxis::Columns => resolve_grid_item_final_percentage_size(
                    child,
                    &final_estimates[index],
                    measured_item,
                    geometry.area_size(&range),
                    layout.height.points(),
                    width,
                ),
                GridLanesAxis::Rows => resolve_grid_item_final_percentage_size(
                    child,
                    &final_estimates[index],
                    measured_item,
                    width.points(),
                    geometry.area_size(&range),
                    width,
                ),
            };
            let (item_width, item_height, grid_axis_offset) = match axis {
                GridLanesAxis::Columns => {
                    let area_width = geometry.area_size(&range);
                    let measured_width = final_percentage_size.width.map_or_else(
                        || {
                            if auto_repeat_geometry_resolves_item_percentages {
                                grid_lanes_grid_axis_specified_border_size(
                                    axis,
                                    &child.style,
                                    layout_pt(area_width),
                                )
                                .map(|size| size.points())
                                .unwrap_or(measured_item.width())
                            } else {
                                measured_item.width()
                            }
                        },
                        SemanticLengthExt::points,
                    );
                    let (item_width, grid_axis_offset) = grid_lanes_grid_axis_alignment(
                        axis,
                        style,
                        &child.style,
                        area_width,
                        margins.left.points(),
                        margins.right.points(),
                        measured_width,
                    );
                    let item_height = final_percentage_size
                        .height
                        .map_or_else(
                            || {
                                self.grid_lanes_item_border_block_size(
                                    child,
                                    item_width,
                                    measurement_context,
                                )
                                .points()
                            },
                            SemanticLengthExt::points,
                        )
                        .max(0.0);
                    (item_width, item_height, grid_axis_offset)
                }
                GridLanesAxis::Rows => {
                    let area_height = geometry.area_size(&range);
                    let measured_height = final_percentage_size.height.map_or_else(
                        || {
                            if auto_repeat_geometry_resolves_item_percentages {
                                grid_lanes_grid_axis_specified_border_size(
                                    axis,
                                    &child.style,
                                    layout_pt(area_height),
                                )
                                .map(|size| size.points())
                                .unwrap_or(measured_item.height())
                            } else {
                                measured_item.height()
                            }
                        },
                        SemanticLengthExt::points,
                    );
                    let (item_height, grid_axis_offset) = grid_lanes_grid_axis_alignment(
                        axis,
                        style,
                        &child.style,
                        area_height,
                        margins.top.points(),
                        margins.bottom.points(),
                        measured_height,
                    );
                    let item_width = final_percentage_size
                        .width
                        .map_or_else(
                            || {
                                self.grid_lanes_item_border_block_size(
                                    child,
                                    item_height,
                                    measurement_context,
                                )
                                .points()
                            },
                            SemanticLengthExt::points,
                        )
                        .max(0.0);
                    (item_width, item_height, grid_axis_offset)
                }
            };
            let stacking_size = match axis {
                GridLanesAxis::Columns => {
                    item_height + margins.top.points() + margins.bottom.points()
                }
                GridLanesAxis::Rows => item_width + margins.left.points() + margins.right.points(),
            }
            .max(0.0);
            let dense_placement = if uses_dense_packing {
                grid_lanes_dense_backfill_position(
                    &occupied,
                    &geometry,
                    &normal_range,
                    stacking_size + stacking_gap,
                    stacking_start,
                    flow_tolerance,
                )
            } else {
                None
            };
            if let Some((dense_range, _)) = &dense_placement {
                range = dense_range.clone();
            }
            let placed_start = dense_placement
                .as_ref()
                .map(|(_, start)| *start)
                .unwrap_or(stacking_start);
            let item = &mut placed[index];
            item.set_grid_lanes_placement(placement_provenance);
            if final_percentage_size.width.is_some() {
                item.mark_final_percentage_axis(GridAxis::Column);
            }
            if final_percentage_size.height.is_some() {
                item.mark_final_percentage_axis(GridAxis::Row);
            }
            match axis {
                GridLanesAxis::Columns => {
                    let area_x = geometry.area_start(&range);
                    item.set_axis_geometry(GridAxis::Column, area_x + grid_axis_offset, item_width);
                    item.set_axis_geometry(
                        GridAxis::Row,
                        placed_start + margins.top.points(),
                        item_height,
                    );
                }
                GridLanesAxis::Rows => {
                    let area_y = geometry.area_start(&range);
                    item.set_axis_geometry(GridAxis::Row, area_y + grid_axis_offset, item_height);
                    item.set_axis_geometry(
                        GridAxis::Column,
                        placed_start + margins.left.points(),
                        item_width,
                    );
                }
            }
            item.area = Some(match axis {
                GridLanesAxis::Columns => GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: u16::try_from(range.start + 1).unwrap_or(u16::MAX),
                    column_end: u16::try_from(range.end + 1).unwrap_or(u16::MAX),
                },
                GridLanesAxis::Rows => GridItemArea {
                    row_start: u16::try_from(range.start + 1).unwrap_or(u16::MAX),
                    row_end: u16::try_from(range.end + 1).unwrap_or(u16::MAX),
                    column_start: 1,
                    column_end: 2,
                },
            });
            let end = stacking_start + stacking_size.max(0.0) + stacking_gap;
            let occupied_end = placed_start + stacking_size + stacking_gap;
            for lane in range.clone() {
                occupied[lane].push(GridLanesOccupiedInterval {
                    start: placed_start,
                    end: occupied_end,
                });
            }
            // Dense placement is pure backfilling: it must not change the
            // running positions or auto-placement cursor used by later items.
            if dense_placement.is_none() {
                for lane_end in &mut lane_ends[range] {
                    *lane_end = end;
                }
            }
        }

        let stacking_extent =
            (lane_ends.iter().cloned().fold(0.0_f32, f32::max) - stacking_gap).max(0.0);
        if axis == GridLanesAxis::Columns {
            // A definite stacking-axis size belongs to the container, not to
            // the packed content.  The lanes may overflow it, just as block
            // contents overflow a fixed-height block; only an automatic
            // height is replaced by the stacking range.
            // <https://drafts.csswg.org/css-grid-3/#sizing-grid-containers>
            if used_length_percentage_or_auto(
                style.box_values.height.value().clone(),
                PercentageBasis::definite(layout_pt(layout.height.points())),
            )
            .is_none()
            {
                layout.height = PhysicalContentHeight::new(content_box_pt(stacking_extent));
            }
            layout.row_line_offsets = vec![0.0, layout.height.points()];
        } else {
            // Row lanes establish the physical block-axis track geometry. A
            // two-dimensional Grid probe may create implicit rows from its
            // own auto-placement, so preserve the authored lane rows here.
            if has_auto_row_geometry
                && used_length_percentage_or_auto(
                    style.box_values.height.value().clone(),
                    PercentageBasis::definite(layout_pt(layout.height.points())),
                )
                .is_none()
            {
                layout.height = PhysicalContentHeight::new(content_box_pt(
                    geometry
                        .ends
                        .last()
                        .cloned()
                        .unwrap_or_else(|| layout.height.points()),
                ));
                layout.row_line_offsets = geometry.line_offsets();
            }
        }
        // Content distribution positions the packed stacking range as one
        // alignment subject. Reverse fill reverses that range before it is
        // offset in the definite container, so `start` and `end` retain their
        // physical content-alignment meaning.
        let (stacking_size, stacking_alignment) = match axis {
            GridLanesAxis::Columns => (layout.height.points(), style.align_content),
            GridLanesAxis::Rows => (width.points(), style.justify_content),
        };
        let stacking_offset =
            grid_lanes_content_alignment_offset(stacking_alignment, stacking_size, stacking_extent);
        match axis {
            GridLanesAxis::Columns => {
                for (item, child) in placed.iter_mut().zip(children) {
                    let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
                    if matches!(
                        style.grid_lanes_direction,
                        css::GridLanesDirection::Axis {
                            fill_reverse: true,
                            ..
                        }
                    ) {
                        item.set_axis_geometry(
                            GridAxis::Row,
                            stacking_extent - (item.y() + item.height() + margins.bottom.points())
                                + margins.top.points(),
                            item.height(),
                        );
                    }
                    item.set_axis_geometry(
                        GridAxis::Row,
                        item.y() + stacking_offset,
                        item.height(),
                    );
                }
            }
            GridLanesAxis::Rows => {
                for (item, child) in placed.iter_mut().zip(children) {
                    let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
                    if matches!(
                        style.grid_lanes_direction,
                        css::GridLanesDirection::Axis {
                            fill_reverse: true,
                            ..
                        }
                    ) {
                        item.set_axis_geometry(
                            GridAxis::Column,
                            stacking_extent - (item.x() + item.width() + margins.right.points())
                                + margins.left.points(),
                            item.width(),
                        );
                    }
                    item.set_axis_geometry(
                        GridAxis::Column,
                        item.x() + stacking_offset,
                        item.width(),
                    );
                }
            }
        }
        match axis {
            GridLanesAxis::Columns => {
                layout.column_line_offsets = geometry.line_offsets();
                let mut names = resolved_grid_axis
                    .map(|axis| axis.physical_line_names().to_vec())
                    .or_else(|| {
                        auto_repeat_resolution
                            .as_ref()
                            .map(|resolution| resolution.line_names.clone())
                    })
                    .unwrap_or_else(|| layout.column_line_names.clone());
                names.resize_with(layout.column_line_offsets.len(), Vec::new);
                names.truncate(layout.column_line_offsets.len());
                layout.column_line_names = names;
            }
            GridLanesAxis::Rows if !has_auto_row_geometry => {
                layout.row_line_offsets = geometry.line_offsets();
                let mut names = resolved_grid_axis
                    .map(|axis| axis.physical_line_names().to_vec())
                    .or_else(|| {
                        auto_repeat_resolution
                            .as_ref()
                            .map(|resolution| resolution.line_names.clone())
                    })
                    .unwrap_or_else(|| layout.row_line_names.clone());
                names.resize_with(layout.row_line_offsets.len(), Vec::new);
                names.truncate(layout.row_line_offsets.len());
                layout.row_line_names = names;
            }
            GridLanesAxis::Rows => {}
        }
        // Replaced items with an automatic size retain an intrinsic used size
        // independently of their track contribution. Apply that geometry
        // after Lanes has established the final origin; doing so earlier can
        // make an intrinsic image incorrectly participate in lane packing.
        apply_grid_replaced_item_size_corrections(style, children, &final_estimates, &mut placed);
        layout.items = placed;
        layout
    }

    /// Measure an item's intrinsic stacking-axis border-box size once its lane
    /// span has made the grid-axis available size definite.
    ///
    /// A Grid Lanes item cannot reuse the height produced by a conventional
    /// two-dimensional grid: that height may have been stretched to an
    /// unrelated row.  The lanes algorithm instead resolves the grid-axis
    /// area, then measures the independent grid item formatting context with
    /// that definite inline size before updating the lane's packing cursor.
    /// <https://drafts.csswg.org/css-grid-3/#grid-lanes-layout-and-placement-algorithm>
    fn grid_lanes_item_border_block_size(
        &mut self,
        child: &GridChild<'_>,
        grid_axis_border_size: f32,
        context: GridLanesItemMeasurementContext<'_>,
    ) -> BorderBoxLength {
        // Column lanes use their grid area as the item's physical inline
        // size. Row lanes stack along the physical inline axis instead, so
        // their width must be measured against the container's inline
        // percentage basis rather than the row-track (block-axis) size.
        // <https://drafts.csswg.org/css-grid-3/#grid-lanes-layout-and-placement-algorithm>
        let available_width = match context.axis {
            GridLanesAxis::Columns => grid_lanes_content_inline_size(
                child,
                border_box_pt(grid_axis_border_size),
                context.inline_percentage_basis,
            ),
            GridLanesAxis::Rows => {
                content_box_pt(grid_lanes_basis_points(context.inline_percentage_basis))
            }
        };
        let estimate = if let Some(subgrid_context) = ResolvedSubgridContext::from_parent(
            context.parent_style,
            context.parent_layout,
            &child.style,
            context.area,
            Some(context.placement),
        ) {
            self.with_resolved_subgrid_context(subgrid_context, |layout| {
                layout.estimate_grid_item_size(
                    child,
                    context.stylesheets,
                    available_width.points(),
                    grid_percentage_basis(
                        Some(available_width),
                        GridAvailableSizeSource::ContainerInlineSize,
                    ),
                    PercentageBasis::indefinite(),
                )
            })
        } else {
            self.estimate_grid_item_size(
                child,
                context.stylesheets,
                available_width.points(),
                grid_percentage_basis(
                    Some(available_width),
                    GridAvailableSizeSource::ContainerInlineSize,
                ),
                PercentageBasis::indefinite(),
            )
        };
        border_box_pt(match context.axis {
            GridLanesAxis::Columns => {
                estimate.height.points()
                    + grid_lanes_vertical_non_content(&child.style, context.inline_percentage_basis)
                        .points()
            }
            GridLanesAxis::Rows => {
                estimate.width.points()
                    + grid_lanes_horizontal_non_content(
                        &child.style,
                        context.inline_percentage_basis,
                    )
                    .points()
            }
        })
    }

    /// Build the grid-axis geometry for simple all-auto column lanes.
    ///
    /// Grid Lanes differs from ordinary two-dimensional Grid placement here:
    /// an auto-placed item contributes its hypothetical size to each eligible
    /// grid-axis auto track.  The ordinary Taffy probe has already assigned
    /// each item a conventional grid column, so reusing its final widths would
    /// incorrectly make sibling lanes depend on that unrelated placement.
    /// <https://drafts.csswg.org/css-grid-3/#track-sizing>
    #[allow(dead_code, clippy::too_many_arguments)]
    fn grid_lanes_auto_column_offsets(
        &mut self,
        tracks: &css::GridTrackList,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        inline_percentage_basis: GridLanesPercentageBasis,
        gap: css::ComputedGap,
        content_alignment: css::ContentAlignment,
    ) -> Option<GridLanesTrackGeometry> {
        let column_count = grid_lanes_all_auto_track_count(tracks)?;
        if column_count == 0
            // A nested grid can have a subgridded grid-axis contribution. The
            // simple all-auto path has no parent-track contribution protocol,
            // so defer to the shared Grid pass until that propagation exists.
            || children
                .iter()
                .any(|child| child.style.display.inner == DisplayInner::Grid)
            || grid_lanes_has_out_of_range_explicit_placement(
                children,
                GridLanesAxis::Columns,
                column_count,
            )
        {
            return None;
        }

        let mut track_sizes = vec![0.0_f32; column_count];
        let mut auto_contribution = 0.0_f32;
        for child in children {
            let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
            let estimate = self.estimate_grid_item_size(
                child,
                stylesheets,
                grid_lanes_basis_points(inline_percentage_basis),
                grid_percentage_basis(
                    inline_percentage_basis
                        .value()
                        .map(|value| content_box_pt(value.points())),
                    GridAvailableSizeSource::ContainerInlineSize,
                ),
                PercentageBasis::indefinite(),
            );
            let contribution = estimate.min_width.points()
                + grid_lanes_horizontal_non_content(&child.style, inline_percentage_basis).points()
                + margins.left.points()
                + margins.right.points();
            let span = grid_lanes_span(GridLanesAxis::Columns, child, column_count);
            if let Some(range) =
                grid_lanes_fixed_range(GridLanesAxis::Columns, child, column_count, span, None)
                && range.len() == 1
            {
                track_sizes[range.start] = track_sizes[range.start].max(contribution);
            } else if span == 1 {
                auto_contribution = auto_contribution.max(contribution);
            }
        }
        for size in &mut track_sizes {
            *size = size.max(auto_contribution);
        }
        GridLanesTrackGeometry::from_track_sizes(
            &track_sizes,
            used_grid_lanes_gap(gap, inline_percentage_basis),
        )
        .map(|geometry| {
            geometry.with_auto_track_stretch(
                &vec![true; column_count],
                content_alignment,
                grid_lanes_basis_points(inline_percentage_basis),
            )
        })
    }

    /// Resolve a simple intrinsic auto-repeat before its item percentages have
    /// a definite grid area.
    ///
    /// An auto-repeat count is itself part of the grid-axis track-sizing
    /// algorithm. Percentages on a lane item cannot resolve against the whole
    /// container while that count is being chosen; doing so feeds a final-size
    /// percentage back into intrinsic track sizing. Use the item's
    /// max-content contribution instead, then let item percentages resolve
    /// against the selected track area during replay.
    /// <https://drafts.csswg.org/css-grid-1/#algo-content> and
    /// <https://drafts.csswg.org/css-grid-3/#track-sizing>
    #[allow(dead_code, clippy::too_many_arguments)]
    fn grid_lanes_auto_repeat_auto_geometry(
        &mut self,
        style: &ComputedStyle,
        axis: GridLanesAxis,
        tracks: &css::GridTrackList,
        children: &[GridChild<'_>],
        items: &[GridItemLayout],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        root_height: Option<PhysicalContentHeight>,
        inline_percentage_basis: GridLanesPercentageBasis,
        available_size: GridLanesPercentageBasis,
        gap: f32,
        content_alignment: css::ContentAlignment,
    ) -> Option<GridLanesTrackGeometry> {
        if let Some(resolution) = self.grid_lanes_resolve_intrinsic_auto_repeat(
            style,
            axis,
            tracks,
            children,
            items,
            stylesheets,
            width,
            root_height,
            inline_percentage_basis,
            available_size,
            gap,
            content_alignment,
        ) {
            return Some(
                resolution.geometry(content_alignment, grid_lanes_basis_points(available_size)),
            );
        }
        let css::GridTrackList::Tracks { components, .. } = tracks else {
            return None;
        };
        let [css::GridTrackListComponent::Repeat(_, repeat)] = components.as_slice() else {
            return None;
        };
        if !matches!(
            repeat.count,
            css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
        ) {
            return None;
        }
        let [css::GridTrackListComponent::Track(_, track)] = repeat.tracks.as_slice() else {
            return None;
        };
        let intrinsic_track = grid_lanes_intrinsic_auto_repeat_track(track.clone())?;
        if children.len() != items.len() {
            return None;
        }

        let mut contribution = 0.0_f32;
        for (child, item) in children.iter().zip(items) {
            let span = grid_lanes_span(axis, child, usize::MAX);
            let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
            let (specified, measured, non_content, margin_sum) = match axis {
                GridLanesAxis::Columns => (
                    child.style.box_values.width.clone(),
                    item.width(),
                    grid_lanes_horizontal_non_content(&child.style, inline_percentage_basis),
                    margins.left.points() + margins.right.points(),
                ),
                GridLanesAxis::Rows => (
                    child.style.box_values.height.value().clone(),
                    item.height(),
                    grid_lanes_vertical_non_content(&child.style, inline_percentage_basis),
                    margins.top.points() + margins.bottom.points(),
                ),
            };
            let percentage_sized = matches!(
                specified,
                css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value)
                    if value.contains_percentage()
            );
            let estimate = self.estimate_grid_item_size(
                child,
                stylesheets,
                grid_lanes_basis_points(inline_percentage_basis),
                grid_percentage_basis(
                    inline_percentage_basis
                        .value()
                        .map(|value| content_box_pt(value.points())),
                    GridAvailableSizeSource::ContainerInlineSize,
                ),
                PercentageBasis::indefinite(),
            );
            let (min_content, max_content) = match axis {
                GridLanesAxis::Columns => {
                    (estimate.min_width.points(), estimate.content_width.points())
                }
                GridLanesAxis::Rows => (
                    estimate.min_height.points(),
                    estimate.content_height.points(),
                ),
            };
            let item_contribution =
                if matches!(intrinsic_track, GridLanesIntrinsicAutoRepeatTrack::Auto)
                    && !percentage_sized
                {
                    used_length_percentage_or_auto(specified, available_size)
                        .map(|size| {
                            let size = size.points();
                            if child.style.box_sizing == BoxSizing::ContentBox {
                                size + non_content.points()
                            } else {
                                size
                            }
                        })
                        .unwrap_or(measured)
                        + margin_sum
                } else {
                    let intrinsic_contribution = match intrinsic_track {
                        GridLanesIntrinsicAutoRepeatTrack::Auto
                        | GridLanesIntrinsicAutoRepeatTrack::MaxContent => max_content,
                        GridLanesIntrinsicAutoRepeatTrack::MinContent => min_content,
                        GridLanesIntrinsicAutoRepeatTrack::FitContent(ref limit) => max_content
                            .min(used_length_percentage((*limit).clone(), available_size).points()),
                    };
                    intrinsic_contribution + non_content.points() + margin_sum
                };
            // A spanning item's contribution is distributed to the auto
            // tracks that form its hypothetical grid area. The intervening
            // gutters are already part of that area and cannot become track
            // breadth themselves.
            let track_contribution =
                (item_contribution - gap * span.saturating_sub(1) as f32).max(0.0) / span as f32;
            contribution = contribution.max(track_contribution);
        }
        if contribution <= 0.0 {
            return None;
        }
        let count = ((grid_lanes_basis_points(available_size) + gap) / (contribution + gap))
            .floor()
            .max(1.0) as usize;
        GridLanesTrackGeometry::from_track_sizes(&vec![contribution; count], gap)
    }

    /// Resolve the Level 3 intrinsic auto-repeat as a complete grid-axis
    /// geometry. This deliberately runs before ordinary Grid Lanes placement:
    /// the hypothetical sizing pass ignores explicit line positions, while
    /// the final geometry retains enough implicit tracks for them.
    #[allow(clippy::too_many_arguments)]
    fn grid_lanes_resolve_intrinsic_auto_repeat(
        &mut self,
        style: &ComputedStyle,
        axis: GridLanesAxis,
        tracks: &css::GridTrackList,
        children: &[GridChild<'_>],
        items: &[GridItemLayout],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        root_height: Option<PhysicalContentHeight>,
        _inline_percentage_basis: GridLanesPercentageBasis,
        available_size: GridLanesPercentageBasis,
        gap: f32,
        content_alignment: css::ContentAlignment,
    ) -> Option<GridLanesAutoRepeatResolution> {
        let css::GridTrackList::Tracks { components, .. } = tracks else {
            return None;
        };
        if children.len() != items.len() {
            return None;
        }

        let mut fixed_sizes = Vec::new();
        let mut repeat_start = None;
        let mut repeated_tracks = None;
        let mut auto_fit = false;
        for component in components {
            match component {
                css::GridTrackListComponent::Track(_, track) => {
                    fixed_sizes.push(grid_lanes_definite_track_size(
                        track.clone(),
                        available_size,
                    )?);
                }
                css::GridTrackListComponent::Repeat(_, repeat) => match repeat.count {
                    css::GridRepeatCount::Number(count) => {
                        for _ in 0..count {
                            for component in &repeat.tracks {
                                let css::GridTrackListComponent::Track(_, track) = component else {
                                    return None;
                                };
                                fixed_sizes.push(grid_lanes_definite_track_size(
                                    track.clone(),
                                    available_size,
                                )?);
                            }
                        }
                    }
                    css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => {
                        if repeated_tracks.is_some() {
                            return None;
                        }
                        let tracks = repeat
                            .tracks
                            .iter()
                            .map(|component| match component {
                                css::GridTrackListComponent::Track(_, track) => Some(track.clone()),
                                css::GridTrackListComponent::Repeat(_, _) => None,
                            })
                            .collect::<Option<Vec<_>>>()?;
                        repeat_start = Some(fixed_sizes.len());
                        auto_fit = matches!(repeat.count, css::GridRepeatCount::AutoFit);
                        repeated_tracks = Some(tracks);
                    }
                },
            }
        }
        let repeat_start = repeat_start?;
        let repeated_tracks = repeated_tracks?;
        if repeated_tracks.is_empty() {
            return None;
        }

        // CSS Grid Level 3 requires enough virtual repetitions to expose all
        // automatic starts for the largest span. The positions themselves are
        // intentionally ignored in this sizing phase.
        let largest_span = children
            .iter()
            .map(|child| grid_lanes_span(axis, child, usize::MAX))
            .max()
            .unwrap_or(1);
        let hypothetical_repetitions = 2 + largest_span.saturating_sub(2) / repeated_tracks.len();
        debug_assert!(hypothetical_repetitions >= 2);

        let repeated_sizes = grid_lanes_virtual_mixed_auto_repeat_sizes(
            axis,
            children,
            items,
            &repeated_tracks,
            hypothetical_repetitions,
        )
        .or_else(|| {
            self.grid_lanes_virtual_intrinsic_auto_repeat_sizes(
                style,
                axis,
                children,
                stylesheets,
                width,
                root_height,
                hypothetical_repetitions,
                repeat_start,
                repeated_tracks.len(),
            )
        })?;
        let repeated_auto_sized = repeated_tracks
            .iter()
            .map(|track| {
                matches!(
                    grid_lanes_intrinsic_auto_repeat_track(track.clone()),
                    Some(GridLanesIntrinsicAutoRepeatTrack::Auto)
                )
            })
            .collect::<Vec<_>>();
        let fixed_size = fixed_sizes.iter().sum::<f32>();
        let fixed_track_count = fixed_sizes.len();
        let repeat_size = repeated_sizes.iter().sum::<f32>();
        let repeat_track_count = repeated_sizes.len();
        let first_repetition_size = fixed_size
            + repeat_size
            + gap * (fixed_track_count + repeat_track_count).saturating_sub(1) as f32;
        let available = grid_lanes_basis_points(available_size);
        let repetitions = if available_size.is_definite() && first_repetition_size <= available {
            ((available - first_repetition_size) / (repeat_size + gap * repeat_track_count as f32))
                .floor()
                .max(0.0) as usize
                + 1
        } else {
            1
        };
        let mut suffix_sizes = fixed_sizes.split_off(repeat_start);
        let prefix_sizes = fixed_sizes;
        let mut resolved_tracks = prefix_sizes
            .into_iter()
            .map(|used_size| GridLanesResolvedAutoRepeatTrack {
                source: GridLanesAutoRepeatTrackSource::FixedPrefix,
                used_size,
                auto_sized: false,
                collapsed: false,
            })
            .collect::<Vec<_>>();
        for repetition in 0..repetitions {
            resolved_tracks.extend(repeated_sizes.iter().enumerate().map(|(slot, &used_size)| {
                GridLanesResolvedAutoRepeatTrack {
                    source: GridLanesAutoRepeatTrackSource::Repeated { repetition, slot },
                    used_size,
                    auto_sized: repeated_auto_sized[slot],
                    collapsed: false,
                }
            }));
        }
        resolved_tracks.extend(suffix_sizes.drain(..).map(|used_size| {
            GridLanesResolvedAutoRepeatTrack {
                source: GridLanesAutoRepeatTrackSource::FixedSuffix,
                used_size,
                auto_sized: false,
                collapsed: false,
            }
        }));
        let repeat_range = repeat_start..repeat_start + repetitions * repeat_track_count;
        let line_names = grid_lanes_expanded_auto_repeat_line_names(tracks, repetitions)?;
        let explicit_line_count = resolved_tracks.len() + 1;
        let mut resolution = GridLanesAutoRepeatResolution {
            tracks: resolved_tracks,
            explicit_line_count,
            line_names,
            repeat_range,
            gap,
            materialized_geometry: None,
        };

        // Explicit placement can extend the final grid, but it must not have
        // influenced the hypothetical repeat-size pass above.
        let required_track_count =
            grid_lanes_required_track_count(axis, children, resolution.tracks.len());
        resolution.append_end_implicit_tracks(
            required_track_count.saturating_sub(resolution.tracks.len()),
            grid_lanes_auto_implicit_track_size(axis, style, children, items),
            grid_lanes_implicit_track_is_auto(axis, style),
        );
        // Once the intrinsic repeat count is known, ordinary Grid owns the
        // used breadth of the resulting explicit and implicit track list.
        // Replaying that *materialized* template is intentionally distinct
        // from the preceding hypothetical pass: it observes authored line
        // placement and lets `grid-auto-*` size the implicit tracks.  In
        // particular, an implicit auto track must not inherit the largest
        // repeated-track breadth merely because it is adjacent to the
        // repeat.
        // <https://drafts.csswg.org/css-grid-3/#grid-axis-track-sizing>
        if !auto_fit {
            resolution.materialized_geometry = self
                .grid_lanes_materialized_auto_repeat_geometry(
                    style,
                    axis,
                    children,
                    stylesheets,
                    width,
                    root_height,
                    repetitions,
                )
                .filter(|geometry| geometry.track_count() == resolution.tracks.len());
        }
        if auto_fit {
            resolution.apply_auto_fit(
                axis,
                children,
                items,
                grid_lanes_flow_tolerance(style, _inline_percentage_basis),
            );
        }
        let _ = content_alignment;
        let _ = available;
        Some(resolution)
    }

    /// Replay an intrinsic auto-repeat after its count has become definite.
    ///
    /// This is the final, placement-aware sizing pass. Unlike the virtual
    /// pass, it leaves item placement intact and therefore creates the
    /// normal implicit tracks required by spans extending past the explicit
    /// grid. The returned sizes are already used sizes in the container's
    /// coordinate space, including Grid's `normal`/stretch distribution.
    #[allow(clippy::too_many_arguments)]
    fn grid_lanes_materialized_auto_repeat_geometry(
        &mut self,
        style: &ComputedStyle,
        axis: GridLanesAxis,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        root_height: Option<PhysicalContentHeight>,
        repetitions: usize,
    ) -> Option<GridLanesTrackGeometry> {
        let mut materialized_style = style.clone();
        let template = match axis {
            GridLanesAxis::Columns => &mut materialized_style.grid_template_columns,
            GridLanesAxis::Rows => &mut materialized_style.grid_template_rows,
        };
        let css::GridTrackList::Tracks { components, .. } = template else {
            return None;
        };
        let repetitions = u16::try_from(repetitions).ok()?;
        let mut materialized = false;
        for component in components {
            let css::GridTrackListComponent::Repeat(_, repeat) = component else {
                continue;
            };
            if matches!(
                repeat.count,
                css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
            ) {
                repeat.count = css::GridRepeatCount::Number(repetitions);
                materialized = true;
            }
        }
        materialized.then_some(())?;
        let layout = self.compute_grid_layout_pass(
            &materialized_style,
            children,
            stylesheets,
            None,
            &[],
            GridLayoutPassConfig {
                width,
                root_height,
                item_height_basis: grid_percentage_basis(
                    root_height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                row_gap_basis: grid_percentage_basis(
                    root_height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: None,
                item_placement_overrides: Vec::new(),
                baseline_plan: None,
            },
        )?;
        match axis {
            GridLanesAxis::Columns => GridLanesTrackGeometry::from_grid_layout_offsets(
                &layout.column_line_offsets,
                &layout.gap_gutters.columns,
            ),
            GridLanesAxis::Rows => GridLanesTrackGeometry::from_grid_layout_offsets(
                &layout.row_line_offsets,
                &layout.gap_gutters.rows,
            ),
        }
    }

    /// Size the hypothetical repeat list with the ordinary Grid track-sizing
    /// adapter. Every item is copied to every start at which its span fits;
    /// its authored line placement is deliberately discarded for this pass.
    ///
    /// CSS Grid Level 3 defines these copies as theoretical automatic
    /// positions, whose corresponding repeat slots contribute their maximum
    /// resolved size: <https://drafts.csswg.org/css-grid-3/#grid-axis-track-sizing>.
    #[allow(clippy::too_many_arguments)]
    fn grid_lanes_virtual_intrinsic_auto_repeat_sizes(
        &mut self,
        style: &ComputedStyle,
        axis: GridLanesAxis,
        children: &[GridChild<'_>],
        stylesheets: &Stylesheets<'_>,
        width: PhysicalContentWidth,
        root_height: Option<PhysicalContentHeight>,
        repetitions: usize,
        repeat_start: usize,
        repeat_track_count: usize,
    ) -> Option<Vec<f32>> {
        let mut virtual_style = style.clone();
        let template = match axis {
            GridLanesAxis::Columns => &mut virtual_style.grid_template_columns,
            GridLanesAxis::Rows => &mut virtual_style.grid_template_rows,
        };
        let css::GridTrackList::Tracks { components, .. } = template else {
            return None;
        };
        let repetitions = u16::try_from(repetitions).ok()?;
        for component in components {
            let css::GridTrackListComponent::Repeat(_, repeat) = component else {
                continue;
            };
            if matches!(
                repeat.count,
                css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
            ) {
                repeat.count = css::GridRepeatCount::Number(repetitions);
            }
        }
        match axis {
            GridLanesAxis::Columns => {
                virtual_style.justify_content =
                    css::ContentAlignment::new(css::ContentAlignmentKeyword::Start);
            }
            GridLanesAxis::Rows => {
                virtual_style.align_content =
                    css::ContentAlignment::new(css::ContentAlignmentKeyword::Start);
            }
        }

        let total_track_count = grid_lanes_template_track_count(template)?;
        let mut virtual_children = Vec::new();
        for child in children {
            let span = grid_lanes_span(axis, child, total_track_count);
            for start in 0..=total_track_count.saturating_sub(span) {
                let mut copy = child.clone();
                grid_lanes_set_virtual_axis_placement(&mut copy.style, axis, start, span)?;
                grid_lanes_set_virtual_cross_axis_placement(&mut copy.style, axis)?;
                virtual_children.push(copy);
            }
        }
        let layout = self.compute_grid_layout_pass(
            &virtual_style,
            &virtual_children,
            stylesheets,
            None,
            &[],
            GridLayoutPassConfig {
                width,
                root_height,
                item_height_basis: grid_percentage_basis(
                    root_height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                row_gap_basis: grid_percentage_basis(
                    root_height.map(PhysicalContentHeight::content_box_length),
                    GridAvailableSizeSource::ContainerBlockSize,
                ),
                reported_height: None,
                item_placement_overrides: Vec::new(),
                baseline_plan: None,
            },
        )?;
        let track_sizes = layout.physical_track_sizes(match axis {
            GridLanesAxis::Columns => GridAxis::Column,
            GridLanesAxis::Rows => GridAxis::Row,
        });
        (track_sizes.len() >= repeat_start + usize::from(repetitions) * repeat_track_count)
            .then_some(())?;
        Some(
            (0..repeat_track_count)
                .map(|slot| {
                    (0..usize::from(repetitions))
                        .filter_map(|repetition| {
                            track_sizes.get(repeat_start + repetition * repeat_track_count + slot)
                        })
                        .copied()
                        .fold(0.0_f32, f32::max)
                })
                .collect::<Vec<_>>(),
        )
    }
}

/// A virtual track used only while resolving an intrinsic Grid Lanes
/// auto-repeat. Definite tracks reserve their complete breadth before a
/// spanning item's contribution is distributed to the intrinsic tracks.
/// <https://www.w3.org/TR/css-grid-3/#grid-axis-track-sizing>
#[derive(Clone, Copy)]
enum GridLanesVirtualMixedTrack {
    Fixed(f32),
    Intrinsic,
}

/// Resolve the simple mixed intrinsic/definite repeated list that the shared
/// Grid adapter cannot model: Grid Lanes must place every virtual copy of a
/// spanning item, but definite tracks cannot absorb its intrinsic growth.
///
/// This intentionally handles only an unadorned repeated list. More complex
/// templates continue through the shared adapter until their complete track
/// sizing protocol is represented here.
fn grid_lanes_virtual_mixed_auto_repeat_sizes(
    axis: GridLanesAxis,
    children: &[GridChild<'_>],
    items: &[GridItemLayout],
    repeated_tracks: &[css::GridTrackSize],
    repetitions: usize,
) -> Option<Vec<f32>> {
    if children.len() != items.len() || repeated_tracks.len() < 2 || repetitions == 0 {
        return None;
    }
    let slots = repeated_tracks
        .iter()
        .cloned()
        .map(grid_lanes_virtual_mixed_track)
        .collect::<Option<Vec<_>>>()?;
    if !slots
        .iter()
        .any(|slot| matches!(slot, GridLanesVirtualMixedTrack::Fixed(_)))
        || !slots
            .iter()
            .any(|slot| matches!(slot, GridLanesVirtualMixedTrack::Intrinsic))
    {
        return None;
    }

    let mut track_sizes = (0..repetitions)
        .flat_map(|_| slots.iter().copied())
        .map(|slot| match slot {
            GridLanesVirtualMixedTrack::Fixed(size) => size,
            GridLanesVirtualMixedTrack::Intrinsic => 0.0,
        })
        .collect::<Vec<_>>();
    let mut virtual_items = Vec::new();
    for (child, item) in children.iter().zip(items) {
        let span = grid_lanes_span(axis, child, track_sizes.len());
        let contribution = match axis {
            GridLanesAxis::Columns => item.width(),
            GridLanesAxis::Rows => item.height(),
        }
        .max(0.0);
        for start in 0..=track_sizes.len().saturating_sub(span) {
            virtual_items.push((span, start, contribution));
        }
    }
    virtual_items.sort_by_key(|(span, start, _)| (*span, *start));
    for (span, start, contribution) in virtual_items {
        let range = start..start + span;
        let growable = range
            .clone()
            .filter(|&index| {
                matches!(
                    slots[index % slots.len()],
                    GridLanesVirtualMixedTrack::Intrinsic
                )
            })
            .collect::<Vec<_>>();
        if growable.is_empty() {
            continue;
        }
        let fixed_size = range
            .filter_map(|index| match slots[index % slots.len()] {
                GridLanesVirtualMixedTrack::Fixed(size) => Some(size),
                GridLanesVirtualMixedTrack::Intrinsic => None,
            })
            .sum::<f32>();
        let growth = ((contribution - fixed_size).max(0.0) / growable.len() as f32).max(0.0);
        for index in growable {
            track_sizes[index] = track_sizes[index].max(growth);
        }
    }
    Some(
        (0..slots.len())
            .map(|slot| {
                (0..repetitions)
                    .map(|repetition| track_sizes[repetition * slots.len() + slot])
                    .fold(0.0_f32, f32::max)
            })
            .collect(),
    )
}

fn grid_lanes_virtual_mixed_track(track: css::GridTrackSize) -> Option<GridLanesVirtualMixedTrack> {
    if let Some(intrinsic) = grid_lanes_intrinsic_auto_repeat_track(track.clone()) {
        let _ = intrinsic;
        return Some(GridLanesVirtualMixedTrack::Intrinsic);
    }
    let (
        css::GridMinTrackBreadth::LengthPercentage(min),
        css::GridMaxTrackBreadth::LengthPercentage(max),
    ) = (track.min, track.max)
    else {
        return None;
    };
    if min.contains_percentage() || max.contains_percentage() {
        return None;
    }
    let min = min.length_max_zero().points();
    let max = max.length_max_zero().points();
    ((min - max).abs() < 0.01).then_some(GridLanesVirtualMixedTrack::Fixed(min.max(0.0)))
}

/// Resolve the simple default `grid-auto-* : auto` track used after an
/// intrinsic repeat has formed an end implicit grid. This is a contribution
/// of the implicit track's own sizing function, not a borrowed repeated
/// track size: a spanning item contributes evenly to every track in its
/// candidate area.
fn grid_lanes_auto_implicit_track_size(
    axis: GridLanesAxis,
    style: &ComputedStyle,
    children: &[GridChild<'_>],
    items: &[GridItemLayout],
) -> f32 {
    if !grid_lanes_implicit_track_is_auto(axis, style) {
        return 0.0;
    }
    children
        .iter()
        .zip(items)
        .map(|(child, item)| {
            let span = grid_lanes_span(axis, child, usize::MAX).max(1) as f32;
            let contribution = match axis {
                GridLanesAxis::Columns => item.width(),
                GridLanesAxis::Rows => item.height(),
            };
            (contribution / span).max(0.0)
        })
        .fold(0.0, f32::max)
}

fn grid_lanes_implicit_track_is_auto(axis: GridLanesAxis, style: &ComputedStyle) -> bool {
    let auto_tracks = match axis {
        GridLanesAxis::Columns => &style.grid_auto_columns,
        GridLanesAxis::Rows => &style.grid_auto_rows,
    };
    auto_tracks.as_slice().iter().all(|track| {
        matches!(
            grid_lanes_intrinsic_auto_repeat_track(track.clone()),
            Some(GridLanesIntrinsicAutoRepeatTrack::Auto)
        )
    })
}

/// Return the intrinsic track sizing function supported by the auto-repeat
/// hypothetical pass. A definite or flexible breadth keeps using the shared
/// Grid/Taffy track sizing path, which already has a concrete repeat count.
fn grid_lanes_intrinsic_auto_repeat_track(
    track: css::GridTrackSize,
) -> Option<GridLanesIntrinsicAutoRepeatTrack> {
    match (track.min, track.max) {
        (css::GridMinTrackBreadth::Auto, css::GridMaxTrackBreadth::Auto) => {
            Some(GridLanesIntrinsicAutoRepeatTrack::Auto)
        }
        (css::GridMinTrackBreadth::MinContent, css::GridMaxTrackBreadth::MinContent) => {
            Some(GridLanesIntrinsicAutoRepeatTrack::MinContent)
        }
        (css::GridMinTrackBreadth::MaxContent, css::GridMaxTrackBreadth::MaxContent)
        | (css::GridMinTrackBreadth::MinContent, css::GridMaxTrackBreadth::MaxContent) => {
            Some(GridLanesIntrinsicAutoRepeatTrack::MaxContent)
        }
        (css::GridMinTrackBreadth::Auto, css::GridMaxTrackBreadth::FitContent(limit)) => {
            Some(GridLanesIntrinsicAutoRepeatTrack::FitContent(limit))
        }
        _ => None,
    }
}

fn grid_lanes_expanded_auto_repeat_line_names(
    tracks: &css::GridTrackList,
    repetitions: usize,
) -> Option<Vec<Vec<String>>> {
    let mut tracks = tracks.clone();
    let css::GridTrackList::Tracks { components, .. } = &mut tracks else {
        return None;
    };
    let repetitions = u16::try_from(repetitions).ok()?;
    for component in components {
        let css::GridTrackListComponent::Repeat(_, repeat) = component else {
            continue;
        };
        if matches!(
            repeat.count,
            css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
        ) {
            repeat.count = css::GridRepeatCount::Number(repetitions);
        }
    }
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        unreachable!();
    };
    explicit_grid_line_names(&components, &trailing_names)
}

fn grid_lanes_template_track_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    components.iter().try_fold(0_usize, |count, component| {
        let added = match component {
            css::GridTrackListComponent::Track(_, _) => 1,
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    return None;
                };
                let slots =
                    repeat
                        .tracks
                        .iter()
                        .try_fold(0_usize, |slots, component| match component {
                            css::GridTrackListComponent::Track(_, _) => slots.checked_add(1),
                            css::GridTrackListComponent::Repeat(_, _) => None,
                        })?;
                usize::from(repetitions).checked_mul(slots)?
            }
        };
        count.checked_add(added)
    })
}

fn grid_lanes_set_virtual_axis_placement(
    style: &mut GridUsedStyle,
    axis: GridLanesAxis,
    start: usize,
    span: usize,
) -> Option<()> {
    let start = i32::try_from(start.checked_add(1)?).ok()?;
    let end = start.checked_add(i32::try_from(span).ok()?)?;
    let start = css::GridPlacement::Line(css::GridLinePlacement::Number(
        std::num::NonZeroI32::new(start)?,
    ));
    let end = css::GridPlacement::Line(css::GridLinePlacement::Number(std::num::NonZeroI32::new(
        end,
    )?));
    match axis {
        GridLanesAxis::Columns => {
            style.grid_column_start = start;
            style.grid_column_end = end;
        }
        GridLanesAxis::Rows => {
            style.grid_row_start = start;
            style.grid_row_end = end;
        }
    }
    Some(())
}

fn grid_lanes_set_virtual_cross_axis_placement(
    style: &mut GridUsedStyle,
    axis: GridLanesAxis,
) -> Option<()> {
    let start = css::GridPlacement::Line(css::GridLinePlacement::Number(
        std::num::NonZeroI32::new(1)?,
    ));
    let end = css::GridPlacement::Line(css::GridLinePlacement::Number(std::num::NonZeroI32::new(
        2,
    )?));
    match axis {
        GridLanesAxis::Columns => {
            style.grid_row_start = start;
            style.grid_row_end = end;
        }
        GridLanesAxis::Rows => {
            style.grid_column_start = start;
            style.grid_column_end = end;
        }
    }
    Some(())
}

/// Return the physical number of tracks required by final explicit placement.
/// Positive line references beyond the resolved explicit repeat establish
/// ordinary implicit tracks; they must never be clamped into the repeat.
fn grid_lanes_required_track_count(
    axis: GridLanesAxis,
    children: &[GridChild<'_>],
    explicit_track_count: usize,
) -> usize {
    children
        .iter()
        .fold(explicit_track_count, |required, child| {
            let (start, end) = axis.placements(child);
            let span = grid_lanes_span(axis, child, usize::MAX);
            let start_line = grid_lanes_positive_line(start);
            let end_line = grid_lanes_positive_line(end);
            let item_required = match (start_line, end_line) {
                (Some(start), Some(end)) => start.max(end).saturating_sub(1),
                (Some(start), None) => start.saturating_sub(1).saturating_add(span),
                (None, Some(end)) => end.saturating_sub(1),
                // An automatically placed span expands the implicit grid
                // when the explicit repeat has fewer tracks than the span.
                // Its final position is chosen later by the Grid Lanes
                // placement algorithm, but it must fit before then.
                (None, None) => span,
            };
            required.max(item_required)
        })
}

fn grid_lanes_positive_line(placement: &css::GridPlacement) -> Option<usize> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    usize::try_from(line.index()?).ok().filter(|&line| line > 0)
}

fn grid_lanes_reverse_range(
    range: std::ops::Range<usize>,
    lane_count: usize,
) -> std::ops::Range<usize> {
    lane_count.saturating_sub(range.end)..lane_count.saturating_sub(range.start)
}

fn grid_lanes_content_alignment_offset(
    alignment: css::ContentAlignment,
    container_size: f32,
    content_size: f32,
) -> f32 {
    let free_space = container_size - content_size;
    let free_space = if alignment.safety == AlignmentSafety::Unsafe {
        free_space
    } else {
        free_space.max(0.0)
    };
    match alignment.keyword {
        css::ContentAlignmentKeyword::Center => free_space / 2.0,
        css::ContentAlignmentKeyword::End
        | css::ContentAlignmentKeyword::FlexEnd
        | css::ContentAlignmentKeyword::Right => free_space,
        _ => 0.0,
    }
}

/// Resolve the grid-axis size and physical offset of one Grid Lanes item.
///
/// The Grid Lanes algorithm first establishes an item's lane area, then uses
/// the normal Grid self-alignment rules inside that area.  The shared Grid
/// probe supplies the item's non-stretched border-box size; stretch is the
/// only case in which the lane area replaces it:
/// <https://drafts.csswg.org/css-grid-3/#alignment> and
/// <https://www.w3.org/TR/css-align-3/#self-alignment>.
fn grid_lanes_grid_axis_alignment(
    axis: GridLanesAxis,
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    area_size: f32,
    margin_start: f32,
    margin_end: f32,
    measured_border_size: f32,
) -> (f32, f32) {
    let alignment = match axis {
        GridLanesAxis::Columns => effective_grid_justify_self(child_style, container_style),
        GridLanesAxis::Rows => effective_grid_align_self(child_style, container_style),
    };
    let available_border_size = (area_size - margin_start - margin_end).max(0.0);
    let stretches = matches!(
        alignment.keyword,
        SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) && grid_lanes_grid_axis_size_is_auto(axis, child_style);
    let border_size = if stretches {
        available_border_size
    } else {
        // Self-alignment positions a definite item inside its grid area; it
        // does not constrain that item's used size to the area. Overflowing
        // a lane is therefore valid, with `safe` alignment affecting only
        // the chosen position:
        // <https://www.w3.org/TR/css-align-3/#overflow-values>.
        measured_border_size.max(0.0)
    };
    let free_space = area_size - margin_start - border_size - margin_end;
    let overflow_is_safe = alignment.safety != AlignmentSafety::Unsafe;
    let distributable_free_space = if overflow_is_safe {
        free_space.max(0.0)
    } else {
        free_space
    };
    let start_side = grid_lanes_alignment_uses_physical_start(
        axis,
        alignment.keyword,
        child_style,
        container_style,
    );
    let offset = match alignment.keyword {
        SelfAlignmentKeyword::Center => margin_start + distributable_free_space / 2.0,
        SelfAlignmentKeyword::End
        | SelfAlignmentKeyword::SelfEnd
        | SelfAlignmentKeyword::FlexEnd
        | SelfAlignmentKeyword::Right => {
            if start_side {
                margin_start + distributable_free_space
            } else {
                margin_start
            }
        }
        SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::SelfStart
        | SelfAlignmentKeyword::FlexStart
        | SelfAlignmentKeyword::Left => {
            if start_side {
                margin_start
            } else {
                margin_start + distributable_free_space
            }
        }
        _ => margin_start,
    };
    (border_size, offset)
}

/// Resolve a definite lane item's grid-axis size against its final grid area.
///
/// The ordinary two-dimensional Grid probe can only resolve an item's
/// percentage against its provisional grid geometry. Grid Lanes selects the
/// auto-repeat count and lane area afterwards, so percentage sizes must cross
/// this boundary as computed values and become used values here.
/// <https://drafts.csswg.org/css-grid-3/#placement> and
/// <https://www.w3.org/TR/css-sizing-3/#percentages>
fn grid_lanes_grid_axis_specified_border_size(
    axis: GridLanesAxis,
    child_style: &ComputedStyle,
    area_size: LayoutLength,
) -> Option<BorderBoxLength> {
    let (specified, padding_start, padding_end, border_start, border_end) = match axis {
        GridLanesAxis::Columns => (
            child_style.box_values.width.clone(),
            used_length_percentage(
                child_style.box_values.padding.left.clone(),
                PercentageBasis::definite(area_size),
            ),
            used_length_percentage(
                child_style.box_values.padding.right.clone(),
                PercentageBasis::definite(area_size),
            ),
            layout_pt(used_border_widths(child_style).left),
            layout_pt(used_border_widths(child_style).right),
        ),
        GridLanesAxis::Rows => (
            child_style.box_values.height.value().clone(),
            used_length_percentage(
                child_style.box_values.padding.top.clone(),
                PercentageBasis::definite(area_size),
            ),
            used_length_percentage(
                child_style.box_values.padding.bottom.clone(),
                PercentageBasis::definite(area_size),
            ),
            layout_pt(used_border_widths(child_style).top),
            layout_pt(used_border_widths(child_style).bottom),
        ),
    };
    let size = used_length_percentage_or_auto(specified, PercentageBasis::definite(area_size))?;
    let non_content = non_content_pt(
        padding_start.points() + padding_end.points() + border_start.points() + border_end.points(),
    );
    Some(match child_style.box_sizing {
        BoxSizing::ContentBox => content_box_to_border_box_length(
            crate::units::layout_to_content_box_length(size),
            non_content,
        ),
        BoxSizing::BorderBox => crate::units::layout_to_border_box_length(size),
    })
}

/// Whether an alignment keyword's start-like physical side is the positive
/// end of the lane coordinate. Grid Lanes presently has a physical lane
/// geometry, so this maps logical/self sides at the placement boundary.
fn grid_lanes_alignment_uses_physical_start(
    axis: GridLanesAxis,
    alignment: SelfAlignmentKeyword,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> bool {
    let side = match (axis, alignment) {
        (GridLanesAxis::Columns, SelfAlignmentKeyword::Left) => return true,
        (GridLanesAxis::Columns, SelfAlignmentKeyword::Right) => return false,
        (GridLanesAxis::Columns, SelfAlignmentKeyword::SelfStart) => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Horizontal)
        }
        (GridLanesAxis::Columns, SelfAlignmentKeyword::SelfEnd) => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Horizontal)
        }
        (GridLanesAxis::Rows, SelfAlignmentKeyword::SelfStart) => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Vertical)
        }
        (GridLanesAxis::Rows, SelfAlignmentKeyword::SelfEnd) => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Vertical)
        }
        (GridLanesAxis::Columns, _) => {
            return container_style.direction != Direction::Rtl;
        }
        (GridLanesAxis::Rows, _) => return true,
    };
    matches!(side, Some(PhysicalSide::Left | PhysicalSide::Top))
}

fn grid_lanes_grid_axis_size_is_auto(axis: GridLanesAxis, style: &ComputedStyle) -> bool {
    let size = match axis {
        GridLanesAxis::Columns => style.box_values.width.clone(),
        GridLanesAxis::Rows => style.box_values.height.value().clone(),
    };
    used_length_percentage_or_auto(size, PercentageBasis::<LayoutLength>::indefinite()).is_none()
}

#[derive(Clone, Copy)]
struct GridLanesMargins {
    top: LayoutLength,
    right: LayoutLength,
    bottom: LayoutLength,
    left: LayoutLength,
}

#[derive(Clone, Copy)]
struct GridLanesOccupiedInterval {
    start: f32,
    end: f32,
}

/// Find a compatible previously-skipped slot for dense Grid Lanes placement.
///
/// The Grid Lanes algorithm only permits backfilling into spans with the same
/// used grid-axis size as the item's normal placement.  The interval model
/// below represents the reserved outer-size-plus-gap regions for every track,
/// which lets dense placement reuse a hole without perturbing the running
/// positions that normal placement depends on.
fn grid_lanes_dense_backfill_position(
    occupied: &[Vec<GridLanesOccupiedInterval>],
    geometry: &GridLanesTrackGeometry,
    normal_range: &std::ops::Range<usize>,
    reserved_size: f32,
    normal_start: f32,
    tolerance: f32,
) -> Option<(std::ops::Range<usize>, f32)> {
    let normal_width = geometry.area_size(normal_range);
    let mut candidates = std::iter::once(0.0)
        .chain(
            occupied
                .iter()
                .flat_map(|lane| lane.iter().map(|interval| interval.end)),
        )
        .collect::<Vec<_>>();
    candidates.sort_by(f32::total_cmp);
    candidates.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    let mut best: Option<(std::ops::Range<usize>, f32)> = None;
    for start in candidates {
        if start + reserved_size > normal_start + 0.01 {
            continue;
        }
        for range_start in 0..=occupied.len().saturating_sub(normal_range.len()) {
            let range_end = range_start + normal_range.len();
            let width = geometry.area_size(&(range_start..range_end));
            if (width - normal_width).abs() > 0.01 {
                continue;
            }
            let fits = occupied[range_start..range_end].iter().all(|lane| {
                lane.iter().all(|interval| {
                    start + reserved_size <= interval.start + 0.01 || start >= interval.end - 0.01
                })
            });
            if fits
                && best
                    .as_ref()
                    .is_none_or(|(_, best_start)| start < *best_start - tolerance)
            {
                best = Some((range_start..range_end, start));
            }
        }
    }
    best.filter(|(_, start)| *start + 0.01 < normal_start)
}

fn grid_lanes_margins(
    style: &ComputedStyle,
    percentage_basis: GridLanesPercentageBasis,
) -> GridLanesMargins {
    let margin = style.box_values.margin.clone();
    GridLanesMargins {
        top: used_length_percentage_or_auto(margin.top, percentage_basis)
            .unwrap_or_else(|| layout_pt(0.0)),
        right: used_length_percentage_or_auto(margin.right, percentage_basis)
            .unwrap_or_else(|| layout_pt(0.0)),
        bottom: used_length_percentage_or_auto(margin.bottom, percentage_basis)
            .unwrap_or_else(|| layout_pt(0.0)),
        left: used_length_percentage_or_auto(margin.left, percentage_basis)
            .unwrap_or_else(|| layout_pt(0.0)),
    }
}

fn grid_lanes_content_inline_size(
    child: &GridChild<'_>,
    border_box_width: BorderBoxLength,
    inline_percentage_basis: GridLanesPercentageBasis,
) -> ContentBoxLength {
    border_box_to_content_box_length(
        border_box_width,
        grid_lanes_horizontal_non_content(&child.style, inline_percentage_basis),
    )
}

fn grid_lanes_horizontal_non_content(
    style: &ComputedStyle,
    percentage_basis: GridLanesPercentageBasis,
) -> NonContentLength {
    let padding = style.box_values.padding.clone();
    non_content_pt(
        used_length_percentage(padding.left, percentage_basis).points()
            + used_length_percentage(padding.right, percentage_basis).points()
            + used_border_widths(style).left
            + used_border_widths(style).right,
    )
}

fn grid_lanes_vertical_non_content(
    style: &ComputedStyle,
    percentage_basis: GridLanesPercentageBasis,
) -> NonContentLength {
    let padding = style.box_values.padding.clone();
    non_content_pt(
        used_length_percentage(padding.top, percentage_basis).points()
            + used_length_percentage(padding.bottom, percentage_basis).points()
            + used_border_widths(style).top
            + used_border_widths(style).bottom,
    )
}

fn grid_lanes_flow_tolerance(
    style: &ComputedStyle,
    grid_axis_size: GridLanesPercentageBasis,
) -> f32 {
    match &style.grid_lanes_flow_tolerance {
        css::GridLanesFlowTolerance::Normal => style.font_size.max(0.0),
        css::GridLanesFlowTolerance::LengthPercentage(value) => {
            used_length_percentage(value.clone(), grid_axis_size).points()
        }
        css::GridLanesFlowTolerance::Infinite => f32::INFINITY,
    }
}

/// Expand an explicit grid-axis list whose tracks are all fixed lengths.
///
/// This intentionally does not approximate intrinsic, flex, or auto-repeat
/// track sizing; those remain owned by the shared Grid track-sizing pass.
fn grid_lanes_definite_track_sizes(
    tracks: &css::GridTrackList,
    percentage_basis: GridLanesPercentageBasis,
) -> Option<Vec<f32>> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    let mut sizes = Vec::new();
    grid_lanes_collect_definite_track_sizes(components, percentage_basis, &mut sizes)?;
    if sizes.is_empty() {
        return None;
    }
    Some(sizes)
}

fn grid_lanes_collect_definite_track_sizes(
    components: &[css::GridTrackListComponent],
    percentage_basis: GridLanesPercentageBasis,
    sizes: &mut Vec<f32>,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, size) => {
                sizes.push(grid_lanes_definite_track_size(
                    size.clone(),
                    percentage_basis,
                )?);
            }
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(count) = repeat.count else {
                    return None;
                };
                for _ in 0..count {
                    grid_lanes_collect_definite_track_sizes(
                        &repeat.tracks,
                        percentage_basis,
                        sizes,
                    )?;
                }
            }
        }
    }
    Some(())
}

fn grid_lanes_definite_track_size(
    size: css::GridTrackSize,
    percentage_basis: GridLanesPercentageBasis,
) -> Option<f32> {
    let css::GridMinTrackBreadth::LengthPercentage(min) = size.min else {
        return None;
    };
    let css::GridMaxTrackBreadth::LengthPercentage(max) = size.max else {
        return None;
    };
    let min = used_length_percentage_with_basis(min, percentage_basis)?.points();
    let max = used_length_percentage_with_basis(max, percentage_basis)?.points();
    ((min - max).abs() < 0.01).then_some(min.max(0.0))
}

/// Build the grid-axis geometry for simple all-auto row lanes.
///
/// Auto-placed Grid Lanes items contribute to every eligible auto track. For
/// an all-auto row template, each row therefore takes the largest outer block
/// contribution among the items rather than inheriting rows introduced by a
/// two-dimensional auto-placement probe.
/// <https://drafts.csswg.org/css-grid-3/#grid-axis-track-sizing>
fn grid_lanes_auto_row_offsets(
    tracks: &css::GridTrackList,
    children: &[GridChild<'_>],
    items: &[GridItemLayout],
    inline_percentage_basis: GridLanesPercentageBasis,
    gap: css::ComputedGap,
) -> Option<GridLanesTrackGeometry> {
    let row_count = grid_lanes_all_auto_track_count(tracks)?;
    if row_count == 0 || children.len() != items.len() {
        return None;
    }
    if grid_lanes_has_out_of_range_explicit_placement(children, GridLanesAxis::Rows, row_count) {
        return None;
    }
    let mut track_sizes = vec![0.0_f32; row_count];
    let mut auto_contribution = 0.0_f32;
    for (child, item) in children.iter().zip(items) {
        let margins = grid_lanes_margins(&child.style, inline_percentage_basis);
        let contribution = item.height() + margins.top.points() + margins.bottom.points();
        let span = grid_lanes_span(GridLanesAxis::Rows, child, row_count);
        if let Some(range) =
            grid_lanes_fixed_range(GridLanesAxis::Rows, child, row_count, span, None)
            && range.len() == 1
        {
            track_sizes[range.start] = track_sizes[range.start].max(contribution);
        } else if span == 1 {
            // A Level 3 auto-placed item contributes to every row it could
            // occupy. Spanning contributions need the normal Grid spanning
            // distribution algorithm and remain outside this simple path.
            auto_contribution = auto_contribution.max(contribution);
        }
    }
    for size in &mut track_sizes {
        *size = size.max(auto_contribution);
    }
    GridLanesTrackGeometry::from_track_sizes(
        &track_sizes,
        used_grid_lanes_gap(gap, inline_percentage_basis),
    )
}

fn grid_lanes_all_auto_track_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    let mut count = 0_usize;
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, size)
                if matches!(size.min, css::GridMinTrackBreadth::Auto)
                    && matches!(size.max, css::GridMaxTrackBreadth::Auto) =>
            {
                count += 1;
            }
            css::GridTrackListComponent::Repeat(_, repeat)
                if matches!(repeat.count, css::GridRepeatCount::Number(_)) =>
            {
                let nested = grid_lanes_all_auto_track_count(&css::GridTrackList::Tracks {
                    components: repeat.tracks.clone(),
                    trailing_names: repeat.trailing_names.clone(),
                })?;
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    unreachable!();
                };
                count = count.checked_add(nested.checked_mul(usize::from(repetitions))?)?;
            }
            _ => return None,
        }
    }
    Some(count)
}

/// An explicit line beyond the authored template creates an implicit track.
/// The all-auto shortcut deliberately handles only the self-contained
/// explicit grid; implicit tracks require the full Level 3 auto-track sizing
/// algorithm instead of clamping a placement back into the authored rows.
fn grid_lanes_has_out_of_range_explicit_placement(
    children: &[GridChild<'_>],
    axis: GridLanesAxis,
    track_count: usize,
) -> bool {
    let last_explicit_line = i32::try_from(track_count + 1).unwrap_or(i32::MAX);
    children.iter().any(|child| {
        let (start, end) = axis.placements(child);
        [start, end].into_iter().any(|placement| {
            matches!(placement, css::GridPlacement::Line(line) if line.name().is_none()
                && line.index().is_some_and(|index| index > last_explicit_line))
        })
    })
}

fn grid_lanes_span(axis: GridLanesAxis, child: &GridChild<'_>, lane_count: usize) -> usize {
    let (start, end) = axis.placements(child);
    match (start, end) {
        (css::GridPlacement::Span(span), _) | (_, css::GridPlacement::Span(span)) => {
            usize::from(span.count().unwrap_or(1))
        }
        _ => 1,
    }
    .clamp(1, lane_count)
}

/// The parent-grid area corresponding to a resolved Grid Lanes range. The
/// stacking axis is the container content box; only the grid axis is a grid
/// area. This is the same topology used for final replay of a subgrid item.
fn grid_lanes_item_area(axis: GridLanesAxis, range: &std::ops::Range<usize>) -> GridItemArea {
    let start = u16::try_from(range.start.saturating_add(1)).unwrap_or(u16::MAX);
    let end = u16::try_from(range.end.saturating_add(1)).unwrap_or(u16::MAX);
    match axis {
        GridLanesAxis::Columns => GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: start,
            column_end: end,
        },
        GridLanesAxis::Rows => GridItemArea {
            row_start: start,
            row_end: end,
            column_start: 1,
            column_end: 2,
        },
    }
}

fn grid_lanes_fixed_range(
    axis: GridLanesAxis,
    child: &GridChild<'_>,
    lane_count: usize,
    span: usize,
    line_names: Option<&[Vec<String>]>,
) -> Option<std::ops::Range<usize>> {
    let (start, end) = axis.placements(child);
    let start_line = grid_lanes_line(start, line_names, lane_count);
    let end_line = grid_lanes_line(end, line_names, lane_count);
    let start = match (start_line, end_line) {
        (Some(start), Some(end)) if end > start => start,
        (Some(start), _) => start,
        (_, Some(end)) => end.checked_sub(span)?,
        _ => return None,
    };
    let start = start.min(lane_count.saturating_sub(span));
    Some(start..start + span)
}

fn grid_lanes_shortest_range(
    lane_ends: &[f32],
    active_tracks: &[bool],
    span: usize,
    cursor: usize,
    tolerance: f32,
) -> std::ops::Range<usize> {
    debug_assert_eq!(lane_ends.len(), active_tracks.len());
    let mut best_end = f32::INFINITY;
    for start in 0..=lane_ends.len().saturating_sub(span) {
        if !active_tracks[start..start + span]
            .iter()
            .all(|&active| active)
        {
            continue;
        }
        let end = lane_ends[start..start + span]
            .iter()
            .cloned()
            .fold(0.0_f32, f32::max);
        best_end = best_end.min(end);
    }
    let mut first = None;
    for start in 0..=lane_ends.len().saturating_sub(span) {
        if !active_tracks[start..start + span]
            .iter()
            .all(|&active| active)
        {
            continue;
        }
        let end = lane_ends[start..start + span]
            .iter()
            .cloned()
            .fold(0.0_f32, f32::max);
        if end <= best_end + tolerance {
            first.get_or_insert(start);
            if start >= cursor {
                return start..start + span;
            }
        }
    }
    let start =
        first.unwrap_or_else(|| active_tracks.iter().position(|&active| active).unwrap_or(0));
    start..start + span
}

fn grid_lanes_line(
    placement: &css::GridPlacement,
    line_names: Option<&[Vec<String>]>,
    lane_count: usize,
) -> Option<usize> {
    if let Some(line_names) = line_names {
        return grid_line_index(placement, line_names)
            .and_then(|line| usize::try_from(line - 1).ok())
            .filter(|line| *line <= lane_count);
    }
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    let index = line.index()?;
    if index > 0 {
        usize::try_from(index - 1)
            .ok()
            .filter(|index| *index <= lane_count)
    } else {
        let line_count = i32::try_from(lane_count + 1).ok()?;
        usize::try_from(line_count + index)
            .ok()
            .filter(|index| *index <= lane_count)
    }
}

fn grid_lanes_explicit_line_names(tracks: &css::GridTrackList) -> Option<Vec<Vec<String>>> {
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        return None;
    };
    explicit_grid_line_names(components, trailing_names)
}

fn used_grid_lanes_gap(gap: css::ComputedGap, basis: GridLanesPercentageBasis) -> f32 {
    match gap {
        css::ComputedGap::Normal => 0.0,
        css::ComputedGap::LengthPercentage(value) => used_length_percentage(value, basis).points(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_auto_fit_track_preserves_one_gutter_between_active_neighbors() {
        let geometry = GridLanesTrackGeometry::from_track_sizes_with_active(
            &[37.5, 37.5, 0.0, 37.5],
            7.5,
            &[true, true, false, true],
        )
        .expect("non-empty track list has geometry");

        assert_eq!(geometry.starts, vec![0.0, 45.0, 90.0, 90.0]);
        assert_eq!(geometry.ends, vec![37.5, 82.5, 90.0, 127.5]);
    }
}
