use super::model::{GridItemArea, GridItemLayout};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridBaselineSet {
    First,
    Last,
}

/// The result of resolving one Grid baseline-alignment request before
/// measured baseline geometry is applied.
///
/// CSS Grid excludes a baseline-aligned item when the item's size in the
/// relevant axis depends on an intrinsically sized track in the same axis.
/// Recording that decision separately from the eventual baseline coordinate
/// keeps Grid's track sizing, self-alignment, and exported baselines from
/// disagreeing after tracks have been stretched:
/// <https://www.w3.org/TR/css-grid-1/#row-align> and
/// <https://www.w3.org/TR/css-grid-1/#algo-content-alignment>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridBaselineParticipation {
    NotRequested,
    Shares(GridBaselineSet),
    Fallback {
        baseline_set: GridBaselineSet,
        reason: GridBaselineFallbackReason,
    },
}

impl GridBaselineParticipation {
    fn shares(self, baseline_set: GridBaselineSet) -> bool {
        self == Self::Shares(baseline_set)
    }

    fn requests(self, baseline_set: GridBaselineSet) -> bool {
        matches!(
            self,
            Self::Shares(requested)
                | Self::Fallback {
                    baseline_set: requested,
                    ..
                } if requested == baseline_set
        )
    }

    fn fallback_set(self) -> Option<GridBaselineSet> {
        match self {
            Self::Fallback { baseline_set, .. } => Some(baseline_set),
            Self::NotRequested | Self::Shares(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridBaselineFallbackReason {
    CyclicTrackSizing,
    IncompatibleWritingMode,
}

/// All baseline requests associated with one item, retained in the grid's
/// logical axes even when the physical Taffy adapter swaps those axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridBaselineResolution {
    row_self: GridBaselineParticipation,
    column_self: GridBaselineParticipation,
    row_content: GridBaselineParticipation,
    column_content: GridBaselineParticipation,
}

/// A virtual margin used only while Grid sizes intrinsic tracks for a
/// baseline-sharing group.  CSS Grid calls this a baseline "shim"; keeping it
/// distinct from used margins prevents the sizing contribution from leaking
/// into item replay.
/// <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GridBaselineSizingShim {
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
}

/// Baseline decisions shared by the Grid sizing and final-alignment phases.
///
/// The vector is indexed by the real Grid item index, so sizing-only subgrid
/// contribution leaves cannot accidentally acquire an item's baseline shim.
#[derive(Debug, Clone, Default)]
pub(super) struct GridBaselinePlan {
    shims: Vec<GridBaselineSizingShim>,
}

impl GridBaselinePlan {
    pub(super) fn shim(&self, index: usize) -> Option<GridBaselineSizingShim> {
        self.shims.get(index).copied()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.shims.iter().all(|shim| {
            shim.top == 0.0 && shim.bottom == 0.0 && shim.left == 0.0 && shim.right == 0.0
        })
    }
}

/// Whether the placed Grid topology contains a row baseline-sharing group
/// that can produce a track-sizing shim.
///
/// The current Taffy adapter only represents row shims for grids whose
/// logical axes match the physical axes. A sharing group needs at least two
/// same-set participants on its first or last baseline edge before measuring
/// their baselines can affect intrinsic track sizing.
pub(super) fn grid_baseline_sizing_may_need_shims(
    container_style: &ComputedStyle,
    resolutions: &[GridBaselineResolution],
    items: &[GridItemLayout],
) -> bool {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return false;
    }

    [GridBaselineSet::First, GridBaselineSet::Last]
        .into_iter()
        .any(|baseline_set| {
            items.iter().enumerate().any(|(index, item)| {
                let Some(area) = item.area else {
                    return false;
                };
                if !resolutions[index]
                    .self_alignment(GridAxis::Row)
                    .shares(baseline_set)
                {
                    return false;
                }
                let (row_start, row_end) = grid_baseline_group_key(area, baseline_set);
                items
                    .iter()
                    .enumerate()
                    .skip(index + 1)
                    .any(|(other_index, other)| {
                        grid_item_in_baseline_group(other, row_start, row_end, baseline_set)
                            && resolutions[other_index]
                                .self_alignment(GridAxis::Row)
                                .shares(baseline_set)
                    })
            })
        })
}

/// Construct the Grid track-sizing shims for baseline-aligned row groups.
///
/// Taffy's Grid implementation uses physical rows/columns, while the
/// measured text baselines currently available to this adapter are physical
/// top-edge offsets.  Therefore this phase contributes row shims for a
/// horizontal grid; vertical/column baseline geometry remains represented by
/// the participation resolution and falls back safely until its physical
/// baseline table is measured by the inline adapter.
pub(super) fn grid_baseline_plan(
    container_style: &ComputedStyle,
    _children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    resolutions: &[GridBaselineResolution],
    items: &[GridItemLayout],
) -> GridBaselinePlan {
    let mut plan = GridBaselinePlan {
        shims: vec![GridBaselineSizingShim::default(); items.len()],
    };
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return plan;
    }
    for baseline_set in [GridBaselineSet::First, GridBaselineSet::Last] {
        let mut groups = Vec::<(u16, u16)>::new();
        for item in items {
            let Some(area) = item.area else {
                continue;
            };
            let group = grid_baseline_group_key(area, baseline_set);
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
        for (row_start, row_end) in groups {
            let participants = items
                .iter()
                .enumerate()
                .filter(|(index, item)| {
                    grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
                        && resolutions[*index]
                            .self_alignment(GridAxis::Row)
                            .shares(baseline_set)
                })
                .collect::<Vec<_>>();
            if participants.len() < 2 {
                continue;
            }
            let greatest_distance = participants
                .iter()
                .map(|(index, item)| {
                    let baseline =
                        grid_item_border_box_baseline(&estimates[*index], item, baseline_set);
                    match baseline_set {
                        GridBaselineSet::First => baseline,
                        GridBaselineSet::Last => (item.height() - baseline).max(0.0),
                    }
                })
                .fold(0.0_f32, f32::max);
            for (index, item) in participants {
                let baseline = grid_item_border_box_baseline(&estimates[index], item, baseline_set);
                let distance = match baseline_set {
                    GridBaselineSet::First => baseline,
                    GridBaselineSet::Last => (item.height() - baseline).max(0.0),
                };
                let shim = (greatest_distance - distance).max(0.0);
                match baseline_set {
                    GridBaselineSet::First => plan.shims[index].top = shim,
                    GridBaselineSet::Last => plan.shims[index].bottom = shim,
                }
            }
        }
    }
    plan
}

/// Convert authored margins to the sizing-only margin model with an optional
/// Grid baseline shim.  Grid resolves cyclic margin percentages before this
/// boundary, so any non-auto edge can safely become one fixed Taffy length.
pub(super) fn grid_taffy_margin_with_baseline_shim<Source: Copy>(
    style: &ComputedStyle,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
    shim: Option<GridBaselineSizingShim>,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let mut margin = taffy_bridge::margin(
        style,
        percentage_basis,
        taffy_bridge::TaffyCyclicPercentage::ResolveToLengthComponent,
    );
    let Some(shim) = shim else {
        return margin;
    };
    fn add(
        value: taffy_layout::LengthPercentageAuto,
        amount: f32,
    ) -> taffy_layout::LengthPercentageAuto {
        if amount == 0.0 || value.is_auto() {
            return value;
        }
        taffy_layout::LengthPercentageAuto::length(
            value.resolve_to_option(0.0, |_, _| 0.0).unwrap_or(0.0) + amount,
        )
    }
    margin.top = add(margin.top, shim.top);
    margin.bottom = add(margin.bottom, shim.bottom);
    margin.left = add(margin.left, shim.left);
    margin.right = add(margin.right, shim.right);
    margin
}

impl GridBaselineResolution {
    fn self_alignment(self, axis: GridAxis) -> GridBaselineParticipation {
        match axis {
            GridAxis::Row => self.row_self,
            GridAxis::Column => self.column_self,
        }
    }

    /// Return the fallback required for the grid item's own content alignment.
    ///
    /// The cyclic exclusion is a used-value decision, so replay must receive
    /// the fallback rather than the authored baseline keyword:
    /// <https://www.w3.org/TR/css-grid-1/#row-align>.
    pub(super) fn content_alignment_fallback(self, axis: GridAxis) -> Option<GridBaselineSet> {
        match axis {
            GridAxis::Row => self.row_content,
            GridAxis::Column => self.column_content,
        }
        .fallback_set()
    }
}

#[derive(Clone, Copy)]
enum GridBaselineAlignmentSource {
    SelfAlignment,
    ContentAlignment,
}

pub(super) fn resolve_grid_baseline_participation(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    items: &[GridItemLayout],
    available_space: GridPhysicalAvailableSpace,
) -> Vec<GridBaselineResolution> {
    children
        .iter()
        .zip(items)
        .map(|(child, item)| GridBaselineResolution {
            row_self: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_space,
            ),
            column_self: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Column,
                GridBaselineAlignmentSource::SelfAlignment,
                available_space,
            ),
            row_content: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Row,
                GridBaselineAlignmentSource::ContentAlignment,
                available_space,
            ),
            column_content: resolve_grid_item_baseline_participation(
                container_style,
                &child.style,
                item.area,
                GridAxis::Column,
                GridBaselineAlignmentSource::ContentAlignment,
                available_space,
            ),
        })
        .collect()
}

fn resolve_grid_item_baseline_participation(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    area: Option<GridItemArea>,
    axis: GridAxis,
    source: GridBaselineAlignmentSource,
    available_space: GridPhysicalAvailableSpace,
) -> GridBaselineParticipation {
    let Some(baseline_set) =
        grid_requested_baseline_set(container_style, child_style, axis, source)
    else {
        return GridBaselineParticipation::NotRequested;
    };
    let Some(area) = area else {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::CyclicTrackSizing,
        };
    };
    if grid_item_axis_depends_on_intrinsic_track(
        container_style,
        child_style,
        area,
        axis,
        available_space,
    ) {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::CyclicTrackSizing,
        };
    }
    if child_style.writing_mode != WritingMode::HorizontalTb {
        return GridBaselineParticipation::Fallback {
            baseline_set,
            reason: GridBaselineFallbackReason::IncompatibleWritingMode,
        };
    }
    GridBaselineParticipation::Shares(baseline_set)
}

fn grid_requested_baseline_set(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    axis: GridAxis,
    source: GridBaselineAlignmentSource,
) -> Option<GridBaselineSet> {
    match source {
        GridBaselineAlignmentSource::SelfAlignment => match axis {
            GridAxis::Row => grid_self_alignment_baseline_set(
                effective_grid_align_self(child_style, container_style).keyword,
            ),
            GridAxis::Column => grid_self_alignment_baseline_set(
                effective_grid_justify_self(child_style, container_style).keyword,
            ),
        },
        GridBaselineAlignmentSource::ContentAlignment => match axis {
            GridAxis::Row => grid_content_alignment_baseline_set(child_style.align_content.keyword),
            GridAxis::Column => {
                grid_content_alignment_baseline_set(child_style.justify_content.keyword)
            }
        },
    }
}

fn grid_self_alignment_baseline_set(keyword: SelfAlignmentKeyword) -> Option<GridBaselineSet> {
    match keyword {
        SelfAlignmentKeyword::Baseline => Some(GridBaselineSet::First),
        SelfAlignmentKeyword::LastBaseline => Some(GridBaselineSet::Last),
        _ => None,
    }
}

fn grid_content_alignment_baseline_set(
    keyword: ContentAlignmentKeyword,
) -> Option<GridBaselineSet> {
    match keyword {
        ContentAlignmentKeyword::Baseline => Some(GridBaselineSet::First),
        ContentAlignmentKeyword::LastBaseline => Some(GridBaselineSet::Last),
        _ => None,
    }
}

/// Returns whether the item must know the resolved size of an intrinsic track
/// before it can determine its own size in the requested logical axis.
///
/// This is deliberately based on the pre-alignment track functions and the
/// original grid area, never on the final stretched track size. CSS Grid says
/// the presence of this cycle is invariant over the course of layout:
/// <https://www.w3.org/TR/css-grid-1/#row-align>.
fn grid_item_axis_depends_on_intrinsic_track(
    container_style: &ComputedStyle,
    child_style: &ComputedStyle,
    area: GridItemArea,
    axis: GridAxis,
    available_space: GridPhysicalAvailableSpace,
) -> bool {
    let physical_axis = grid_physical_axis(container_style, axis);
    grid_item_size_depends_on_track(child_style, physical_axis)
        && grid_area_has_intrinsic_track(
            container_style,
            area,
            axis,
            grid_physical_axis_is_definite(physical_axis, available_space),
        )
}

fn grid_physical_axis(container_style: &ComputedStyle, axis: GridAxis) -> PhysicalAxis {
    let logical_axis = match axis {
        GridAxis::Column => LogicalAxis::Inline,
        GridAxis::Row => LogicalAxis::Block,
    };
    WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .physical_axis(logical_axis)
}

fn grid_physical_axis_is_definite(
    axis: PhysicalAxis,
    available_space: GridPhysicalAvailableSpace,
) -> bool {
    match axis {
        PhysicalAxis::Horizontal => available_space.width_basis.points().is_some(),
        PhysicalAxis::Vertical => available_space.height_basis.points().is_some(),
    }
}

fn grid_item_size_depends_on_track(style: &ComputedStyle, axis: PhysicalAxis) -> bool {
    let values = match axis {
        PhysicalAxis::Horizontal => [
            &style.box_values.width,
            &style.box_values.min_width,
            &style.box_values.max_width,
        ],
        PhysicalAxis::Vertical => [
            style.box_values.height.value(),
            &style.box_values.min_height,
            &style.box_values.max_height,
        ],
    };
    values.into_iter().any(grid_box_size_value_depends_on_track)
}

fn grid_box_size_value_depends_on_track(value: &css::ComputedLengthPercentageOrAuto) -> bool {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.contains_percentage(),
        css::ComputedLengthPercentageOrAuto::FitContent(Some(value)) => value.contains_percentage(),
        // `calc-size()` can retain either a percentage or an intrinsic basis
        // until Grid has selected the item's track-sized box. Treating it as
        // track-dependent is conservative and prevents a cycle from being
        // reintroduced as this syntax gains more used-value support.
        css::ComputedLengthPercentageOrAuto::CalcSize(_) => true,
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(None)
        | css::ComputedLengthPercentageOrAuto::Stretch => false,
    }
}

fn grid_area_has_intrinsic_track(
    style: &ComputedStyle,
    area: GridItemArea,
    axis: GridAxis,
    axis_is_definite: bool,
) -> bool {
    let (start, end) = match axis {
        GridAxis::Column => (area.column_start, area.column_end),
        GridAxis::Row => (area.row_start, area.row_end),
    };
    (usize::from(start).saturating_sub(1)..usize::from(end).saturating_sub(1)).any(|index| {
        grid_track_at(style, axis, index)
            .is_some_and(|track| grid_track_is_intrinsic(track, axis_is_definite))
    })
}

fn grid_track_at(
    style: &ComputedStyle,
    axis: GridAxis,
    index: usize,
) -> Option<&css::GridTrackSize> {
    let (tracks, auto_tracks) = match axis {
        GridAxis::Column => (&style.grid_template_columns, &style.grid_auto_columns),
        GridAxis::Row => (&style.grid_template_rows, &style.grid_auto_rows),
    };
    let explicit_count = grid_track_list_count(tracks)?;
    if index < explicit_count {
        return grid_explicit_track_at(tracks, index);
    }
    auto_tracks.get((index - explicit_count) % auto_tracks.len().max(1))
}

fn grid_track_list_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return Some(0);
    };
    grid_track_component_count(components)
}

fn grid_track_component_count(components: &[css::GridTrackListComponent]) -> Option<usize> {
    components
        .iter()
        .try_fold(0_usize, |count, component| match component {
            css::GridTrackListComponent::Track(_, _) => count.checked_add(1),
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    return None;
                };
                count.checked_add(
                    grid_track_component_count(&repeat.tracks)?
                        .checked_mul(usize::from(repetitions))?,
                )
            }
        })
}

fn grid_explicit_track_at(
    tracks: &css::GridTrackList,
    index: usize,
) -> Option<&css::GridTrackSize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    grid_track_component_at(components, index)
}

fn grid_track_component_at(
    components: &[css::GridTrackListComponent],
    mut index: usize,
) -> Option<&css::GridTrackSize> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, track) => {
                if index == 0 {
                    return Some(track);
                }
                index -= 1;
            }
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repetitions) = repeat.count else {
                    return None;
                };
                let repeated_count = grid_track_component_count(&repeat.tracks)?;
                let total_count = repeated_count.checked_mul(usize::from(repetitions))?;
                if index < total_count {
                    return grid_track_component_at(&repeat.tracks, index % repeated_count);
                }
                index -= total_count;
            }
        }
    }
    None
}

fn grid_track_is_intrinsic(track: &css::GridTrackSize, axis_is_definite: bool) -> bool {
    matches!(
        track.min,
        css::GridMinTrackBreadth::Auto
            | css::GridMinTrackBreadth::MinContent
            | css::GridMinTrackBreadth::MaxContent
    ) || matches!(
        track.max,
        css::GridMaxTrackBreadth::Auto
            | css::GridMaxTrackBreadth::MinContent
            | css::GridMaxTrackBreadth::MaxContent
            | css::GridMaxTrackBreadth::FitContent(_)
            | css::GridMaxTrackBreadth::Flex(_) if !axis_is_definite
    ) || matches!(
        (&track.min, &track.max),
        (
            css::GridMinTrackBreadth::LengthPercentage(min),
            css::GridMaxTrackBreadth::LengthPercentage(max),
        ) if !axis_is_definite && (min.contains_percentage() || max.contains_percentage())
    )
}

struct GridBaselineAlignmentContext<'a, 'box_tree> {
    children: &'a [GridChild<'box_tree>],
    estimates: &'a [GridItemEstimate],
    resolutions: &'a [GridBaselineResolution],
    row_line_offsets: &'a [f32],
}
/// Apply Quire-measured baseline self-alignment for simple grid rows.
///
/// Taffy's grid measure callback does not receive text baseline metadata, so
/// same-row baseline self-alignment would otherwise synthesize from item boxes.
/// This post-layout pass adjusts horizontal writing-mode participants sharing
/// the relevant baseline row edge to share their measured first or last
/// baselines:
/// <https://www.w3.org/TR/css-grid-1/#grid-baselines> and
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self>.
pub(super) fn apply_grid_baseline_alignment(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    resolutions: &[GridBaselineResolution],
    row_line_offsets: &[f32],
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    let context = GridBaselineAlignmentContext {
        children,
        estimates,
        resolutions,
        row_line_offsets,
    };
    for baseline_set in [GridBaselineSet::First, GridBaselineSet::Last] {
        let mut row_groups = Vec::<(u16, u16)>::new();
        for item in items.iter() {
            let Some(area) = item.area else {
                continue;
            };
            let key = grid_baseline_group_key(area, baseline_set);
            if !row_groups.contains(&key) {
                row_groups.push(key);
            }
        }
        for (row_start, row_end) in row_groups {
            align_grid_row_baseline_group(&context, items, row_start, row_end, baseline_set);
        }
    }
}

fn grid_baseline_group_key(area: GridItemArea, baseline_set: GridBaselineSet) -> (u16, u16) {
    match baseline_set {
        GridBaselineSet::First => (area.row_start, 0),
        GridBaselineSet::Last => (0, area.row_end),
    }
}

fn align_grid_row_baseline_group(
    context: &GridBaselineAlignmentContext<'_, '_>,
    items: &mut [GridItemLayout],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) {
    let mut participant_count = 0_usize;
    let mut largest_distance = None::<f32>;
    for (index, item) in items.iter().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
        {
            continue;
        }
        let baseline = grid_item_border_box_baseline(&context.estimates[index], item, baseline_set);
        participant_count += 1;
        let distance = match baseline_set {
            GridBaselineSet::First => baseline,
            GridBaselineSet::Last => (item.height() - baseline).max(0.0),
        };
        largest_distance = Some(
            largest_distance
                .map(|target| target.max(distance))
                .unwrap_or(distance),
        );
    }
    if participant_count < 2 {
        // CSS Box Alignment falls a baseline self-alignment request back to
        // safe self-start/self-end when no compatible sharing group remains.
        // That includes items excluded by Grid's intrinsic-track cycle rule.
        // Taffy cannot perform this fallback from Quire's measured baseline
        // metadata, so apply it to every requesting border box here.
        // <https://www.w3.org/TR/css-align-3/#baseline-align-self>
        for (index, item) in items.iter_mut().enumerate() {
            if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
                || !context.resolutions[index]
                    .self_alignment(GridAxis::Row)
                    .requests(baseline_set)
            {
                continue;
            }
            apply_grid_baseline_self_alignment_fallback(
                item,
                &context.children[index].style,
                context.row_line_offsets,
                row_start,
                row_end,
                baseline_set,
            );
        }
        return;
    }
    let Some(largest_distance) = largest_distance else {
        return;
    };
    let row_edge = grid_row_baseline_edge(
        items,
        context.row_line_offsets,
        row_start,
        row_end,
        baseline_set,
    );
    let target_baseline = match baseline_set {
        GridBaselineSet::First => row_edge + largest_distance,
        GridBaselineSet::Last => row_edge - largest_distance,
    };
    for (index, item) in items.iter_mut().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
        {
            continue;
        }
        let baseline = grid_item_border_box_baseline(&context.estimates[index], item, baseline_set);
        item.set_axis_geometry(GridAxis::Row, target_baseline - baseline, item.height());
    }
    // Cyclic and otherwise incompatible baseline requests remain fallback
    // aligned even when their row still has a compatible sharing group.
    for (index, item) in items.iter_mut().enumerate() {
        if !grid_item_in_baseline_group(item, row_start, row_end, baseline_set)
            || !context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .requests(baseline_set)
            || context.resolutions[index]
                .self_alignment(GridAxis::Row)
                .shares(baseline_set)
        {
            continue;
        }
        apply_grid_baseline_self_alignment_fallback(
            item,
            &context.children[index].style,
            context.row_line_offsets,
            row_start,
            row_end,
            baseline_set,
        );
    }
}

fn apply_grid_baseline_self_alignment_fallback(
    item: &mut GridItemLayout,
    child_style: &ComputedStyle,
    row_line_offsets: &[f32],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) {
    let area_start = row_line_offsets
        .get(usize::from(row_start).saturating_sub(1))
        .copied()
        .unwrap_or(item.y());
    let area_end = row_line_offsets
        .get(usize::from(row_end).saturating_sub(1))
        .copied()
        .unwrap_or(item.y() + item.height());
    let y = match baseline_set {
        GridBaselineSet::First => {
            area_start
                + item
                    .used_box_metrics()
                    .map(|metrics| metrics.margin.top.points())
                    .unwrap_or(child_style.margin.top)
        }
        GridBaselineSet::Last => {
            area_end
                - item
                    .used_box_metrics()
                    .map(|metrics| metrics.margin.bottom.points())
                    .unwrap_or(child_style.margin.bottom)
                - item.height().max(0.0)
        }
    };
    item.set_axis_geometry(GridAxis::Row, y, item.height());
}

fn grid_row_baseline_edge(
    items: &[GridItemLayout],
    row_line_offsets: &[f32],
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) -> f32 {
    match baseline_set {
        GridBaselineSet::First => row_line_offsets
            .get(usize::from(row_start).saturating_sub(1))
            .cloned()
            .unwrap_or_else(|| {
                items
                    .iter()
                    .filter(|item| {
                        grid_item_in_baseline_group(
                            item,
                            row_start,
                            row_end,
                            GridBaselineSet::First,
                        )
                    })
                    .map(GridItemLayout::y)
                    .reduce(f32::min)
                    .unwrap_or(0.0)
            }),
        GridBaselineSet::Last => row_line_offsets
            .get(usize::from(row_end).saturating_sub(1))
            .cloned()
            .unwrap_or_else(|| {
                items
                    .iter()
                    .filter(|item| {
                        grid_item_in_baseline_group(item, row_start, row_end, GridBaselineSet::Last)
                    })
                    .map(|item| item.y() + item.height())
                    .reduce(f32::max)
                    .unwrap_or(0.0)
            }),
    }
}

fn grid_item_in_baseline_group(
    item: &GridItemLayout,
    row_start: u16,
    row_end: u16,
    baseline_set: GridBaselineSet,
) -> bool {
    item.area.is_some_and(|area| match baseline_set {
        GridBaselineSet::First => area.row_start == row_start,
        GridBaselineSet::Last => area.row_end == row_end,
    })
}

/// Return a same-page grid container baseline in content-box coordinates.
///
/// CSS Grid exports first and last baselines from the first or last row that
/// contains grid items. If that row has a compatible baseline-sharing group,
/// the container baseline comes from the shared alignment baseline; otherwise
/// it comes from the first or last item in row-major grid order, synthesizing a
/// missing item baseline from the item border box:
/// <https://www.w3.org/TR/css-grid-1/#grid-baselines> and
/// <https://www.w3.org/TR/css-align-3/#synthesize-baseline>.
pub(super) fn grid_container_baseline(
    container_style: &ComputedStyle,
    estimates: &[GridItemEstimate],
    resolutions: &[GridBaselineResolution],
    items: &[GridItemLayout],
    baseline_set: GridBaselineSet,
) -> Option<f32> {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return None;
    }
    let row_index = grid_container_baseline_row(items, baseline_set)?;
    // CSS Grid's exported baseline has an ordered fallback chain.  In
    // particular, a last-baseline sharing group is still preferable to an
    // arbitrary item baseline when no first-baseline group exists in the
    // relevant edge track.
    // <https://drafts.csswg.org/css-grid-2/#grid-baselines>
    for requested_set in [baseline_set, grid_opposite_baseline_set(baseline_set)] {
        if let Some((index, item)) = items.iter().enumerate().find(|(index, item)| {
            grid_item_is_container_baseline_eligible(item, row_index, requested_set)
                && resolutions[*index]
                    .self_alignment(GridAxis::Row)
                    .shares(requested_set)
        }) {
            return Some(
                item.y() + grid_item_border_box_baseline(&estimates[index], item, requested_set),
            );
        }
    }
    if let Some((index, item)) = items.iter().enumerate().find(|(index, item)| {
        grid_item_is_container_baseline_eligible(item, row_index, baseline_set)
            && grid_item_has_baseline(&estimates[*index], baseline_set)
    }) {
        return Some(
            item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set),
        );
    }
    // The final fallback is the first Grid item in grid order, not merely an
    // item intersecting the edge track. Its absent baseline is synthesized
    // from its border box by `grid_item_border_box_baseline`.
    items.iter().enumerate().find_map(|(index, item)| {
        item.area.map(|_| {
            item.y() + grid_item_border_box_baseline(&estimates[index], item, baseline_set)
        })
    })
}

fn grid_opposite_baseline_set(baseline_set: GridBaselineSet) -> GridBaselineSet {
    match baseline_set {
        GridBaselineSet::First => GridBaselineSet::Last,
        GridBaselineSet::Last => GridBaselineSet::First,
    }
}

fn grid_item_is_container_baseline_eligible(
    item: &GridItemLayout,
    row_index: u16,
    baseline_set: GridBaselineSet,
) -> bool {
    item.area.is_some_and(|area| match baseline_set {
        GridBaselineSet::First => area.row_start == row_index,
        GridBaselineSet::Last => area.row_end.saturating_sub(1) == row_index,
    })
}

fn grid_item_has_baseline(estimate: &GridItemEstimate, baseline_set: GridBaselineSet) -> bool {
    match baseline_set {
        GridBaselineSet::First => estimate.first_baseline.is_some(),
        GridBaselineSet::Last => estimate.last_baseline.is_some(),
    }
}

fn grid_container_baseline_row(
    items: &[GridItemLayout],
    baseline_set: GridBaselineSet,
) -> Option<u16> {
    items
        .iter()
        .filter_map(|item| item.area)
        .map(|area| match baseline_set {
            GridBaselineSet::First => area.row_start,
            GridBaselineSet::Last => area.row_end.saturating_sub(1),
        })
        .reduce(match baseline_set {
            GridBaselineSet::First => u16::min,
            GridBaselineSet::Last => u16::max,
        })
}

fn grid_item_border_box_baseline(
    estimate: &GridItemEstimate,
    item: &GridItemLayout,
    baseline_set: GridBaselineSet,
) -> f32 {
    match baseline_set {
        GridBaselineSet::First => estimate.first_baseline.unwrap_or(item.height()),
        GridBaselineSet::Last => estimate.last_baseline.unwrap_or(0.0),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

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

    fn baseline_child_with_height_percent() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        style.box_values.height = css::PhysicalHeight::from_computed(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.2),
            ),
        );
        style
    }

    fn first_row_area() -> GridItemArea {
        GridItemArea {
            row_start: 1,
            row_end: 2,
            column_start: 1,
            column_end: 2,
        }
    }

    fn available_grid_space(block_size: Option<f32>) -> GridPhysicalAvailableSpace {
        GridPhysicalAvailableSpace {
            width_basis: grid_percentage_basis(
                Some(content_box_pt(100.0)),
                GridAvailableSizeSource::ContainerInlineSize,
            ),
            height_basis: grid_percentage_basis(
                block_size.map(content_box_pt),
                GridAvailableSizeSource::ContainerBlockSize,
            ),
        }
    }

    #[test]
    fn percentage_item_in_intrinsic_row_uses_permanent_baseline_fallback() {
        let container = ComputedStyle::initial();
        let mut child = baseline_child_with_height_percent();
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Auto);
        child.align_content = css::AlignContent::new(ContentAlignmentKeyword::Baseline);

        let resolution = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::ContentAlignment,
            available_grid_space(Some(100.0)),
        );

        assert_eq!(
            resolution,
            GridBaselineParticipation::Fallback {
                baseline_set: GridBaselineSet::First,
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
            }
        );
    }

    #[test]
    fn percentage_item_in_fixed_row_can_share_its_baseline() {
        let mut container = ComputedStyle::initial();
        container.grid_template_rows = grid_tracks(vec![track(
            css::GridMinTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::from_points(
                40.0,
            )),
            css::GridMaxTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::from_points(
                40.0,
            )),
        )]);
        let child = baseline_child_with_height_percent();

        assert_eq!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(Some(100.0)),
            ),
            GridBaselineParticipation::Shares(GridBaselineSet::First)
        );
    }

    #[test]
    fn indefinite_flexible_row_is_intrinsic_for_baseline_cycle_detection() {
        let mut container = ComputedStyle::initial();
        container.grid_template_rows = grid_tracks(vec![track(
            css::GridMinTrackBreadth::Auto,
            css::GridMaxTrackBreadth::Flex(1.0),
        )]);
        let child = baseline_child_with_height_percent();

        assert!(matches!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(None),
            ),
            GridBaselineParticipation::Fallback {
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
                ..
            }
        ));
    }

    #[test]
    fn first_and_last_baseline_requests_retain_their_fallback_edges() {
        let container = ComputedStyle::initial();
        let mut child = baseline_child_with_height_percent();
        let first = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::SelfAlignment,
            available_grid_space(Some(100.0)),
        );
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::LastBaseline);
        let last = resolve_grid_item_baseline_participation(
            &container,
            &child,
            Some(first_row_area()),
            GridAxis::Row,
            GridBaselineAlignmentSource::SelfAlignment,
            available_grid_space(Some(100.0)),
        );

        assert!(first.requests(GridBaselineSet::First));
        assert!(!first.requests(GridBaselineSet::Last));
        assert!(last.requests(GridBaselineSet::Last));
        assert!(!last.requests(GridBaselineSet::First));
        assert_eq!(first.fallback_set(), Some(GridBaselineSet::First));
        assert_eq!(last.fallback_set(), Some(GridBaselineSet::Last));
    }

    #[test]
    fn vertical_grid_projects_row_dependency_to_physical_width() {
        let mut container = ComputedStyle::initial();
        container.writing_mode = WritingMode::VerticalLr;
        let mut child = ComputedStyle::initial();
        child.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        child.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.2),
        );

        assert!(matches!(
            resolve_grid_item_baseline_participation(
                &container,
                &child,
                Some(first_row_area()),
                GridAxis::Row,
                GridBaselineAlignmentSource::SelfAlignment,
                available_grid_space(Some(100.0)),
            ),
            GridBaselineParticipation::Fallback {
                reason: GridBaselineFallbackReason::CyclicTrackSizing,
                ..
            }
        ));
    }

    fn baseline_test_item(area: GridItemArea, y: f32, height: f32) -> GridItemLayout {
        GridItemLayout::new(
            GridRect::new(GridPoint::new(0.0, y), GridSize::new(20.0, height)),
            Some(area),
        )
    }

    fn baseline_test_resolution(baseline_set: GridBaselineSet) -> GridBaselineResolution {
        GridBaselineResolution {
            row_self: GridBaselineParticipation::Shares(baseline_set),
            column_self: GridBaselineParticipation::NotRequested,
            row_content: GridBaselineParticipation::NotRequested,
            column_content: GridBaselineParticipation::NotRequested,
        }
    }

    #[test]
    fn baseline_sizing_eligibility_requires_a_horizontal_sharing_pair() {
        let first = baseline_test_item(
            GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            0.0,
            30.0,
        );
        let same_first_row = baseline_test_item(
            GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 2,
                column_end: 3,
            },
            0.0,
            30.0,
        );
        let next_row = baseline_test_item(
            GridItemArea {
                row_start: 2,
                row_end: 3,
                column_start: 1,
                column_end: 2,
            },
            30.0,
            30.0,
        );
        let first_resolution = baseline_test_resolution(GridBaselineSet::First);

        assert!(!grid_baseline_sizing_may_need_shims(
            &ComputedStyle::initial(),
            &[first_resolution],
            std::slice::from_ref(&first),
        ));
        assert!(grid_baseline_sizing_may_need_shims(
            &ComputedStyle::initial(),
            &[first_resolution, first_resolution],
            &[first.clone(), same_first_row.clone()],
        ));
        assert!(!grid_baseline_sizing_may_need_shims(
            &ComputedStyle::initial(),
            &[first_resolution, first_resolution],
            &[first.clone(), next_row],
        ));
        assert!(!grid_baseline_sizing_may_need_shims(
            &ComputedStyle::initial(),
            &[
                first_resolution,
                baseline_test_resolution(GridBaselineSet::Last),
            ],
            &[first.clone(), same_first_row],
        ));

        let mut vertical = ComputedStyle::initial();
        vertical.writing_mode = WritingMode::VerticalLr;
        assert!(!grid_baseline_sizing_may_need_shims(
            &vertical,
            &[first_resolution, first_resolution],
            &[first.clone(), first],
        ));
    }

    #[test]
    fn baseline_shims_equalize_first_baselines_without_used_margins() {
        let style = ComputedStyle::initial();
        let items = vec![
            baseline_test_item(
                GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
                0.0,
                30.0,
            ),
            baseline_test_item(
                GridItemArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
                0.0,
                30.0,
            ),
        ];
        let mut first = GridItemEstimate::fixed(20.0, 30.0);
        first.first_baseline = Some(8.0);
        let mut second = GridItemEstimate::fixed(20.0, 30.0);
        second.first_baseline = Some(14.0);
        let plan = grid_baseline_plan(
            &style,
            &[],
            &[first, second],
            &[
                baseline_test_resolution(GridBaselineSet::First),
                baseline_test_resolution(GridBaselineSet::First),
            ],
            &items,
        );

        assert_eq!(plan.shim(0).unwrap().top, 6.0);
        assert_eq!(plan.shim(1).unwrap().top, 0.0);
        assert_eq!(plan.shim(0).unwrap().bottom, 0.0);
    }

    #[test]
    fn grid_container_baseline_prefers_last_group_before_item_baseline() {
        let style = ComputedStyle::initial();
        let items = vec![baseline_test_item(
            GridItemArea {
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
            10.0,
            30.0,
        )];
        let mut estimate = GridItemEstimate::fixed(20.0, 30.0);
        estimate.first_baseline = Some(4.0);
        estimate.last_baseline = Some(20.0);
        let resolution = baseline_test_resolution(GridBaselineSet::Last);

        assert_eq!(
            grid_container_baseline(
                &style,
                &[estimate],
                &[resolution],
                &items,
                GridBaselineSet::First,
            ),
            Some(30.0)
        );
    }
}
