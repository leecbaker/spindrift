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
    Tracks {
        components: Vec<GridTrackListComponent>,
        trailing_names: GridLineNames,
    },
}

impl GridTrackList {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.resolve_font_metric_lengths(ch_advance);
            }
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::Tracks { components, .. } = self {
            for component in components {
                component.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
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
    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::Track(_, size) => size.resolve_font_metric_lengths(ch_advance),
            Self::Repeat(_, repeat) => repeat.resolve_font_metric_lengths(ch_advance),
        }
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::Track(_, size) => size.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::Repeat(_, repeat) => repeat.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
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
    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        for track in &mut self.tracks {
            track.resolve_font_metric_lengths(ch_advance);
        }
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        for track in &mut self.tracks {
            track.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GridTrackSize {
    pub(crate) min: GridMinTrackBreadth,
    pub(crate) max: GridMaxTrackBreadth,
}

impl GridTrackSize {
    pub(crate) const AUTO: Self = Self {
        min: GridMinTrackBreadth::Auto,
        max: GridMaxTrackBreadth::Auto,
    };

    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.min.resolve_font_metric_lengths(ch_advance);
        self.max.resolve_font_metric_lengths(ch_advance);
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.min.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.max.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GridMinTrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    LengthPercentage(ComputedLengthPercentage),
}

impl GridMinTrackBreadth {
    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GridMaxTrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    LengthPercentage(ComputedLengthPercentage),
    Flex(f32),
    FitContent(ComputedLengthPercentage),
}

impl GridMaxTrackBreadth {
    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::Auto | Self::MinContent | Self::MaxContent | Self::Flex(_) => {}
        }
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(value) => {
                value.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        for track in &mut self.tracks {
            track.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        for track in &mut self.tracks {
            track.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
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
