use std::num::{NonZeroI32, NonZeroU16};

use super::*;

/// Computed CSS Grid explicit track list.
///
/// CSS Grid stores explicit track definitions as a sequence of track sizing
/// functions and `repeat()` fragments. Layout expands the list once the grid
/// container's available size and auto-repeat rules are known:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridTrackList {
    None,
    /// A subgridded axis and its locally assigned line names.
    ///
    /// The track sizes and the final number of lines are supplied by the
    /// parent grid at layout time.  Keeping the authored name-repeat list
    /// intact here is important: `repeat(auto-fill, ...)` on a subgrid fills
    /// its *used parent span*, rather than a container size.
    /// <https://drafts.csswg.org/css-grid-2/#subgrid-listing>
    Subgrid {
        line_names: SubgridLineNameList,
    },
    Tracks {
        components: Vec<GridTrackListComponent>,
        trailing_names: GridLineNames,
    },
}

/// The `<line-name-list>` following the `subgrid` keyword.
///
/// Unlike a standalone track list, every entry names one inherited grid line;
/// it never defines a track.  The list is expanded only after the subgrid's
/// used parent span is known.
/// <https://drafts.csswg.org/css-grid-2/#typedef-line-name-list>
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SubgridLineNameList {
    pub(crate) components: Vec<SubgridLineNameComponent>,
}

impl SubgridLineNameList {
    /// Expand local names to the used number of inherited lines.
    ///
    /// Fixed repeats and overlong lists are truncated.  The sole allowed
    /// `auto-fill` name repeat is expanded just far enough to fill the span,
    /// then the result is truncated at the final inherited line.  This avoids
    /// materializing an authored `repeat(100, ...)` beyond the subgrid span.
    /// <https://drafts.csswg.org/css-grid-2/#auto-repeat>
    pub(crate) fn expand_to_line_count(&self, line_count: usize) -> Vec<GridLineNames> {
        let fixed_slot_count = self
            .components
            .iter()
            .filter_map(|component| match component {
                SubgridLineNameComponent::LineNames(_) => Some(1_usize),
                SubgridLineNameComponent::Repeat {
                    count: SubgridLineNameRepeatCount::Number(count),
                    line_names,
                } => Some(usize::from(*count).saturating_mul(line_names.len())),
                SubgridLineNameComponent::Repeat {
                    count: SubgridLineNameRepeatCount::AutoFill,
                    ..
                } => None,
            })
            .sum::<usize>();
        let mut result = Vec::with_capacity(line_count);
        for component in &self.components {
            match component {
                SubgridLineNameComponent::LineNames(names) => result.push(names.clone()),
                SubgridLineNameComponent::Repeat { count, line_names } => {
                    let repetitions = match count {
                        SubgridLineNameRepeatCount::Number(count) => usize::from(*count),
                        SubgridLineNameRepeatCount::AutoFill => {
                            let remaining = line_count.saturating_sub(fixed_slot_count);
                            remaining.div_ceil(line_names.len().max(1))
                        }
                    };
                    for _ in 0..repetitions {
                        result.extend(line_names.iter().cloned());
                    }
                }
            }
        }
        result.truncate(line_count);
        result.resize_with(line_count, Vec::new);
        result
    }
}

/// One line-name slot or a repetition of adjacent line-name slots in a
/// subgrid declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubgridLineNameComponent {
    LineNames(GridLineNames),
    Repeat {
        count: SubgridLineNameRepeatCount,
        line_names: Vec<GridLineNames>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubgridLineNameRepeatCount {
    Number(u16),
    AutoFill,
}

impl GridTrackList {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.resolve_font_metric_lengths(ch_advance);
            }
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.resolve_root_font_metric_lengths(basis);
            }
        }
    }

    /// Layout and paint containment establish an independent formatting
    /// context, so a subgridded axis computes to the used value `none`.
    /// <https://drafts.csswg.org/css-grid-2/#subgrid-listing>
    pub(crate) fn resolve_contained_subgrid(&mut self) {
        if matches!(self, Self::Subgrid { .. }) {
            *self = Self::None;
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Tracks { components, .. } if components.iter().any(GridTrackListComponent::requires_ch_advance))
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Tracks { components, .. } if components.iter().any(GridTrackListComponent::requires_root_font_metrics))
    }

    /// Scale fixed track-breadth components at the CSS `zoom` used-value
    /// boundary.
    ///
    /// Percentages remain relative to the grid container's already zoomed
    /// content box, while flex and intrinsic track sizing functions remain
    /// algorithmic values.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/css-grid-1/#track-sizing>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.scale_fixed_length_components(factor);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridTrackListComponent {
    Track(GridLineNames, GridTrackSize),
    Repeat(GridLineNames, GridRepeat),
}

impl GridTrackListComponent {
    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::Track(_, size) => size.resolve_font_metric_lengths(ch_advance),
            Self::Repeat(_, repeat) => repeat.resolve_font_metric_lengths(ch_advance),
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Track(_, size) => size.resolve_root_font_metric_lengths(basis),
            Self::Repeat(_, repeat) => repeat.resolve_root_font_metric_lengths(basis),
        }
    }

    fn requires_ch_advance(&self) -> bool {
        match self {
            Self::Track(_, size) => size.requires_ch_advance(),
            Self::Repeat(_, repeat) => repeat.requires_ch_advance(),
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Track(_, size) => size.requires_root_font_metrics(),
            Self::Repeat(_, repeat) => repeat.requires_root_font_metrics(),
        }
    }

    fn scale_fixed_length_components(&mut self, factor: f32) {
        match self {
            Self::Track(_, size) => size.scale_fixed_length_components(factor),
            Self::Repeat(_, repeat) => repeat.scale_fixed_length_components(factor),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridRepeat {
    pub(crate) count: GridRepeatCount,
    pub(crate) tracks: Vec<GridTrackListComponent>,
    pub(crate) trailing_names: GridLineNames,
}

impl GridRepeat {
    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        for track in &mut self.tracks {
            track.resolve_font_metric_lengths(ch_advance);
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        for track in &mut self.tracks {
            track.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_ch_advance(&self) -> bool {
        self.tracks
            .iter()
            .any(GridTrackListComponent::requires_ch_advance)
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.tracks
            .iter()
            .any(GridTrackListComponent::requires_root_font_metrics)
    }

    fn scale_fixed_length_components(&mut self, factor: f32) {
        for track in &mut self.tracks {
            track.scale_fixed_length_components(factor);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridRepeatCount {
    Number(u16),
    AutoFill,
    AutoFit,
}

pub(crate) type GridLineNames = Vec<String>;

/// Computed CSS Grid track sizing function.
///
/// CSS Grid defines each track as a min/max sizing function. Shorthands such
/// as bare `<flex>` and `<length-percentage>` are normalized during parsing to
/// this min/max representation:
/// <https://www.w3.org/TR/css-grid-1/#track-sizing>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridTrackSize {
    pub(crate) min: GridMinTrackBreadth,
    pub(crate) max: GridMaxTrackBreadth,
}

impl GridTrackSize {
    pub(crate) const AUTO: Self = Self {
        min: GridMinTrackBreadth::Auto,
        max: GridMaxTrackBreadth::Auto,
    };

    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.min.resolve_font_metric_lengths(ch_advance);
        self.max.resolve_font_metric_lengths(ch_advance);
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.min.resolve_root_font_metric_lengths(basis);
        self.max.resolve_root_font_metric_lengths(basis);
    }

    fn requires_ch_advance(&self) -> bool {
        self.min.requires_ch_advance() || self.max.requires_ch_advance()
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.min.requires_root_font_metrics() || self.max.requires_root_font_metrics()
    }

    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.min.scale_fixed_length_components(factor);
        self.max.scale_fixed_length_components(factor);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridMinTrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    LengthPercentage(ComputedLengthPercentage),
}

impl GridMinTrackBreadth {
    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }

    fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::LengthPercentage(value) = self {
            value.scale_fixed_length_components(factor);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridMaxTrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    LengthPercentage(ComputedLengthPercentage),
    Flex(f32),
    FitContent(ComputedLengthPercentage),
}

impl GridMaxTrackBreadth {
    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::Auto | Self::MinContent | Self::MaxContent | Self::Flex(_) => {}
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.resolve_root_font_metric_lengths(basis);
            }
            Self::Auto | Self::MinContent | Self::MaxContent | Self::Flex(_) => {}
        }
    }

    fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) | Self::FitContent(value) if value.requires_ch_advance())
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) | Self::FitContent(value) if value.requires_root_font_metrics())
    }

    fn scale_fixed_length_components(&mut self, factor: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.scale_fixed_length_components(factor);
            }
            Self::Auto | Self::MinContent | Self::MaxContent | Self::Flex(_) => {}
        }
    }
}

/// Computed CSS Grid auto-track list for implicit tracks.
///
/// CSS Grid repeats this list as needed for implicit rows or columns:
/// <https://www.w3.org/TR/css-grid-1/#auto-tracks>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridAutoTrackList {
    representation: GridAutoTrackListRepresentation,
}

#[derive(Debug, Clone, PartialEq)]
enum GridAutoTrackListRepresentation {
    /// The overwhelmingly common one-track form, including the initial
    /// `auto` value. Keeping it inline avoids an allocation for each axis of
    /// every computed style.
    Single(GridTrackSize),
    Multiple(Vec<GridTrackSize>),
}

impl GridAutoTrackList {
    pub(crate) fn initial() -> Self {
        Self {
            representation: GridAutoTrackListRepresentation::Single(GridTrackSize::AUTO),
        }
    }

    /// Builds a non-empty auto-track list, keeping a singleton inline.
    pub(crate) fn from_tracks(mut tracks: Vec<GridTrackSize>) -> Option<Self> {
        match tracks.len() {
            0 => None,
            1 => Some(Self {
                representation: GridAutoTrackListRepresentation::Single(
                    tracks.pop().expect("checked singleton track"),
                ),
            }),
            _ => Some(Self {
                representation: GridAutoTrackListRepresentation::Multiple(tracks),
            }),
        }
    }

    pub(crate) fn as_slice(&self) -> &[GridTrackSize] {
        match &self.representation {
            GridAutoTrackListRepresentation::Single(track) => std::slice::from_ref(track),
            GridAutoTrackListRepresentation::Multiple(tracks) => tracks,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&GridTrackSize> {
        self.as_slice().get(index)
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, GridTrackSize> {
        self.as_slice().iter()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut GridTrackSize> {
        match &mut self.representation {
            GridAutoTrackListRepresentation::Single(track) => {
                std::slice::from_mut(track).iter_mut()
            }
            GridAutoTrackListRepresentation::Multiple(tracks) => tracks.iter_mut(),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        for track in self.iter_mut() {
            track.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        for track in self.iter_mut() {
            track.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.iter().any(|track| track.clone().requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.iter().any(|track| track.requires_root_font_metrics())
    }

    /// Scale fixed implicit-track breadth components while retaining their
    /// percentage and intrinsic semantics.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/css-grid-1/#implicit-grids>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        for track in self.iter_mut() {
            track.scale_fixed_length_components(factor);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridTemplateAreas {
    None,
    Areas(Vec<GridTemplateAreaRow>),
}

impl GridTemplateAreas {
    pub(crate) const NONE: Self = Self::None;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GridTemplateAreaRow {
    pub(crate) cells: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

impl GridAutoFlow {
    pub(crate) const ROW: Self = Self::Row;
}

/// Direction of the fixed and stacking axes in Grid Lanes Layout.
///
/// `track-reverse` reverses the grid-axis packing order; `fill-reverse`
/// reverses the stacking-axis fill direction:
/// <https://drafts.csswg.org/css-grid-3/#grid-lanes-direction-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridLanesDirection {
    Normal,
    Axis {
        axis: GridLanesAxis,
        track_reverse: bool,
        fill_reverse: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridLanesAxis {
    Row,
    Column,
}

impl GridLanesDirection {
    pub(crate) const NORMAL: Self = Self::Normal;
}

/// Tie threshold used by Grid Lanes auto-placement.
///
/// Grid Lanes considers track positions within this distance of the shortest
/// position equally good, then uses its auto-placement cursor to keep visual
/// order moving forward: <https://drafts.csswg.org/css-grid-3/#flow-tolerance-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridLanesFlowTolerance {
    Normal,
    LengthPercentage(ComputedLengthPercentage),
    Infinite,
}

impl GridLanesFlowTolerance {
    pub(crate) const NORMAL: Self = Self::Normal;

    /// Scale the fixed length component at the CSS `zoom` used-value
    /// boundary while retaining percentage and keyword semantics.
    ///
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-grid-3/#flow-tolerance-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::LengthPercentage(value) = self {
            value.scale_fixed_length_components(factor);
        }
    }
}

/// Computed CSS Grid item placement for one axis edge.
///
/// The CSS grammar allows automatic placement, integer lines, named lines,
/// and spans. Layout resolves these against the explicit and implicit grid
/// after named lines and template areas are known:
/// <https://www.w3.org/TR/css-grid-1/#line-placement>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GridPlacement {
    Auto,
    Line(GridLinePlacement),
    Span(GridSpanPlacement),
}

impl GridPlacement {
    pub(crate) const AUTO: Self = Self::Auto;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GridLinePlacement {
    Number(NonZeroI32),
    Named {
        name: String,
        occurrence: Option<NonZeroI32>,
    },
}

impl GridLinePlacement {
    pub(crate) fn name(&self) -> Option<&str> {
        match self {
            Self::Number(_) => None,
            Self::Named { name, .. } => Some(name),
        }
    }

    pub(crate) fn index(&self) -> Option<i32> {
        match self {
            Self::Number(index) => Some(index.get()),
            Self::Named { occurrence, .. } => occurrence.map(NonZeroI32::get),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GridSpanPlacement {
    Count(NonZeroU16),
    Named {
        name: String,
        count: Option<NonZeroU16>,
    },
}

impl GridSpanPlacement {
    pub(crate) fn name(&self) -> Option<&str> {
        match self {
            Self::Count(_) => None,
            Self::Named { name, .. } => Some(name),
        }
    }

    pub(crate) fn count(&self) -> Option<u16> {
        match self {
            Self::Count(count) => Some(count.get()),
            Self::Named { count, .. } => count.map(NonZeroU16::get),
        }
    }
}

impl ResolveViewportLengths for GridTrackList {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.resolve_viewport_lengths(basis);
            }
        }
    }
}

impl ResolveViewportLengths for GridTrackListComponent {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::Track(_, size) => size.resolve_viewport_lengths(basis),
            Self::Repeat(_, repeat) => repeat.resolve_viewport_lengths(basis),
        }
    }
}

impl ResolveViewportLengths for GridRepeat {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        for track in &mut self.tracks {
            track.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GridTrackSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.min.resolve_viewport_lengths(basis);
        self.max.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for GridMinTrackBreadth {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GridMaxTrackBreadth {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.resolve_viewport_lengths(basis);
            }
            Self::Auto | Self::MinContent | Self::MaxContent | Self::Flex(_) => {}
        }
    }
}

impl ResolveViewportLengths for GridAutoTrackList {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        for track in self.iter_mut() {
            track.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GridLanesFlowTolerance {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_auto_track_lists_keep_single_tracks_inline() {
        let initial = GridAutoTrackList::initial();
        let parsed_single = GridAutoTrackList::from_tracks(vec![GridTrackSize::AUTO])
            .expect("single test track is non-empty");
        let multiple = GridAutoTrackList::from_tracks(vec![
            GridTrackSize::AUTO,
            GridTrackSize {
                min: GridMinTrackBreadth::MinContent,
                max: GridMaxTrackBreadth::MaxContent,
            },
        ])
        .expect("multiple test tracks are non-empty");

        assert!(matches!(
            &initial.representation,
            GridAutoTrackListRepresentation::Single(_)
        ));
        assert!(matches!(
            &parsed_single.representation,
            GridAutoTrackListRepresentation::Single(_)
        ));
        assert!(matches!(
            &multiple.representation,
            GridAutoTrackListRepresentation::Multiple(_)
        ));
        assert_eq!(initial.as_slice(), [GridTrackSize::AUTO]);
        assert_eq!(multiple.len(), 2);
        assert_eq!(GridAutoTrackList::from_tracks(Vec::new()), None);
    }

    #[test]
    fn inline_grid_auto_tracks_resolve_fixed_lengths() {
        let mut tracks = GridAutoTrackList::from_tracks(vec![GridTrackSize {
            min: GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(3.0)),
            max: GridMaxTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(3.0)),
        }])
        .expect("single test track is non-empty");

        tracks.scale_fixed_length_components(2.0);

        let GridMinTrackBreadth::LengthPercentage(minimum) = &tracks
            .get(0)
            .expect("inline test track remains present")
            .min
        else {
            panic!("minimum remains a length percentage");
        };
        assert_eq!(minimum.length_points(), 6.0);
    }
}
