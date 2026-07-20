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

    fn requires_ch_advance(&self) -> bool {
        match self {
            Self::Track(_, size) => size.requires_ch_advance(),
            Self::Repeat(_, repeat) => repeat.requires_ch_advance(),
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

    fn requires_ch_advance(&self) -> bool {
        self.tracks
            .iter()
            .any(GridTrackListComponent::requires_ch_advance)
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

    fn requires_ch_advance(&self) -> bool {
        self.min.requires_ch_advance() || self.max.requires_ch_advance()
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

    fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
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

    fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) | Self::FitContent(value) if value.requires_ch_advance())
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
    pub(crate) tracks: Vec<GridTrackSize>,
}

impl GridAutoTrackList {
    pub(crate) fn initial() -> Self {
        Self {
            tracks: vec![GridTrackSize::AUTO],
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        for track in &mut self.tracks {
            track.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.tracks
            .iter()
            .any(|track| track.clone().requires_ch_advance())
    }

    /// Scale fixed implicit-track breadth components while retaining their
    /// percentage and intrinsic semantics.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/css-grid-1/#implicit-grids>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        for track in &mut self.tracks {
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
pub(crate) struct GridLanesDirection {
    pub(crate) axis: GridLanesDirectionAxis,
    pub(crate) track_reverse: bool,
    pub(crate) fill_reverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridLanesDirectionAxis {
    Normal,
    Row,
    Column,
}

impl GridLanesDirection {
    pub(crate) const NORMAL: Self = Self {
        axis: GridLanesDirectionAxis::Normal,
        track_reverse: false,
        fill_reverse: false,
    };
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
pub(crate) struct GridLinePlacement {
    pub(crate) name: Option<String>,
    pub(crate) index: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GridSpanPlacement {
    pub(crate) name: Option<String>,
    pub(crate) span: Option<u16>,
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
        for track in &mut self.tracks {
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
