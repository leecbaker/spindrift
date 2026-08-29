use super::lanes::GridLanesItemPlacement;
use super::*;

/// Final geometry of one explicit parent grid axis.
///
/// A subgrid borrows a contiguous range of these lines instead of creating
/// tracks of its own. Track spans and outer edges are derived once from the
/// parent’s used tracks; line offsets and decoration gutters are projections
/// of that geometry, never a source for reconstructing it.
/// <https://drafts.csswg.org/css-grid-2/#subgrids>
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResolvedGridAxis {
    outer_start: f32,
    outer_end: f32,
    line_offsets: Vec<f32>,
    track_starts: Vec<f32>,
    track_ends: Vec<f32>,
    line_names: Vec<css::GridLineNames>,
}

impl ResolvedGridAxis {
    fn from_parent_layout(
        parent_offsets: &[f32],
        parent_track_sizes: &[f32],
        parent_gutters: &[GapDecorationGutter],
        parent_line_names: &[css::GridLineNames],
    ) -> Self {
        let line_count = parent_track_sizes.len().saturating_add(1);
        let mut line_names = parent_line_names.to_vec();
        line_names.resize_with(line_count, Vec::new);
        line_names.truncate(line_count);

        let track_count = parent_track_sizes.len();
        let mut track_starts = Vec::with_capacity(track_count);
        let mut track_ends = Vec::with_capacity(track_count);
        let outer_start = parent_offsets.first().copied().unwrap_or(0.0);
        for (index, size) in parent_track_sizes.iter().enumerate() {
            let line_number = u16::try_from(index + 1).ok();
            let following_line_number = u16::try_from(index + 2).ok();
            let gutter_at_line = |line_number| {
                parent_gutters
                    .iter()
                    .find(|gutter| gutter.grid_line == line_number)
            };
            let start = if index == 0 {
                outer_start
            } else {
                gutter_at_line(line_number)
                    .map(|gutter| gutter.span.end)
                    .unwrap_or_else(|| parent_offsets.get(index).copied().unwrap_or(outer_start))
            };
            // A grid area ends with its used parent track, not with a gutter
            // or a legacy line-offset projection.
            // <https://www.w3.org/TR/css-grid-2/#gutters>
            let end = start + size.max(0.0);
            debug_assert!(start <= end);
            debug_assert!(
                gutter_at_line(following_line_number).is_none_or(|gutter| end <= gutter.span.start)
            );
            track_starts.push(start);
            track_ends.push(end);
        }
        let outer_end = track_ends.last().copied().unwrap_or(outer_start);
        let mut line_offsets = track_starts.clone();
        line_offsets.push(outer_end);
        Self {
            outer_start,
            outer_end,
            line_offsets,
            track_starts,
            track_ends,
            line_names,
        }
    }

    fn subgrid_slice(
        &self,
        start_line: u16,
        end_line: u16,
        local_names: &css::SubgridLineNameList,
        child_style: &ComputedStyle,
        child_axis: GridAxis,
        inherit_parent_line_names: bool,
    ) -> Option<ResolvedSubgridAxis> {
        debug_assert!(self.outer_start <= self.outer_end);
        let start = usize::from(start_line.checked_sub(1)?);
        let end = usize::from(end_line.checked_sub(1)?);
        if start >= end || end > self.track_starts.len() {
            return None;
        }
        let track_count = end - start;
        let physical_line_names = if inherit_parent_line_names {
            self.line_names.get(start..=end)?.to_vec()
        } else {
            vec![Vec::new(); track_count + 1]
        };
        let logical_axis = match child_axis {
            GridAxis::Column => LogicalAxis::Inline,
            GridAxis::Row => LogicalAxis::Block,
        };
        let reversed = WritingModeAxes::new(child_style.writing_mode, child_style.used_direction())
            .is_reversed(logical_axis);
        let logical_to_physical_line = if reversed {
            (0..=track_count).rev().collect::<Vec<_>>()
        } else {
            (0..=track_count).collect::<Vec<_>>()
        };
        let mut line_names = logical_to_physical_line
            .iter()
            .map(|&physical| physical_line_names[physical].clone())
            .collect::<Vec<_>>();
        for (inherited, local) in line_names
            .iter_mut()
            .zip(local_names.expand_to_line_count(track_count + 1))
        {
            inherited.extend(local);
        }
        let physical_line_names = logical_to_physical_line.iter().enumerate().fold(
            vec![Vec::new(); track_count + 1],
            |mut names, (logical, &physical)| {
                names[physical] = line_names[logical].clone();
                names
            },
        );
        Some(ResolvedSubgridAxis {
            line_offsets: self.track_starts[start..end]
                .iter()
                .map(|offset| *offset - self.track_starts[start])
                .chain(std::iter::once(
                    self.track_ends[end - 1] - self.track_starts[start],
                ))
                .collect(),
            track_starts: self.track_starts[start..end]
                .iter()
                .map(|offset| *offset - self.track_starts[start])
                .collect(),
            track_ends: self.track_ends[start..end]
                .iter()
                .map(|offset| *offset - self.track_starts[start])
                .collect(),
            gutter_sizes: (start + 1..end)
                .map(|index| (self.track_starts[index] - self.track_ends[index - 1]).max(0.0))
                .collect(),
            line_names,
            physical_line_names,
            logical_to_physical_line,
        })
    }
}

/// A subgrid's borrowed axis in its local coordinate space.
///
/// This is deliberately geometry rather than a `GridTrackList`: inherited
/// tracks have already been sized by the parent and must not participate in a
/// second independent allocation.  The Taffy adapter receives fixed views of
/// this geometry only at its boundary.
/// <https://drafts.csswg.org/css-grid-2/#subgrid-track-sizing>
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResolvedSubgridAxis {
    line_offsets: Vec<f32>,
    track_starts: Vec<f32>,
    track_ends: Vec<f32>,
    /// Parent gutters are retained individually for edge adjustments and Grid
    /// Lanes geometry. Taffy receives the first value only at its uniform-gap
    /// adapter boundary.
    gutter_sizes: Vec<f32>,
    /// Names in this subgrid's logical line order, which is the order used by
    /// CSS Grid placement. Physical geometry stays in increasing Taffy order.
    line_names: Vec<css::GridLineNames>,
    physical_line_names: Vec<css::GridLineNames>,
    logical_to_physical_line: Vec<usize>,
}

impl ResolvedSubgridAxis {
    pub(super) fn track_count(&self) -> usize {
        self.track_starts.len()
    }

    pub(super) fn line_names(&self) -> &[css::GridLineNames] {
        &self.line_names
    }

    pub(super) fn physical_line_names(&self) -> &[css::GridLineNames] {
        &self.physical_line_names
    }

    pub(super) fn taffy_tracks(&self) -> Vec<taffy_layout::GridTemplateComponent<String>> {
        self.track_starts
            .iter()
            .zip(&self.track_ends)
            .map(|(start, end)| {
                let size = (*end - *start).max(0.0);
                taffy_layout::GridTemplateComponent::Single(taffy_layout::TrackSizingFunction {
                    min: taffy_layout::MinTrackSizingFunction::length(size),
                    max: taffy_layout::MaxTrackSizingFunction::length(size),
                })
            })
            .collect()
    }

    pub(super) fn taffy_gap(&self) -> f32 {
        self.gutter_sizes.first().copied().unwrap_or(0.0)
    }

    pub(super) fn gutter_sizes(&self) -> &[f32] {
        &self.gutter_sizes
    }

    pub(super) fn line_offsets(&self) -> &[f32] {
        &self.line_offsets
    }

    pub(super) fn track_starts(&self) -> &[f32] {
        &self.track_starts
    }

    pub(super) fn track_ends(&self) -> &[f32] {
        &self.track_ends
    }

    /// Return the used local span of the inherited axis.
    pub(super) fn outer_extent(&self) -> f32 {
        self.line_offsets.last().copied().unwrap_or(0.0)
    }

    /// Resolve a physical Taffy area to the parent-owned track-area span.
    /// <https://www.w3.org/TR/css-grid-2/#subgrids>
    pub(super) fn track_area_span(&self, start_line: u16, end_line: u16) -> Option<(f32, f32)> {
        let start = usize::from(start_line.checked_sub(1)?);
        let end = usize::from(end_line.checked_sub(1)?);
        (start < end && end <= self.track_ends.len())
            .then(|| (self.track_starts[start], self.track_ends[end - 1]))
    }

    fn line_count_i32(&self) -> i32 {
        i32::try_from(self.track_count() + 1).unwrap_or(i32::from(i16::MAX))
    }

    pub(super) fn resolved_range(
        &self,
        start: &css::GridPlacement,
        end: &css::GridPlacement,
        fallback_start: i32,
    ) -> ResolvedSubgridPlacement {
        let last_line = self.line_count_i32();
        let span = |placement: &css::GridPlacement| match placement {
            css::GridPlacement::Span(span) => i32::from(span.count().unwrap_or(1)).max(1),
            _ => 1,
        };
        let line = |placement: &css::GridPlacement| self.hypothetical_line(placement);
        let named_span_end = |start: i32, placement: &css::GridPlacement| {
            self.hypothetical_named_span_end(start, placement)
        };
        let named_span_start = |end: i32, placement: &css::GridPlacement| {
            self.hypothetical_named_span_start(end, placement)
        };
        let (mut start, mut end) = match (line(start), line(end)) {
            (Some(start), Some(end)) => (start, end),
            (Some(start), None) => (
                start,
                named_span_end(start, end).unwrap_or_else(|| start.saturating_add(span(end))),
            ),
            (None, Some(end)) => (
                named_span_start(end, start).unwrap_or_else(|| end.saturating_sub(span(start))),
                end,
            ),
            (None, None) => {
                let start = fallback_start.clamp(1, last_line.saturating_sub(1).max(1));
                (start, start.saturating_add(span(end)))
            }
        };
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        if start == end {
            end = end.saturating_add(1);
        }
        let (start, end) = if end <= 1 {
            (1, 2.min(last_line))
        } else if start >= last_line {
            (last_line.saturating_sub(1).max(1), last_line)
        } else {
            (start.max(1), end.min(last_line))
        };
        self.physical_range(start, end)
    }

    /// Resolve a placement endpoint in the conceptual implicit grid. A
    /// subgrid never materializes these lines, but Grid still uses them to
    /// determine a complete area before clamping it to inherited tracks.
    /// <https://drafts.csswg.org/css-grid-2/#subgrids>
    fn hypothetical_line(&self, placement: &css::GridPlacement) -> Option<i32> {
        let css::GridPlacement::Line(line) = placement else {
            return None;
        };
        let index = line.index().unwrap_or(1);
        let last_line = self.line_count_i32();
        let Some(name) = line.name() else {
            return (index > 0)
                .then_some(index)
                .or_else(|| index.checked_add(last_line)?.checked_add(1));
        };
        let target = index.unsigned_abs();
        let matching = self
            .line_names
            .iter()
            .enumerate()
            .filter_map(|(offset, names)| {
                names
                    .iter()
                    .any(|candidate| candidate == name)
                    .then(|| i32::try_from(offset + 1).ok())
                    .flatten()
            })
            .collect::<Vec<_>>();
        if index > 0 {
            matching
                .get(usize::try_from(target.saturating_sub(1)).ok()?)
                .copied()
                .or_else(|| {
                    i32::try_from(target)
                        .ok()?
                        .checked_sub(i32::try_from(matching.len()).ok()?)
                        .and_then(|missing| last_line.checked_add(missing))
                })
        } else {
            matching
                .iter()
                .rev()
                .nth(usize::try_from(target.saturating_sub(1)).ok()?)
                .copied()
                .or_else(|| {
                    i32::try_from(target)
                        .ok()?
                        .checked_sub(i32::try_from(matching.len()).ok()?)
                        .and_then(|missing| 1_i32.checked_sub(missing))
                })
        }
    }

    fn hypothetical_named_span_end(
        &self,
        start: i32,
        placement: &css::GridPlacement,
    ) -> Option<i32> {
        let css::GridPlacement::Span(span) = placement else {
            return None;
        };
        let name = span.name()?;
        let target = i32::from(span.count().unwrap_or(1)).max(1);
        let first = start.saturating_add(1).max(1);
        let last = self.line_count_i32();
        let matches = (first..=last)
            .filter(|line| {
                usize::try_from(*line - 1)
                    .ok()
                    .and_then(|index| self.line_names.get(index))
                    .is_some_and(|names| names.iter().any(|candidate| candidate == name))
            })
            .collect::<Vec<_>>();
        matches
            .get(usize::try_from(target - 1).ok()?)
            .copied()
            .or_else(|| {
                let missing = target.checked_sub(i32::try_from(matches.len()).ok()?)?;
                start.max(last).checked_add(missing)
            })
    }

    fn hypothetical_named_span_start(
        &self,
        end: i32,
        placement: &css::GridPlacement,
    ) -> Option<i32> {
        let css::GridPlacement::Span(span) = placement else {
            return None;
        };
        let name = span.name()?;
        let target = i32::from(span.count().unwrap_or(1)).max(1);
        let first = end.saturating_sub(1).min(self.line_count_i32());
        let matches = (1..=first)
            .rev()
            .filter(|line| {
                usize::try_from(*line - 1)
                    .ok()
                    .and_then(|index| self.line_names.get(index))
                    .is_some_and(|names| names.iter().any(|candidate| candidate == name))
            })
            .collect::<Vec<_>>();
        matches
            .get(usize::try_from(target - 1).ok()?)
            .copied()
            .or_else(|| {
                let missing = target.checked_sub(i32::try_from(matches.len()).ok()?)?;
                end.min(1).checked_sub(missing)
            })
    }

    fn physical_range(&self, start: i32, end: i32) -> ResolvedSubgridPlacement {
        let logical_start = usize::try_from(start.saturating_sub(1)).unwrap_or(0);
        let logical_end = usize::try_from(end.saturating_sub(1)).unwrap_or(logical_start);
        let physical_start = self.logical_to_physical_line[logical_start];
        let physical_end = self.logical_to_physical_line[logical_end];
        ResolvedSubgridPlacement {
            start: i32::try_from(physical_start.min(physical_end) + 1).unwrap_or(i32::MAX),
            end: i32::try_from(physical_start.max(physical_end) + 1).unwrap_or(i32::MAX),
        }
    }

    /// Resolve explicit placement against the inherited explicit grid.
    ///
    /// A subgridded axis cannot create implicit tracks.  Numeric and named
    /// endpoints therefore clamp to the inherited line range before reaching
    /// Taffy's placement code.  The remaining auto-placement cases are
    /// bounded by the fixed template supplied by this axis.
    /// <https://drafts.csswg.org/css-grid-2/#subgrid-item-placement>
    pub(super) fn clamped_taffy_line(
        &self,
        start: &css::GridPlacement,
        end: &css::GridPlacement,
    ) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
        if matches!(start, css::GridPlacement::Line(_))
            || matches!(end, css::GridPlacement::Line(_))
        {
            return self.resolved_range(start, end, 1).taffy_line();
        }
        taffy_grid_line(start, end)
    }
}

/// A fixed used line range for one inherited grid axis.  The range is always
/// inside its borrowed explicit grid, making implicit-track creation
/// unrepresentable at the Taffy boundary.
/// <https://drafts.csswg.org/css-grid-2/#subgrid-item-placement>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResolvedSubgridPlacement {
    start: i32,
    end: i32,
}

impl ResolvedSubgridPlacement {
    pub(super) fn taffy_line(self) -> taffy_layout::Line<taffy_layout::GridPlacement<String>> {
        taffy_layout::Line {
            start: taffy_layout::line(i16::try_from(self.start).unwrap_or(i16::MAX)),
            end: taffy_layout::line(i16::try_from(self.end).unwrap_or(i16::MAX)),
        }
    }

    pub(super) fn track_range(self) -> std::ops::Range<usize> {
        let start = usize::try_from(self.start.saturating_sub(1)).unwrap_or(0);
        let end = usize::try_from(self.end.saturating_sub(1)).unwrap_or(start);
        start..end
    }

    fn span(self) -> usize {
        usize::try_from(self.end.saturating_sub(self.start)).unwrap_or(1)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ResolvedSubgridItemPlacement {
    pub(super) columns: Option<ResolvedSubgridPlacement>,
    pub(super) rows: Option<ResolvedSubgridPlacement>,
}

/// Resolved parent-track slices for the two logical axes of one grid item.
///
/// The context is consumed by the subgrid's own grid formatting context.  A
/// nested subgrid is derived from its enclosing `GridLayout`, preserving names
/// added by every enclosing level.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::layout) struct ResolvedSubgridContext {
    pub(super) columns: Option<ResolvedSubgridAxis>,
    pub(super) rows: Option<ResolvedSubgridAxis>,
}

impl ResolvedSubgridContext {
    pub(super) fn from_parent(
        parent_style: &ComputedStyle,
        parent_layout: &GridLayout,
        child_style: &ComputedStyle,
        area: GridItemArea,
        grid_lanes_placement: Option<GridLanesItemPlacement>,
    ) -> Option<Self> {
        let columns = match &child_style.grid_template_columns {
            css::GridTrackList::Subgrid { line_names } => subgrid_axis_from_parent(
                parent_style,
                parent_layout,
                child_style,
                area,
                GridAxis::Column,
                line_names,
                grid_lanes_placement,
            ),
            _ => None,
        };
        let rows = match &child_style.grid_template_rows {
            css::GridTrackList::Subgrid { line_names } => subgrid_axis_from_parent(
                parent_style,
                parent_layout,
                child_style,
                area,
                GridAxis::Row,
                line_names,
                grid_lanes_placement,
            ),
            _ => None,
        };
        (columns.is_some() || rows.is_some()).then_some(Self { columns, rows })
    }

    pub(super) fn physical_axis(
        &self,
        physical: GridAxis,
        swaps_axes: bool,
    ) -> Option<&ResolvedSubgridAxis> {
        match (physical, swaps_axes) {
            (GridAxis::Column, false) | (GridAxis::Row, true) => self.columns.as_ref(),
            (GridAxis::Row, false) | (GridAxis::Column, true) => self.rows.as_ref(),
        }
    }

    /// Resolve automatic placement before entering Taffy for a subgrid. The
    /// occupancy model is intentionally bounded by inherited axes; when only
    /// one axis is inherited, the other remains Taffy's standalone axis.
    /// <https://drafts.csswg.org/css-grid-1/#auto-placement-algo>
    pub(super) fn resolve_item_placements(
        &self,
        children: &[GridChild<'_>],
        auto_flow: css::GridAutoFlow,
    ) -> Vec<ResolvedSubgridItemPlacement> {
        let mut placements = vec![ResolvedSubgridItemPlacement::default(); children.len()];
        // With a column-only subgrid in row flow, the ordinary auto-placement
        // algorithm owns the unbounded row axis. Supplying synthetic column
        // lines here would freeze a full cursor at the final inherited column
        // instead of advancing to that next row. Explicit column placement is
        // still clamped at the Taffy boundary below.
        // <https://www.w3.org/TR/css-grid-2/#subgrids>
        // <https://www.w3.org/TR/css-grid-1/#auto-placement-algo>
        if self.columns.is_some()
            && self.rows.is_none()
            && matches!(
                auto_flow,
                css::GridAutoFlow::Row | css::GridAutoFlow::RowDense
            )
        {
            return placements;
        }
        let row_count = self.rows.as_ref().map(ResolvedSubgridAxis::track_count);
        let column_count = self.columns.as_ref().map(ResolvedSubgridAxis::track_count);
        let mut order = (0..children.len()).collect::<Vec<_>>();
        order.sort_by_key(|&index| (children[index].style.order, index));
        let dense = matches!(
            auto_flow,
            css::GridAutoFlow::RowDense | css::GridAutoFlow::ColumnDense
        );
        let column_flow = matches!(
            auto_flow,
            css::GridAutoFlow::Column | css::GridAutoFlow::ColumnDense
        );
        let rows = row_count.unwrap_or(1).max(1);
        let columns = column_count.unwrap_or(1).max(1);
        let mut occupied = vec![false; rows.saturating_mul(columns)];
        let mut cursor_row = 0_usize;
        let mut cursor_column = 0_usize;

        for index in order {
            let child = &children[index];
            let explicit_columns = self.columns.as_ref().and_then(|axis| {
                (matches!(child.style.grid_column_start, css::GridPlacement::Line(_))
                    || matches!(child.style.grid_column_end, css::GridPlacement::Line(_)))
                .then(|| {
                    axis.resolved_range(
                        &child.style.grid_column_start,
                        &child.style.grid_column_end,
                        1,
                    )
                })
            });
            let explicit_rows = self.rows.as_ref().and_then(|axis| {
                (matches!(child.style.grid_row_start, css::GridPlacement::Line(_))
                    || matches!(child.style.grid_row_end, css::GridPlacement::Line(_)))
                .then(|| {
                    axis.resolved_range(&child.style.grid_row_start, &child.style.grid_row_end, 1)
                })
            });
            let column_span = self
                .columns
                .as_ref()
                .map_or(1, |axis| {
                    explicit_columns.map_or_else(
                        || {
                            axis.resolved_range(
                                &child.style.grid_column_start,
                                &child.style.grid_column_end,
                                1,
                            )
                            .span()
                        },
                        ResolvedSubgridPlacement::span,
                    )
                })
                .min(columns);
            let row_span = self
                .rows
                .as_ref()
                .map_or(1, |axis| {
                    explicit_rows.map_or_else(
                        || {
                            axis.resolved_range(
                                &child.style.grid_row_start,
                                &child.style.grid_row_end,
                                1,
                            )
                            .span()
                        },
                        ResolvedSubgridPlacement::span,
                    )
                })
                .min(rows);

            let mut selected = None;
            let start_row =
                explicit_rows.map(|range| usize::try_from(range.start - 1).unwrap_or(0));
            let start_column =
                explicit_columns.map(|range| usize::try_from(range.start - 1).unwrap_or(0));
            let search_row = if dense { 0 } else { cursor_row };
            let search_column = if dense { 0 } else { cursor_column };
            for step in 0..rows.saturating_mul(columns) {
                let row = if column_flow {
                    (search_row + step) % rows
                } else {
                    (search_row + (search_column + step) / columns) % rows
                };
                let column = if column_flow {
                    (search_column + (search_row + step) / rows) % columns
                } else {
                    (search_column + step) % columns
                };
                let row = start_row.unwrap_or(row);
                let column = start_column.unwrap_or(column);
                if row + row_span > rows || column + column_span > columns {
                    continue;
                }
                if (0..row_span).all(|row_offset| {
                    (0..column_span).all(|column_offset| {
                        !occupied[(row + row_offset) * columns + column + column_offset]
                    })
                }) {
                    selected = Some((row, column));
                    break;
                }
            }
            // A full inherited grid overflows at its final valid cell rather
            // than producing an implicit inherited track.
            let (row, column) = selected.unwrap_or((
                start_row.unwrap_or(rows.saturating_sub(row_span)),
                start_column.unwrap_or(columns.saturating_sub(column_span)),
            ));
            for row_offset in 0..row_span {
                for column_offset in 0..column_span {
                    occupied[(row + row_offset) * columns + column + column_offset] = true;
                }
            }
            if !dense {
                if column_flow {
                    cursor_row = row.saturating_add(row_span);
                    cursor_column = column;
                    if cursor_row >= rows {
                        cursor_row = 0;
                        cursor_column = (cursor_column + 1).min(columns.saturating_sub(1));
                    }
                } else {
                    cursor_column = column.saturating_add(column_span);
                    cursor_row = row;
                    if cursor_column >= columns {
                        cursor_column = 0;
                        cursor_row = (cursor_row + 1).min(rows.saturating_sub(1));
                    }
                }
            }
            placements[index] = ResolvedSubgridItemPlacement {
                columns: self.columns.as_ref().map(|axis| {
                    axis.resolved_range(
                        &child.style.grid_column_start,
                        &child.style.grid_column_end,
                        i32::try_from(column + 1).unwrap_or(1),
                    )
                }),
                rows: self.rows.as_ref().map(|axis| {
                    axis.resolved_range(
                        &child.style.grid_row_start,
                        &child.style.grid_row_end,
                        i32::try_from(row + 1).unwrap_or(1),
                    )
                }),
            };
        }
        placements
    }
}

fn subgrid_axis_from_parent(
    parent_style: &ComputedStyle,
    parent_layout: &GridLayout,
    child_style: &ComputedStyle,
    area: GridItemArea,
    child_axis: GridAxis,
    local_names: &css::SubgridLineNameList,
    grid_lanes_placement: Option<GridLanesItemPlacement>,
) -> Option<ResolvedSubgridAxis> {
    let child_logical_axis = match child_axis {
        GridAxis::Column => LogicalAxis::Inline,
        GridAxis::Row => LogicalAxis::Block,
    };
    let child_physical_axis =
        WritingModeAxes::new(child_style.writing_mode, child_style.used_direction())
            .physical_axis(child_logical_axis);
    let column_offsets = parent_layout.columns.line_offsets();
    let row_offsets = parent_layout.rows.line_offsets();
    let parent_gutters = parent_layout.gap_decoration_gutters(parent_style);
    let (offsets, track_sizes, gutters, line_names, start, end) = match child_physical_axis {
        PhysicalAxis::Horizontal => (
            &column_offsets,
            parent_layout.physical_track_sizes(GridAxis::Column),
            &parent_gutters.columns,
            &parent_layout.column_line_names,
            area.column_start,
            area.column_end,
        ),
        PhysicalAxis::Vertical => (
            &row_offsets,
            parent_layout.physical_track_sizes(GridAxis::Row),
            &parent_gutters.rows,
            &parent_layout.row_line_names,
            area.row_start,
            area.row_end,
        ),
    };
    let child_parent_physical_axis = match child_physical_axis {
        PhysicalAxis::Horizontal => GridAxis::Column,
        PhysicalAxis::Vertical => GridAxis::Row,
    };
    let inherit_parent_line_names = !grid_lanes_placement.is_some_and(|placement| {
        placement.is_automatic() && placement.grid_axis() == child_parent_physical_axis
    });
    ResolvedGridAxis::from_parent_layout(offsets, &track_sizes, gutters, line_names).subgrid_slice(
        start,
        end,
        local_names,
        child_style,
        child_axis,
        inherit_parent_line_names,
    )
}

pub(super) fn resolved_parent_line_names(
    tracks: &css::GridTrackList,
    style: &ComputedStyle,
    axis: GridAxis,
    line_count: usize,
) -> Vec<css::GridLineNames> {
    let mut names = match tracks {
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } => explicit_grid_line_names(components, trailing_names).unwrap_or_default(),
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => Vec::new(),
    };
    add_generated_area_line_names(&mut names, &style.grid_template_areas, axis);
    names.resize_with(line_count, Vec::new);
    names.truncate(line_count);
    names
}

/// Materialize a Grid container's final line-name topology in physical Taffy
/// order. The computed track listing is logical, while the stored geometry is
/// always left-to-right or top-to-bottom.
pub(super) fn physical_grid_line_names(
    style: &ComputedStyle,
    physical_axis: GridAxis,
    line_count: usize,
) -> Vec<css::GridLineNames> {
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let inline_is_horizontal = axes.physical_axis(LogicalAxis::Inline) == PhysicalAxis::Horizontal;
    let logical_axis = match (physical_axis, inline_is_horizontal) {
        (GridAxis::Column, true) | (GridAxis::Row, false) => GridAxis::Column,
        (GridAxis::Row, true) | (GridAxis::Column, false) => GridAxis::Row,
    };
    let tracks = match logical_axis {
        GridAxis::Column => &style.grid_template_columns,
        GridAxis::Row => &style.grid_template_rows,
    };
    let mut names = resolved_parent_line_names(tracks, style, logical_axis, line_count);
    let logical_axis = match logical_axis {
        GridAxis::Column => LogicalAxis::Inline,
        GridAxis::Row => LogicalAxis::Block,
    };
    if axes.is_reversed(logical_axis) {
        names.reverse();
    }
    names.resize_with(line_count, Vec::new);
    names.truncate(line_count);
    names
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn with_resolved_subgrid_context<R>(
        &mut self,
        context: ResolvedSubgridContext,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.pending_subgrid_contexts.push(Some(context));
        let result = callback(self);
        let context = self.pending_subgrid_contexts.pop();
        debug_assert!(context.is_some());
        result
    }

    /// Borrow the replay context for an intrinsic Grid probe.
    ///
    /// A direct subgrid's final formatting pass is the only consumer of this
    /// one-shot context.  Earlier intrinsic probes still need the inherited
    /// track topology, but cannot steal it from final replay.
    /// <https://drafts.csswg.org/css-grid-2/#subgrids>
    pub(super) fn resolved_subgrid_context_for_probe(&self) -> Option<ResolvedSubgridContext> {
        self.pending_subgrid_contexts.last().cloned().flatten()
    }

    pub(super) fn take_resolved_subgrid_context(&mut self) -> Option<ResolvedSubgridContext> {
        self.pending_subgrid_contexts
            .last_mut()
            .and_then(Option::take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_track_edges_exclude_following_gutters_from_subgrid_tracks() {
        let axis = ResolvedGridAxis::from_parent_layout(
            &[0.0, 27.5, 55.0, 82.5, 100.0],
            &[17.5, 17.5, 17.5, 17.5],
            &[
                GapDecorationGutter::with_grid_line(17.5, 27.5, Some(2)),
                GapDecorationGutter::with_grid_line(45.0, 55.0, Some(3)),
                GapDecorationGutter::with_grid_line(72.5, 82.5, Some(4)),
            ],
            &vec![Vec::new(); 5],
        );
        assert_eq!(axis.track_starts, vec![0.0, 27.5, 55.0, 82.5]);
        assert_eq!(axis.track_ends, vec![17.5, 45.0, 72.5, 100.0]);

        let slice = axis
            .subgrid_slice(
                1,
                5,
                &css::SubgridLineNameList::default(),
                &ComputedStyle::initial(),
                GridAxis::Column,
                true,
            )
            .unwrap();
        assert_eq!(slice.track_starts(), &[0.0, 27.5, 55.0, 82.5]);
        assert_eq!(slice.track_ends(), &[17.5, 45.0, 72.5, 100.0]);
        assert_eq!(slice.gutter_sizes(), &[10.0, 10.0, 10.0]);
        assert_eq!(slice.outer_extent(), 100.0);
        assert_eq!(
            slice
                .gutter_sizes()
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    (
                        slice.track_ends()[index],
                        slice.track_starts()[index + 1],
                        u16::try_from(index + 2).ok(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (17.5, 27.5, Some(2)),
                (45.0, 55.0, Some(3)),
                (72.5, 82.5, Some(4))
            ],
        );
        assert_eq!(slice.track_area_span(4, 5), Some((82.5, 100.0)));
        assert_eq!(slice.taffy_gap(), 10.0);
        let fixed_track = |size| {
            taffy_layout::GridTemplateComponent::Single(taffy_layout::TrackSizingFunction {
                min: taffy_layout::MinTrackSizingFunction::length(size),
                max: taffy_layout::MaxTrackSizingFunction::length(size),
            })
        };
        assert_eq!(slice.taffy_tracks(), vec![fixed_track(17.5); 4]);

        let partial = axis
            .subgrid_slice(
                2,
                4,
                &css::SubgridLineNameList::default(),
                &ComputedStyle::initial(),
                GridAxis::Row,
                true,
            )
            .unwrap();
        assert_eq!(partial.line_offsets(), &[0.0, 27.5, 45.0]);
        assert_eq!(partial.track_area_span(1, 2), Some((0.0, 17.5)));
        assert_eq!(partial.track_area_span(2, 3), Some((27.5, 45.0)));
    }

    #[test]
    fn subgrid_slice_preserves_names_and_excludes_gutters_from_tracks() {
        let axis = ResolvedGridAxis {
            outer_start: 0.0,
            outer_end: 24.0,
            line_offsets: vec![0.0, 14.0, 24.0],
            track_starts: vec![0.0, 14.0],
            track_ends: vec![10.0, 24.0],
            line_names: vec![vec!["a".into()], vec!["b".into()], vec!["c".into()]],
        };
        let local = css::SubgridLineNameList {
            components: vec![css::SubgridLineNameComponent::LineNames(vec![
                "local".into(),
            ])],
        };
        let slice = axis
            .subgrid_slice(
                1,
                3,
                &local,
                &ComputedStyle::initial(),
                GridAxis::Column,
                true,
            )
            .unwrap();
        assert_eq!(slice.track_count(), 2);
        assert_eq!(slice.taffy_gap(), 4.0);
        assert_eq!(slice.line_names()[0], vec!["a", "local"]);
        assert_eq!(slice.line_names()[1], vec!["b"]);
        assert_eq!(slice.line_offsets(), &[0.0, 14.0, 24.0]);
    }

    #[test]
    fn automatic_grid_lanes_subgrid_keeps_local_names_but_not_parent_names() {
        let axis = ResolvedGridAxis {
            outer_start: 0.0,
            outer_end: 24.0,
            line_offsets: vec![0.0, 14.0, 24.0],
            track_starts: vec![0.0, 14.0],
            track_ends: vec![10.0, 24.0],
            line_names: vec![
                vec!["parent-start".into()],
                vec![],
                vec!["parent-end".into()],
            ],
        };
        let local = css::SubgridLineNameList {
            components: vec![
                css::SubgridLineNameComponent::LineNames(vec!["local-start".into()]),
                css::SubgridLineNameComponent::LineNames(vec!["local-end".into()]),
            ],
        };
        let slice = axis
            .subgrid_slice(
                1,
                3,
                &local,
                &ComputedStyle::initial(),
                GridAxis::Column,
                false,
            )
            .unwrap();
        assert!(
            slice
                .line_names()
                .iter()
                .flatten()
                .any(|name| name == "local-start")
        );
        assert!(
            slice
                .line_names()
                .iter()
                .flatten()
                .any(|name| name == "local-end")
        );
        assert!(
            slice
                .line_names()
                .iter()
                .flatten()
                .all(|name| !name.starts_with("parent-"))
        );
    }

    #[test]
    fn named_subgrid_placement_resolves_inside_the_inherited_explicit_grid() {
        let axis = ResolvedSubgridAxis {
            line_offsets: vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0],
            track_starts: vec![0.0, 20.0, 40.0, 60.0, 80.0],
            track_ends: vec![20.0, 40.0, 60.0, 80.0, 100.0],
            gutter_sizes: vec![0.0; 4],
            line_names: vec![vec!["y".into()]; 6],
            physical_line_names: vec![vec!["y".into()]; 6],
            logical_to_physical_line: (0..=5).collect(),
        };
        let start = css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: "y".into(),
            occurrence: std::num::NonZeroI32::new(3),
        });
        assert_eq!(
            axis.clamped_taffy_line(&start, &css::GridPlacement::Auto),
            taffy_layout::Line {
                start: taffy_layout::line(3),
                end: taffy_layout::line(4),
            }
        );
    }

    #[test]
    fn automatic_and_named_span_placement_stays_inside_inherited_lines() {
        let axis = ResolvedSubgridAxis {
            line_offsets: vec![0.0; 6],
            track_starts: vec![0.0; 5],
            track_ends: vec![0.0; 5],
            gutter_sizes: vec![0.0; 4],
            line_names: vec![
                vec![],
                vec!["y".into()],
                vec![],
                vec!["y".into()],
                vec![],
                vec![],
            ],
            physical_line_names: vec![
                vec![],
                vec!["y".into()],
                vec![],
                vec!["y".into()],
                vec![],
                vec![],
            ],
            logical_to_physical_line: (0..=5).collect(),
        };
        let auto = axis.resolved_range(&css::GridPlacement::Auto, &css::GridPlacement::Auto, 99);
        assert_eq!(auto, ResolvedSubgridPlacement { start: 5, end: 6 });
        let named_span = axis.resolved_range(
            &css::GridPlacement::Line(css::GridLinePlacement::Number(
                std::num::NonZeroI32::new(1).unwrap(),
            )),
            &css::GridPlacement::Span(css::GridSpanPlacement::Named {
                name: "y".into(),
                count: std::num::NonZeroU16::new(2),
            }),
            1,
        );
        assert_eq!(named_span, ResolvedSubgridPlacement { start: 1, end: 4 });
    }

    #[test]
    fn missing_named_lines_resolve_hypothetically_before_subgrid_clamping() {
        let axis = ResolvedSubgridAxis {
            line_offsets: vec![0.0; 5],
            track_starts: vec![0.0; 4],
            track_ends: vec![0.0; 4],
            gutter_sizes: vec![0.0; 3],
            line_names: vec![vec![], vec![], vec![], vec![], vec!["x".into()]],
            physical_line_names: vec![vec![], vec![], vec![], vec![], vec!["x".into()]],
            logical_to_physical_line: (0..=4).collect(),
        };
        let before_last_x = css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: "x".into(),
            occurrence: std::num::NonZeroI32::new(-2),
        });
        let last_x = css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: "x".into(),
            occurrence: std::num::NonZeroI32::new(-1),
        });
        assert_eq!(
            axis.resolved_range(&before_last_x, &last_x, 1),
            ResolvedSubgridPlacement { start: 1, end: 5 }
        );

        let after_last_x = css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: "x".into(),
            occurrence: std::num::NonZeroI32::new(2),
        });
        assert_eq!(
            axis.resolved_range(&after_last_x, &css::GridPlacement::Auto, 1),
            ResolvedSubgridPlacement { start: 4, end: 5 }
        );
    }

    #[test]
    fn reversed_subgrid_maps_logical_names_to_physical_tracks() {
        let axis = ResolvedGridAxis {
            outer_start: 0.0,
            outer_end: 20.0,
            line_offsets: vec![0.0, 10.0, 20.0],
            track_starts: vec![0.0, 10.0],
            track_ends: vec![10.0, 20.0],
            line_names: vec![vec!["a".into()], vec![], vec!["b".into()]],
        };
        let mut rtl = ComputedStyle::initial();
        rtl.direction = Direction::Rtl;
        let slice = axis
            .subgrid_slice(
                1,
                3,
                &css::SubgridLineNameList::default(),
                &rtl,
                GridAxis::Column,
                true,
            )
            .unwrap();
        assert_eq!(slice.line_names()[0], vec!["b"]);
        assert_eq!(slice.physical_line_names()[0], vec!["a"]);
        let start = css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: "b".into(),
            occurrence: std::num::NonZeroI32::new(1),
        });
        assert_eq!(
            slice.resolved_range(&start, &css::GridPlacement::Auto, 1),
            ResolvedSubgridPlacement { start: 2, end: 3 }
        );
    }
}
