use super::*;

/// Final geometry of one explicit grid axis.
///
/// A subgrid borrows a contiguous range of these lines instead of creating
/// tracks of its own.  Track starts and ends are kept separately because a
/// line offset includes the following gutter whereas a grid area does not.
/// <https://drafts.csswg.org/css-grid-2/#subgrids>
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResolvedGridAxis {
    line_offsets: Vec<f32>,
    track_starts: Vec<f32>,
    track_ends: Vec<f32>,
    line_names: Vec<css::GridLineNames>,
}

impl ResolvedGridAxis {
    fn from_parent_layout(
        parent_tracks: &css::GridTrackList,
        parent_offsets: &[f32],
        parent_gutters: &[GapDecorationGutter],
        parent_style: &ComputedStyle,
        axis: GridAxis,
    ) -> Self {
        let line_count = parent_offsets.len();
        let mut line_names =
            resolved_parent_line_names(parent_tracks, parent_style, axis, line_count);
        line_names.resize_with(line_count, Vec::new);
        line_names.truncate(line_count);

        let track_count = line_count.saturating_sub(1);
        let mut track_starts = Vec::with_capacity(track_count);
        let mut track_ends = Vec::with_capacity(track_count);
        for index in 0..track_count {
            let start = if index == 0 {
                parent_offsets[index]
            } else {
                parent_gutters
                    .get(index - 1)
                    .map(|gutter| gutter.span.end)
                    .unwrap_or(parent_offsets[index])
            };
            track_starts.push(start);
            track_ends.push(parent_offsets[index + 1].max(start));
        }
        Self {
            line_offsets: parent_offsets.to_vec(),
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
    ) -> Option<ResolvedSubgridAxis> {
        let start = usize::from(start_line.checked_sub(1)?);
        let end = usize::from(end_line.checked_sub(1)?);
        if start >= end || end >= self.line_offsets.len() {
            return None;
        }
        let track_count = end - start;
        let mut line_names = self.line_names.get(start..=end)?.to_vec();
        for (inherited, local) in line_names
            .iter_mut()
            .zip(local_names.expand_to_line_count(track_count + 1))
        {
            inherited.extend(local);
        }
        Some(ResolvedSubgridAxis {
            line_offsets: self.line_offsets[start..=end]
                .iter()
                .map(|offset| *offset - self.line_offsets[start])
                .collect(),
            track_starts: self.track_starts[start..end]
                .iter()
                .map(|offset| *offset - self.line_offsets[start])
                .collect(),
            track_ends: self.track_ends[start..end]
                .iter()
                .map(|offset| *offset - self.line_offsets[start])
                .collect(),
            gutter_sizes: (start + 1..end)
                .map(|index| (self.track_starts[index] - self.track_ends[index - 1]).max(0.0))
                .collect(),
            line_names,
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
    line_names: Vec<css::GridLineNames>,
}

impl ResolvedSubgridAxis {
    pub(super) fn track_count(&self) -> usize {
        self.track_starts.len()
    }

    pub(super) fn line_names(&self) -> &[css::GridLineNames] {
        &self.line_names
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
        let line = |placement: &css::GridPlacement| {
            grid_line_index(placement, &self.line_names).map(|line| line.clamp(1, last_line))
        };
        let span = |placement: &css::GridPlacement| match placement {
            css::GridPlacement::Span(span) => i32::from(span.count().unwrap_or(1)).max(1),
            _ => 1,
        };
        let named_span_end = |start: i32, placement: &css::GridPlacement| {
            let css::GridPlacement::Span(span) = placement else {
                return None;
            };
            let name = span.name()?;
            let target = usize::from(span.count().unwrap_or(1));
            self.line_names
                .iter()
                .enumerate()
                .skip(usize::try_from(start).ok()?)
                .filter(|(_, names)| names.iter().any(|candidate| candidate == name))
                .nth(target.saturating_sub(1))
                .and_then(|(index, _)| i32::try_from(index + 1).ok())
        };
        let named_span_start = |end: i32, placement: &css::GridPlacement| {
            let css::GridPlacement::Span(span) = placement else {
                return None;
            };
            let name = span.name()?;
            let target = usize::from(span.count().unwrap_or(1));
            self.line_names
                .iter()
                .enumerate()
                .take(usize::try_from(end.saturating_sub(1)).ok()?)
                .rev()
                .filter(|(_, names)| names.iter().any(|candidate| candidate == name))
                .nth(target.saturating_sub(1))
                .and_then(|(index, _)| i32::try_from(index + 1).ok())
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
        start = start.clamp(1, last_line.saturating_sub(1).max(1));
        end = end.clamp(start.saturating_add(1).min(last_line), last_line);
        if end <= start {
            start = start.saturating_sub(1).max(1);
            end = (start + 1).min(last_line);
        }
        ResolvedSubgridPlacement { start, end }
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
        debug_assert_eq!(self.line_names.len(), self.track_count() + 1);
        let last_line = i32::try_from(self.track_count() + 1).unwrap_or(i32::from(i16::MAX));
        let resolve = |placement: &css::GridPlacement| match placement {
            css::GridPlacement::Line(line) => grid_line_index(placement, &self.line_names)
                .unwrap_or_else(|| {
                    if line.index().unwrap_or(1) < 0 {
                        1
                    } else {
                        last_line
                    }
                })
                .clamp(1, last_line),
            css::GridPlacement::Auto | css::GridPlacement::Span(_) => 0,
        };
        let start_line = resolve(start);
        let end_line = resolve(end);
        let span = |placement: &css::GridPlacement| match placement {
            css::GridPlacement::Span(span) if span.name().is_none() => {
                i32::from(span.count().unwrap_or(1)).max(1)
            }
            _ => 1,
        };
        let (start_line, end_line) = match (start_line, end_line) {
            (start_line, end_line) if start_line > 0 && end_line > 0 => (start_line, end_line),
            (start_line, _) if start_line > 0 => (
                start_line,
                start_line.saturating_add(span(end)).clamp(1, last_line),
            ),
            (_, end_line) if end_line > 0 => (
                end_line.saturating_sub(span(start)).clamp(1, last_line),
                end_line,
            ),
            _ => return taffy_grid_line(start, end),
        };
        let (start_line, end_line) = if start_line == end_line {
            if end_line < last_line {
                (start_line, end_line + 1)
            } else {
                (start_line.saturating_sub(1).max(1), end_line)
            }
        } else {
            (start_line.min(end_line), start_line.max(end_line))
        };
        taffy_layout::Line {
            start: taffy_layout::line(i16::try_from(start_line).unwrap_or(i16::MAX)),
            end: taffy_layout::line(i16::try_from(end_line).unwrap_or(i16::MAX)),
        }
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
    ) -> Option<Self> {
        let columns = match &child_style.grid_template_columns {
            css::GridTrackList::Subgrid { line_names } => ResolvedGridAxis::from_parent_layout(
                &parent_style.grid_template_columns,
                &parent_layout.column_line_offsets,
                &parent_layout.gap_gutters.columns,
                parent_style,
                GridAxis::Column,
            )
            .subgrid_slice(area.column_start, area.column_end, line_names),
            _ => None,
        };
        let rows = match &child_style.grid_template_rows {
            css::GridTrackList::Subgrid { line_names } => ResolvedGridAxis::from_parent_layout(
                &parent_style.grid_template_rows,
                &parent_layout.row_line_offsets,
                &parent_layout.gap_gutters.rows,
                parent_style,
                GridAxis::Row,
            )
            .subgrid_slice(area.row_start, area.row_end, line_names),
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

fn resolved_parent_line_names(
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

impl<'a> LayoutBuilder<'a> {
    pub(super) fn with_resolved_subgrid_context<R>(
        &mut self,
        context: ResolvedSubgridContext,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.pending_subgrid_contexts.push(Some(context));
        let result = callback(self);
        let context = self.pending_subgrid_contexts.pop();
        debug_assert!(matches!(context, Some(None)));
        result
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
    fn subgrid_slice_preserves_names_and_excludes_gutters_from_tracks() {
        let axis = ResolvedGridAxis {
            line_offsets: vec![0.0, 12.0, 24.0],
            track_starts: vec![0.0, 14.0],
            track_ends: vec![10.0, 24.0],
            line_names: vec![vec!["a".into()], vec!["b".into()], vec!["c".into()]],
        };
        let local = css::SubgridLineNameList {
            components: vec![css::SubgridLineNameComponent::LineNames(vec![
                "local".into(),
            ])],
        };
        let slice = axis.subgrid_slice(1, 3, &local).unwrap();
        assert_eq!(slice.track_count(), 2);
        assert_eq!(slice.taffy_gap(), 4.0);
        assert_eq!(slice.line_names()[0], vec!["a", "local"]);
        assert_eq!(slice.line_names()[1], vec!["b"]);
        assert_eq!(slice.line_offsets(), &[0.0, 12.0, 24.0]);
    }

    #[test]
    fn named_subgrid_placement_resolves_inside_the_inherited_explicit_grid() {
        let axis = ResolvedSubgridAxis {
            line_offsets: vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0],
            track_starts: vec![0.0, 20.0, 40.0, 60.0, 80.0],
            track_ends: vec![20.0, 40.0, 60.0, 80.0, 100.0],
            gutter_sizes: vec![0.0; 4],
            line_names: vec![vec!["y".into()]; 6],
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
}
