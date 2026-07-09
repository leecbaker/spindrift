use super::*;

const GRID_FRAGMENT_EPSILON: f32 = 0.01;

/// Committed fragment slices for a grid container.
///
/// CSS Fragmentation fragments grid containers across fragmentainers and
/// prefers breaks between grid rows before breaking inside a row-spanning
/// band. Grid layout still owns row geometry and item replay, but this plan
/// records the active fragmentainer kind and source block ranges that later
/// painting and side-effect replay must consume instead of rediscovering breaks
/// from cursor state:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules> and
/// <https://www.w3.org/TR/css-grid-1/#pagination>.
#[derive(Debug, Clone)]
pub(in crate::layout::grid) struct GridFragmentPlan {
    fragmentainer_kind: FragmentainerKind,
    slices: Vec<GridFragmentSlice>,
    starts_after_fragmentainer_break: bool,
}

/// One source-range slice of a fragmented grid container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentSlice {
    pub(in crate::layout::grid) source_block_start: f32,
    pub(in crate::layout::grid) source_block_end: f32,
    pub(in crate::layout::grid) break_after: GridFragmentBreak,
}

/// A possible grid row boundary break.
///
/// CSS Grid fragmentation prefers breaks between rows, while CSS Break
/// resolves forced and avoided breaks at class A opportunities around grid
/// items. Grid layout provides the resolved source offset; the fragmentation
/// planner consumes the target-aware break metadata:
/// <https://www.w3.org/TR/css-grid-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridRowBreakBoundary {
    pub(in crate::layout::grid) source_block_offset: f32,
    pub(in crate::layout::grid) break_before: PageBreak,
    pub(in crate::layout::grid) break_after: PageBreak,
    pub(in crate::layout::grid) break_inside_avoid: bool,
}

/// One planned fragment of a grid container.
///
/// CSS Fragmentation commits the source slice and any target fragmentainer
/// transition before painting a fragment. Grid replay uses this record to avoid
/// deriving placement from the builder cursor after the fact:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/css-grid-1/#pagination>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentRecord {
    pub(in crate::layout::grid) fragmentainer_offset: usize,
    pub(in crate::layout::grid) slice: GridFragmentSlice,
    pub(in crate::layout::grid) transition_before_fragment: Option<GridFragmentTransition>,
}

/// Fragment-local cursor for one committed grid fragment.
///
/// CSS Fragmentation lays a continuation into a new fragmentainer while the
/// source box keeps its original coordinate system. The cursor binds the new
/// fragment-local grid content top to the source block offset selected by the
/// committed fragment record:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentCursor {
    pub(in crate::layout::grid) content_top: f32,
    pub(in crate::layout::grid) block_offset: f32,
}

/// A grid item clipped to one committed grid fragment slice.
#[derive(Debug, Clone)]
pub(in crate::layout::grid) struct GridItemFragment {
    pub(in crate::layout::grid) item_index: usize,
    pub(in crate::layout::grid) original: GridItemLayout,
    pub(in crate::layout::grid) visible: GridItemLayout,
    pub(in crate::layout::grid) content_slice: GridFragmentItemContentSlice,
    pub(in crate::layout::grid) metadata: FragmentPageMetadata,
}

/// Source-block range of a grid item visible in one grid fragment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentItemContentSlice {
    pub(in crate::layout::grid) block_start: f32,
    pub(in crate::layout::grid) block_end: f32,
}

/// The break mode used after a grid slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::grid) enum GridFragmentBreak {
    None,
    RowBoundary,
    ForcedRowBoundary,
    SlicedRowBand,
}

/// Why a grid container fragment starts at this source offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::grid) enum GridFragmentTransitionReason {
    InitialOverflow,
    SliceContinuation,
}

/// Committed transition before one grid fragment.
///
/// The transition owns the target fragmentainer kind and next source block
/// offset. The replay layer can then apply the appropriate target transition
/// before constructing fragment-local item fragments:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentTransition {
    pub(in crate::layout::grid) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::grid) reason: GridFragmentTransitionReason,
    pub(in crate::layout::grid) next_block_offset: f32,
}

impl GridFragmentPlan {
    /// Build one unsplit grid fragment for a monolithic/clipped grid box.
    ///
    /// A definite overflow-clipping grid establishes one principal fragment;
    /// overflowing item content is laid out for painting and clipped by that
    /// fragment instead of generating later page fragments.
    /// <https://www.w3.org/TR/css-overflow-3/#valdef-overflow-clip>
    pub(in crate::layout::grid) fn unfragmented(
        fragmentainer_kind: FragmentainerKind,
        content_block_size: f32,
    ) -> Self {
        let content_block_size = content_block_size.max(0.0);
        Self {
            fragmentainer_kind,
            slices: (content_block_size > GRID_FRAGMENT_EPSILON)
                .then_some(GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: content_block_size,
                    break_after: GridFragmentBreak::None,
                })
                .into_iter()
                .collect(),
            starts_after_fragmentainer_break: false,
        }
    }

    #[cfg(test)]
    pub(in crate::layout::grid) fn from_row_boundaries(
        fragmentainer_kind: FragmentainerKind,
        current_fragmentainer: Fragmentainer,
        content_block_size: f32,
        row_line_offsets: &[f32],
    ) -> Self {
        Self::from_break_boundaries(
            fragmentainer_kind,
            current_fragmentainer,
            content_block_size,
            &GridRowBreakBoundary::neutral_boundaries(row_line_offsets),
        )
    }

    pub(in crate::layout::grid) fn from_grid_item_boundaries(
        fragmentainer_kind: FragmentainerKind,
        current_fragmentainer: Fragmentainer,
        content_block_size: f32,
        row_line_offsets: &[f32],
        items: &[GridItemLayout],
        children: &[GridChild<'_>],
    ) -> Self {
        Self::from_break_boundaries(
            fragmentainer_kind,
            current_fragmentainer,
            content_block_size,
            &GridRowBreakBoundary::from_grid_items(
                fragmentainer_kind,
                row_line_offsets,
                items,
                children,
            ),
        )
    }

    fn from_break_boundaries(
        fragmentainer_kind: FragmentainerKind,
        current_fragmentainer: Fragmentainer,
        content_block_size: f32,
        row_boundaries: &[GridRowBreakBoundary],
    ) -> Self {
        let content_block_size = content_block_size.max(0.0);
        if content_block_size <= GRID_FRAGMENT_EPSILON {
            return Self {
                fragmentainer_kind,
                slices: Vec::new(),
                starts_after_fragmentainer_break: false,
            };
        }

        let mut slices = Vec::new();
        let mut source_block_start = 0.0;
        let mut available_block_end =
            current_fragmentainer.available_block_end_from(source_block_start);
        let empty_fragmentainer_block_size = current_fragmentainer.fragmentainer_block_size();
        let mut starts_after_fragmentainer_break = false;
        let break_opportunities = row_boundaries
            .iter()
            .cloned()
            .map(GridRowBreakBoundary::break_opportunity)
            .collect::<Vec<_>>();

        while source_block_start < content_block_size - GRID_FRAGMENT_EPSILON {
            if let Some(row_boundary) =
                FragmentBreakOpportunity::first_forced_in(FragmentBreakOpportunitySearch {
                    fragmentainer_kind,
                    opportunities: &break_opportunities,
                    source_block_start,
                    available_block_end,
                    content_block_end: content_block_size,
                })
            {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: row_boundary.source_block_offset,
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                });
                source_block_start = row_boundary.source_block_offset;
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                continue;
            }

            if content_block_size <= available_block_end + GRID_FRAGMENT_EPSILON {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: content_block_size,
                    break_after: GridFragmentBreak::None,
                });
                break;
            }

            if let Some(row_boundary) =
                FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(
                    FragmentBreakOpportunitySearch {
                        fragmentainer_kind,
                        opportunities: &break_opportunities,
                        source_block_start,
                        available_block_end,
                        content_block_end: content_block_size,
                    },
                )
            {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: row_boundary.source_block_offset,
                    break_after: GridFragmentBreak::RowBoundary,
                });
                source_block_start = row_boundary.source_block_offset;
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                continue;
            }

            let slice = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
                break_is_applicable: true,
                source_is_oversized: true,
                source_block_end: content_block_size,
                slice_start: source_block_start,
                available_block_end,
            });
            if slice.advance_before_slice {
                starts_after_fragmentainer_break |= slices.is_empty();
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                if empty_fragmentainer_block_size <= GRID_FRAGMENT_EPSILON {
                    slices.push(GridFragmentSlice {
                        source_block_start,
                        source_block_end: content_block_size,
                        break_after: GridFragmentBreak::None,
                    });
                    break;
                }
                continue;
            }

            slices.push(GridFragmentSlice {
                source_block_start: slice.slice_start,
                source_block_end: slice.slice_end,
                break_after: GridFragmentBreak::SlicedRowBand,
            });
            source_block_start = slice.slice_end;
            available_block_end = source_block_start + empty_fragmentainer_block_size;
        }

        Self {
            fragmentainer_kind,
            slices,
            starts_after_fragmentainer_break,
        }
    }

    pub(in crate::layout::grid) fn slices(&self) -> &[GridFragmentSlice] {
        &self.slices
    }

    pub(in crate::layout::grid) fn fragment_records(&self) -> Vec<GridFragmentRecord> {
        self.slices
            .iter()
            .cloned()
            .enumerate()
            .map(|(slice_index, slice)| {
                let fragmentainer_offset =
                    slice_index + usize::from(self.starts_after_fragmentainer_break);
                let transition_before_fragment = if slice_index == 0 {
                    self.starts_after_fragmentainer_break.then(|| {
                        GridFragmentTransition::initial_overflow(
                            self.fragmentainer_kind,
                            slice.source_block_start,
                        )
                    })
                } else {
                    Some(GridFragmentTransition::slice_continuation(
                        self.fragmentainer_kind,
                        slice.source_block_start,
                    ))
                };
                GridFragmentRecord {
                    fragmentainer_offset,
                    slice,
                    transition_before_fragment,
                }
            })
            .collect()
    }

    pub(in crate::layout::grid) fn fragment_record_for_offset(
        &self,
        fragmentainer_offset: usize,
    ) -> Option<GridFragmentRecord> {
        self.fragment_records()
            .into_iter()
            .find(|fragment| fragment.fragmentainer_offset == fragmentainer_offset)
    }

    pub(in crate::layout::grid) fn starts_after_fragmentainer_break(&self) -> bool {
        self.starts_after_fragmentainer_break
    }

    pub(in crate::layout::grid) fn requires_multiple_fragments(&self) -> bool {
        self.starts_after_fragmentainer_break || self.slices.len() > 1
    }
}

impl GridFragmentRecord {
    pub(in crate::layout::grid) fn cursor(self, content_top: f32) -> GridFragmentCursor {
        GridFragmentCursor::new(content_top, self.slice.source_block_start)
    }

    pub(in crate::layout::grid) fn item_fragments(
        self,
        items: &[GridItemLayout],
    ) -> Vec<GridItemFragment> {
        self.slice.item_fragments(items)
    }

    pub(in crate::layout::grid) fn paint_clip(
        self,
        outer_x: f32,
        outer_width: f32,
        cursor: GridFragmentCursor,
    ) -> PaintClip {
        cursor.slice_paint_clip(self.slice, outer_x, outer_width)
    }

    pub(in crate::layout::grid) fn source_range(self) -> (f32, f32) {
        (self.slice.source_block_start, self.slice.source_block_end)
    }
}

impl GridFragmentTransition {
    pub(in crate::layout::grid) fn initial_overflow(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: f32,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: GridFragmentTransitionReason::InitialOverflow,
            next_block_offset,
        }
    }

    pub(in crate::layout::grid) fn slice_continuation(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: f32,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: GridFragmentTransitionReason::SliceContinuation,
            next_block_offset,
        }
    }

    /// Construct the fragment-local grid cursor after this committed transition.
    ///
    /// CSS Fragmentation commits the next source block offset before content
    /// is laid out in the new fragmentainer:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout::grid) fn cursor_after_fragmentainer_advance(
        self,
        content_top: f32,
    ) -> GridFragmentCursor {
        GridFragmentCursor::new(content_top, self.next_block_offset)
    }
}

impl GridFragmentCursor {
    pub(in crate::layout::grid) fn new(content_top: f32, block_offset: f32) -> Self {
        Self {
            content_top,
            block_offset,
        }
    }

    pub(in crate::layout::grid) fn source_block_y(self, source_block_offset: f32) -> f32 {
        self.content_top - (source_block_offset - self.block_offset)
    }

    pub(in crate::layout::grid) fn slice_paint_clip(
        self,
        slice: GridFragmentSlice,
        outer_x: f32,
        outer_width: f32,
    ) -> PaintClip {
        let slice_height = (slice.source_block_end - slice.source_block_start).max(0.0);
        PaintClip::from_paint_rect(paint_space_rect(
            outer_x,
            self.source_block_y(slice.source_block_end),
            outer_width,
            slice_height,
        ))
    }
}

impl GridItemFragment {
    /// Return whether this grid item fragment crosses the source item boundary.
    ///
    /// CSS Fragmentation preserves the source item layout and clips the
    /// fragmentainer slice for continuations:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-grid-1/#pagination>.
    pub(in crate::layout::grid) fn requires_split_replay(&self) -> bool {
        self.content_slice.block_start > GRID_FRAGMENT_EPSILON
            || self.content_slice.block_end
                < self.original.height().max(0.0) - GRID_FRAGMENT_EPSILON
    }
}

impl GridFragmentSlice {
    fn item_fragments(self, items: &[GridItemLayout]) -> Vec<GridItemFragment> {
        items
            .iter()
            .enumerate()
            .filter_map(|(item_index, item)| self.item_fragment(item_index, item))
            .collect()
    }

    fn item_fragment(self, item_index: usize, item: &GridItemLayout) -> Option<GridItemFragment> {
        let item_block_start = item.y();
        let item_block_end = item.y() + item.height().max(0.0);
        let slice_block_start = item_block_start.max(self.source_block_start);
        let slice_block_end = item_block_end.min(self.source_block_end);
        if slice_block_end <= slice_block_start + GRID_FRAGMENT_EPSILON {
            return None;
        }

        let visible = item.with_block_slice(slice_block_start, slice_block_end);
        Some(GridItemFragment {
            item_index,
            original: item.clone(),
            visible,
            content_slice: GridFragmentItemContentSlice {
                block_start: (slice_block_start - item_block_start).max(0.0),
                block_end: (slice_block_end - item_block_start).min(item.height().max(0.0)),
            },
            metadata: FragmentPageMetadata::empty(0),
        })
    }
}

impl GridRowBreakBoundary {
    fn neutral_boundaries(row_line_offsets: &[f32]) -> Vec<Self> {
        row_line_offsets
            .iter()
            .cloned()
            .map(Self::neutral)
            .collect::<Vec<_>>()
    }

    pub(in crate::layout::grid) fn from_grid_items(
        fragmentainer_kind: FragmentainerKind,
        row_line_offsets: &[f32],
        items: &[GridItemLayout],
        children: &[GridChild<'_>],
    ) -> Vec<Self> {
        let mut boundaries = Self::neutral_boundaries(row_line_offsets);
        let line_base = grid_item_row_line_base(items);
        for (item, child) in items.iter().zip(children) {
            let Some((row_start, row_end)) = grid_item_row_line_range(item, line_base) else {
                continue;
            };
            if let Some(boundary) = boundaries.get_mut(row_start) {
                boundary.break_before = fragmentainer_kind
                    .combine_break(boundary.break_before, child.style.break_before);
            }
            if let Some(boundary) = boundaries.get_mut(row_end) {
                boundary.break_after =
                    fragmentainer_kind.combine_break(boundary.break_after, child.style.break_after);
            }
            if fragmentainer_kind.avoids_break_inside(&child.style) {
                for boundary_index in row_start + 1..row_end {
                    if let Some(boundary) = boundaries.get_mut(boundary_index) {
                        boundary.break_inside_avoid = true;
                    }
                }
            }
        }
        boundaries
    }

    fn neutral(source_block_offset: f32) -> Self {
        Self {
            source_block_offset,
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
        }
    }

    fn break_opportunity(self) -> FragmentBreakOpportunity {
        FragmentBreakOpportunity {
            source_block_offset: self.source_block_offset,
            break_before: self.break_before,
            break_after: self.break_after,
            break_inside_avoid: self.break_inside_avoid,
        }
    }
}

fn grid_item_row_line_base(items: &[GridItemLayout]) -> u16 {
    items
        .iter()
        .filter_map(|item| item.area.map(|area| area.row_start))
        .min()
        .unwrap_or(0)
        .min(1)
}

fn grid_item_row_line_range(item: &GridItemLayout, line_base: u16) -> Option<(usize, usize)> {
    let area = item.area?;
    let start = area.row_start.checked_sub(line_base).map(usize::from)?;
    let end = area.row_end.checked_sub(line_base).map(usize::from)?;
    (end > start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fragment_plan_prefers_row_boundaries() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(200.0, 150.0),
            300.0,
            &[0.0, 80.0, 160.0, 300.0],
        );

        assert!(plan.requires_multiple_fragments());
        assert!(!plan.starts_after_fragmentainer_break());
        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 80.0,
                    break_after: GridFragmentBreak::RowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: 80.0,
                    source_block_end: 160.0,
                    break_after: GridFragmentBreak::RowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: 160.0,
                    source_block_end: 300.0,
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_commits_forced_row_boundary_even_when_grid_fits() {
        let mut boundaries = GridRowBreakBoundary::neutral_boundaries(&[0.0, 80.0, 160.0, 240.0]);
        boundaries[1].break_before = PageBreak::Page;
        let plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(300.0, 300.0),
            240.0,
            &boundaries,
        );

        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 80.0,
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: 80.0,
                    source_block_end: 240.0,
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_scopes_forced_boundaries_to_fragmentainer_kind() {
        let mut boundaries = GridRowBreakBoundary::neutral_boundaries(&[0.0, 80.0, 160.0, 240.0]);
        boundaries[1].break_before = PageBreak::Column;

        let page_plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(300.0, 300.0),
            240.0,
            &boundaries,
        );
        let column_plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Column,
            Fragmentainer::new(300.0, 300.0),
            240.0,
            &boundaries,
        );

        assert_eq!(
            page_plan.slices(),
            &[GridFragmentSlice {
                source_block_start: 0.0,
                source_block_end: 240.0,
                break_after: GridFragmentBreak::None,
            }]
        );
        assert_eq!(
            column_plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 80.0,
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: 80.0,
                    source_block_end: 240.0,
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_skips_avoidable_row_boundary_when_later_boundary_fits() {
        let mut boundaries =
            GridRowBreakBoundary::neutral_boundaries(&[0.0, 80.0, 140.0, 220.0, 300.0]);
        boundaries[1].break_inside_avoid = true;
        let plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(300.0, 150.0),
            300.0,
            &boundaries,
        );

        assert_eq!(
            plan.slices()[0],
            GridFragmentSlice {
                source_block_start: 0.0,
                source_block_end: 140.0,
                break_after: GridFragmentBreak::RowBoundary,
            }
        );
    }

    #[test]
    fn grid_fragment_plan_maps_fragments_to_fragmentainer_offsets() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(100.0, 0.0),
            180.0,
            &[0.0, 90.0, 180.0],
        );

        assert_eq!(plan.fragment_record_for_offset(0), None);
        assert_eq!(
            plan.fragment_record_for_offset(1),
            Some(GridFragmentRecord {
                fragmentainer_offset: 1,
                slice: GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 90.0,
                    break_after: GridFragmentBreak::RowBoundary,
                },
                transition_before_fragment: Some(GridFragmentTransition::initial_overflow(
                    FragmentainerKind::Page,
                    0.0,
                )),
            })
        );
        assert_eq!(
            plan.fragment_record_for_offset(2),
            Some(GridFragmentRecord {
                fragmentainer_offset: 2,
                slice: GridFragmentSlice {
                    source_block_start: 90.0,
                    source_block_end: 180.0,
                    break_after: GridFragmentBreak::None,
                },
                transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                    FragmentainerKind::Page,
                    90.0,
                ),),
            })
        );
    }

    #[test]
    fn grid_fragment_plan_commits_fragment_records_and_transitions() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(120.0, 100.0),
            260.0,
            &[0.0, 260.0],
        );

        assert_eq!(
            plan.fragment_records(),
            vec![
                GridFragmentRecord {
                    fragmentainer_offset: 0,
                    slice: GridFragmentSlice {
                        source_block_start: 0.0,
                        source_block_end: 100.0,
                        break_after: GridFragmentBreak::SlicedRowBand,
                    },
                    transition_before_fragment: None,
                },
                GridFragmentRecord {
                    fragmentainer_offset: 1,
                    slice: GridFragmentSlice {
                        source_block_start: 100.0,
                        source_block_end: 220.0,
                        break_after: GridFragmentBreak::SlicedRowBand,
                    },
                    transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                        FragmentainerKind::Page,
                        100.0,
                    ),),
                },
                GridFragmentRecord {
                    fragmentainer_offset: 2,
                    slice: GridFragmentSlice {
                        source_block_start: 220.0,
                        source_block_end: 260.0,
                        break_after: GridFragmentBreak::None,
                    },
                    transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                        FragmentainerKind::Page,
                        220.0,
                    ),),
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_commits_initial_overflow_transition() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(100.0, 0.0),
            60.0,
            &[0.0, 60.0],
        );

        assert_eq!(
            plan.fragment_record_for_offset(1),
            Some(GridFragmentRecord {
                fragmentainer_offset: 1,
                slice: GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 60.0,
                    break_after: GridFragmentBreak::None,
                },
                transition_before_fragment: Some(GridFragmentTransition::initial_overflow(
                    FragmentainerKind::Page,
                    0.0,
                )),
            })
        );
    }

    #[test]
    fn grid_fragment_transitions_preserve_fragmentainer_kind() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Column,
            Fragmentainer::new(100.0, 0.0),
            180.0,
            &[0.0, 90.0, 180.0],
        );

        let fragments = plan.fragment_records();

        assert_eq!(
            fragments[0].transition_before_fragment,
            Some(GridFragmentTransition::initial_overflow(
                FragmentainerKind::Column,
                0.0,
            ))
        );
        assert_eq!(
            fragments[1].transition_before_fragment,
            Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Column,
                90.0,
            ))
        );
    }

    #[test]
    fn grid_fragment_record_projects_slice_from_cursor() {
        let fragment_record = GridFragmentRecord {
            fragmentainer_offset: 1,
            slice: GridFragmentSlice {
                source_block_start: 25.0,
                source_block_end: 75.0,
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                25.0,
            )),
        };
        let cursor = fragment_record.cursor(300.0);
        let clip = fragment_record.paint_clip(10.0, 120.0, cursor);

        assert_eq!(clip.x(), 10.0);
        assert_eq!(clip.y(), 250.0);
        assert_eq!(clip.width(), 120.0);
        assert_eq!(clip.height(), 50.0);
        assert_eq!(fragment_record.source_range(), (25.0, 75.0));
    }

    #[test]
    fn grid_fragment_record_builds_visible_item_fragments() {
        let fragment_record = GridFragmentRecord {
            fragmentainer_offset: 1,
            slice: GridFragmentSlice {
                source_block_start: 50.0,
                source_block_end: 150.0,
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                50.0,
            )),
        };
        let items = [
            GridItemLayout::new(
                GridRect::new(GridPoint::new(0.0, 0.0), GridSize::new(40.0, 40.0)),
                None,
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(10.0, 25.0), GridSize::new(60.0, 100.0)),
                Some(GridItemArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                }),
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(20.0, 140.0), GridSize::new(50.0, 50.0)),
                None,
            ),
        ];

        let fragments = fragment_record.item_fragments(&items);

        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[0].item_index, 1);
        assert_eq!(fragments[0].metadata, FragmentPageMetadata::empty(0));
        assert_eq!(fragments[0].visible.y(), 50.0);
        assert_eq!(fragments[0].visible.height(), 75.0);
        assert_eq!(
            fragments[0].visible.page_top_rect(100.0, 300.0),
            PageTopRect::new(110.0, 250.0, 60.0, 75.0)
        );
        assert_eq!(
            fragments[0].content_slice,
            GridFragmentItemContentSlice {
                block_start: 25.0,
                block_end: 100.0,
            }
        );
        let area = fragments[0]
            .visible
            .area
            .expect("visible item should preserve grid area");
        assert_eq!(area.row_start, 1);
        assert_eq!(area.row_end, 3);
        assert_eq!(area.column_start, 1);
        assert_eq!(area.column_end, 2);
        assert_eq!(fragments[1].item_index, 2);
        assert_eq!(fragments[1].visible.y(), 140.0);
        assert_eq!(fragments[1].visible.height(), 10.0);
        assert_eq!(
            fragments[1].content_slice,
            GridFragmentItemContentSlice {
                block_start: 0.0,
                block_end: 10.0,
            }
        );

        assert!(
            fragments
                .iter()
                .any(GridItemFragment::requires_split_replay)
        );
    }

    #[test]
    fn grid_fragment_record_allows_whole_item_replay_at_row_boundaries() {
        let fragment_record = GridFragmentRecord {
            fragmentainer_offset: 1,
            slice: GridFragmentSlice {
                source_block_start: 40.0,
                source_block_end: 100.0,
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                40.0,
            )),
        };
        let items = [
            GridItemLayout::new(
                GridRect::new(GridPoint::new(0.0, 0.0), GridSize::new(40.0, 40.0)),
                None,
            ),
            GridItemLayout::new(
                GridRect::new(GridPoint::new(10.0, 40.0), GridSize::new(60.0, 60.0)),
                None,
            ),
        ];

        assert!(
            fragment_record
                .item_fragments(&items)
                .iter()
                .all(|fragment| !fragment.requires_split_replay())
        );
    }

    #[test]
    fn grid_fragment_plan_slices_oversized_row_band() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(120.0, 100.0),
            260.0,
            &[0.0, 260.0],
        );

        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: 0.0,
                    source_block_end: 100.0,
                    break_after: GridFragmentBreak::SlicedRowBand,
                },
                GridFragmentSlice {
                    source_block_start: 100.0,
                    source_block_end: 220.0,
                    break_after: GridFragmentBreak::SlicedRowBand,
                },
                GridFragmentSlice {
                    source_block_start: 220.0,
                    source_block_end: 260.0,
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_advances_when_current_fragmentainer_has_no_space() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            Fragmentainer::new(100.0, 0.0),
            60.0,
            &[0.0, 60.0],
        );

        assert!(plan.starts_after_fragmentainer_break());
        assert_eq!(
            plan.slices(),
            &[GridFragmentSlice {
                source_block_start: 0.0,
                source_block_end: 60.0,
                break_after: GridFragmentBreak::None,
            }]
        );
    }
}
