use super::*;
use crate::layout::assets::FragmentainerOrdinal;

pub(super) const GRID_FRAGMENT_EPSILON: f32 = 0.01;

/// A physical block-axis offset within the unfragmented grid container.
///
/// Grid fragmentation replays source layout into page fragmentainers, so this
/// coordinate must not be exchanged with page positions or Flex source offsets.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub(in crate::layout::grid) struct GridFragmentBlockOffset(f32);

impl GridFragmentBlockOffset {
    pub(in crate::layout::grid) const fn new(points: f32) -> Self {
        Self(points)
    }

    pub(in crate::layout::grid) const fn points(self) -> f32 {
        self.0
    }

    /// Project this physical source offset into a page-layout displacement.
    pub(in crate::layout::grid) fn layout_length(self) -> LayoutLength {
        layout_pt(self.0)
    }
}

/// A non-negative physical block-axis extent within a grid fragment.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::layout::grid) struct GridFragmentBlockSize(f32);

impl GridFragmentBlockSize {
    pub(in crate::layout::grid) fn new(points: f32) -> Self {
        Self(points.max(0.0))
    }

    pub(in crate::layout::grid) const fn points(self) -> f32 {
        self.0
    }

    /// Project this physical grid-source extent into page-layout length space.
    ///
    /// Grid source offsets stay distinct from page positions, but a committed
    /// fragment cursor advances through the same physical CSS length.
    pub(in crate::layout::grid) fn layout_length(self) -> LayoutLength {
        layout_pt(self.0)
    }
}

/// Usable grid-content capacity in the originating and continuation
/// fragmentainers.
///
/// Grid source slices advance through the container's content box, never
/// through cloned border or padding. The two capacities remain separate
/// because the initial fragment has already crossed its owned block-start
/// decoration while a continuation has not.
/// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::grid) struct GridFragmentContentCapacity {
    initial: GridFragmentBlockSize,
    continuation: GridFragmentBlockSize,
}

impl GridFragmentContentCapacity {
    pub(in crate::layout::grid) const fn new(
        initial: GridFragmentBlockSize,
        continuation: GridFragmentBlockSize,
    ) -> Self {
        Self {
            initial,
            continuation,
        }
    }
}

impl std::ops::Add<GridFragmentBlockSize> for GridFragmentBlockOffset {
    type Output = Self;

    fn add(self, size: GridFragmentBlockSize) -> Self {
        Self::new(self.0 + size.0)
    }
}

impl std::ops::Sub for GridFragmentBlockOffset {
    type Output = GridFragmentBlockSize;

    fn sub(self, other: Self) -> Self::Output {
        GridFragmentBlockSize::new(self.0 - other.0)
    }
}

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
    pub(in crate::layout::grid) source_block_start: GridFragmentBlockOffset,
    pub(in crate::layout::grid) source_block_end: GridFragmentBlockOffset,
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
    pub(in crate::layout::grid) source_block_offset: GridFragmentBlockOffset,
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
    pub(in crate::layout::grid) content_top: PageTopBlockPosition,
    pub(in crate::layout::grid) block_offset: GridFragmentBlockOffset,
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
    pub(in crate::layout::grid) block_start: GridFragmentBlockOffset,
    pub(in crate::layout::grid) block_end: GridFragmentBlockOffset,
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
    pub(in crate::layout::grid) next_block_offset: GridFragmentBlockOffset,
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
                    source_block_start: GridFragmentBlockOffset::new(0.0),
                    source_block_end: GridFragmentBlockOffset::new(content_block_size),
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
            GridFragmentContentCapacity::new(
                GridFragmentBlockSize::new(current_fragmentainer.available_block_size().points()),
                GridFragmentBlockSize::new(
                    current_fragmentainer.fragmentainer_block_size().points(),
                ),
            ),
            content_block_size,
            &GridRowBreakBoundary::neutral_boundaries(row_line_offsets),
        )
    }

    /// Build a plan from the content capacity that remains after the grid
    /// container's fragment decoration has reserved its owned edges.
    pub(in crate::layout::grid) fn from_grid_item_boundaries_with_content_capacity(
        fragmentainer_kind: FragmentainerKind,
        content_capacity: GridFragmentContentCapacity,
        content_block_size: f32,
        row_line_offsets: &[f32],
        items: &[GridItemLayout],
        children: &[GridChild<'_>],
    ) -> Self {
        Self::from_break_boundaries(
            fragmentainer_kind,
            content_capacity,
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
        content_capacity: GridFragmentContentCapacity,
        content_block_size: f32,
        row_boundaries: &[GridRowBreakBoundary],
    ) -> Self {
        let content_block_end = GridFragmentBlockOffset::new(content_block_size.max(0.0));
        if content_block_end.points() <= GRID_FRAGMENT_EPSILON {
            return Self {
                fragmentainer_kind,
                slices: Vec::new(),
                starts_after_fragmentainer_break: false,
            };
        }

        let mut slices = Vec::new();
        let mut source_block_start = GridFragmentBlockOffset::new(0.0);
        let mut available_block_end = source_block_start + content_capacity.initial;
        let empty_fragmentainer_block_size = content_capacity.continuation;
        let mut starts_after_fragmentainer_break = false;
        let break_opportunities = row_boundaries
            .iter()
            .cloned()
            .map(GridRowBreakBoundary::break_opportunity)
            .collect::<Vec<_>>();

        while source_block_start.points() < content_block_end.points() - GRID_FRAGMENT_EPSILON {
            if let Some(row_boundary) =
                FragmentBreakOpportunity::first_forced_in(FragmentBreakOpportunitySearch {
                    fragmentainer_kind,
                    opportunities: &break_opportunities,
                    source_block_start: source_block_start.points(),
                    available_block_end: available_block_end.points(),
                    content_block_end: content_block_end.points(),
                })
            {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: GridFragmentBlockOffset::new(
                        row_boundary.source_block_offset,
                    ),
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                });
                source_block_start = GridFragmentBlockOffset::new(row_boundary.source_block_offset);
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                continue;
            }

            if content_block_end.points() <= available_block_end.points() + GRID_FRAGMENT_EPSILON {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: content_block_end,
                    break_after: GridFragmentBreak::None,
                });
                break;
            }

            if let Some(row_boundary) =
                FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(
                    FragmentBreakOpportunitySearch {
                        fragmentainer_kind,
                        opportunities: &break_opportunities,
                        source_block_start: source_block_start.points(),
                        available_block_end: available_block_end.points(),
                        content_block_end: content_block_end.points(),
                    },
                )
                // A preferred row boundary may not manufacture a tiny
                // principal fragment immediately before a row that is itself
                // larger than a fresh fragmentainer. That would add an extra
                // cloned border/padding pair without avoiding an inside-row
                // break. In that case Grid must slice the row band at normal
                // fragmentainer capacity instead.
                // <https://www.w3.org/TR/css-grid-1/#pagination>
                // <https://www.w3.org/TR/css-break-3/#unforced-breaks>
                .filter(|row_boundary| {
                    let next_boundary = row_boundaries
                        .iter()
                        .map(|boundary| boundary.source_block_offset.points())
                        .filter(|&offset| {
                            offset > row_boundary.source_block_offset + GRID_FRAGMENT_EPSILON
                        })
                        .min_by(f32::total_cmp)
                        .unwrap_or(content_block_end.points());
                    next_boundary - row_boundary.source_block_offset
                        <= empty_fragmentainer_block_size.points() + GRID_FRAGMENT_EPSILON
                })
            {
                slices.push(GridFragmentSlice {
                    source_block_start,
                    source_block_end: GridFragmentBlockOffset::new(
                        row_boundary.source_block_offset,
                    ),
                    break_after: GridFragmentBreak::RowBoundary,
                });
                source_block_start = GridFragmentBlockOffset::new(row_boundary.source_block_offset);
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                continue;
            }

            let slice = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
                break_is_applicable: true,
                source_is_oversized: true,
                source_block_end: content_block_end.points(),
                slice_start: source_block_start.points(),
                available_block_end: available_block_end.points(),
            });
            if slice.advance_before_slice {
                starts_after_fragmentainer_break |= slices.is_empty();
                available_block_end = source_block_start + empty_fragmentainer_block_size;
                if empty_fragmentainer_block_size.points() <= GRID_FRAGMENT_EPSILON {
                    slices.push(GridFragmentSlice {
                        source_block_start,
                        source_block_end: content_block_end,
                        break_after: GridFragmentBreak::None,
                    });
                    break;
                }
                continue;
            }

            slices.push(GridFragmentSlice {
                source_block_start: GridFragmentBlockOffset::new(slice.slice_start),
                source_block_end: GridFragmentBlockOffset::new(slice.slice_end),
                break_after: GridFragmentBreak::SlicedRowBand,
            });
            source_block_start = GridFragmentBlockOffset::new(slice.slice_end);
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
    /// Materialize this committed source range as a principal container-box
    /// fragment. Grid replay owns the source slice; the caller supplies the
    /// page-local border box after resolving the destination cursor.
    pub(in crate::layout::grid) fn principal_box_fragment(
        self,
        destination_fragmentainer: FragmentainerOrdinal,
        border_box: PaintClip,
        decoration: FragmentDecoration,
    ) -> CommittedContainerFragment<GridFragmentSlice> {
        CommittedContainerFragment::principal(
            destination_fragmentainer,
            self.slice,
            border_box,
            decoration,
        )
    }

    pub(in crate::layout::grid) fn cursor(
        self,
        content_top: PageTopBlockPosition,
    ) -> GridFragmentCursor {
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
        border_box_inline_span: PageInlineSpan,
        cursor: GridFragmentCursor,
    ) -> PaintClip {
        cursor.slice_paint_clip(self.slice, border_box_inline_span)
    }

    pub(in crate::layout::grid) fn source_range(
        self,
    ) -> (GridFragmentBlockOffset, GridFragmentBlockOffset) {
        (self.slice.source_block_start, self.slice.source_block_end)
    }
}

impl GridFragmentTransition {
    pub(in crate::layout::grid) fn initial_overflow(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: GridFragmentBlockOffset,
    ) -> Self {
        Self {
            fragmentainer_kind,
            reason: GridFragmentTransitionReason::InitialOverflow,
            next_block_offset,
        }
    }

    pub(in crate::layout::grid) fn slice_continuation(
        fragmentainer_kind: FragmentainerKind,
        next_block_offset: GridFragmentBlockOffset,
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
        content_top: PageTopBlockPosition,
    ) -> GridFragmentCursor {
        GridFragmentCursor::new(content_top, self.next_block_offset)
    }
}

impl GridFragmentCursor {
    pub(in crate::layout::grid) fn new(
        content_top: PageTopBlockPosition,
        block_offset: GridFragmentBlockOffset,
    ) -> Self {
        Self {
            content_top,
            block_offset,
        }
    }

    pub(in crate::layout::grid) fn source_block_y(
        self,
        source_block_offset: GridFragmentBlockOffset,
    ) -> PageTopBlockPosition {
        self.content_top
            .toward_block_end((source_block_offset - self.block_offset).layout_length())
    }

    /// Return the page-top origin for grid-local rectangles in this fragment.
    pub(in crate::layout::grid) fn grid_container_origin(self, inline_x: f32) -> PageTopPoint {
        PageTopPoint::from_inline_x_and_block_position(
            inline_x,
            self.content_top
                .toward_block_start(self.block_offset.layout_length()),
        )
    }

    pub(in crate::layout::grid) fn slice_paint_clip(
        self,
        slice: GridFragmentSlice,
        border_box_inline_span: PageInlineSpan,
    ) -> PaintClip {
        let slice_height = (slice.source_block_end - slice.source_block_start).layout_length();
        PaintClip::from_paint_rect(paint_space_rect(
            border_box_inline_span.left_x(),
            self.source_block_y(slice.source_block_end).points(),
            border_box_inline_span.width(),
            slice_height.points(),
        ))
    }

    /// Project a content-source slice to its destination border box after
    /// reserving the fragment's owned decoration edges.
    ///
    /// The cursor is already at the destination content-box start. A cloned
    /// continuation therefore grows upward by its owned start edge and
    /// downward by its owned end edge, while source progress remains exactly
    /// the slice's content extent.
    /// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
    pub(in crate::layout::grid) fn decorated_paint_clip(
        self,
        slice: GridFragmentSlice,
        border_box_inline_span: PageInlineSpan,
        reservation: FragmentDecorationReservation,
    ) -> PaintClip {
        let content_height = (slice.source_block_end - slice.source_block_start)
            .layout_length()
            .points();
        let border_height =
            content_height + reservation.block_start().points() + reservation.block_end().points();
        let border_bottom =
            self.source_block_y(slice.source_block_end).points() - reservation.block_end().points();
        PaintClip::new(
            border_box_inline_span.left_x(),
            border_bottom,
            border_box_inline_span.width(),
            border_height,
        )
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
        self.content_slice.block_start.points() > GRID_FRAGMENT_EPSILON
            || self.content_slice.block_end.points()
                < self.original.fragmentation_source_height().max(0.0) - GRID_FRAGMENT_EPSILON
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
        // The grid track retains source placement geometry, but a cloned grid
        // item consumes a larger destination extent because each occupied
        // fragmentainer owns its border and padding. Intersect the committed
        // grid slice in destination space, then map only that interval back
        // to the item's continuous source content for replay.
        // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
        let item_block_end = item.y() + item.fragmentation_height().max(0.0);
        let slice_block_start = item_block_start.max(self.source_block_start.points());
        let slice_block_end = item_block_end.min(self.source_block_end.points());
        if slice_block_end <= slice_block_start + GRID_FRAGMENT_EPSILON {
            return None;
        }

        let visible = item.with_block_slice(slice_block_start, slice_block_end);
        let destination_slice = GridFragmentItemContentSlice {
            block_start: GridFragmentBlockOffset::new(
                (slice_block_start - item_block_start).max(0.0),
            ),
            block_end: GridFragmentBlockOffset::new(
                (slice_block_end - item_block_start).min(item.fragmentation_height().max(0.0)),
            ),
        };
        let content_slice = item
            .source_slice_for_destination_slice(destination_slice)
            .unwrap_or(destination_slice);
        Some(GridItemFragment {
            item_index,
            original: item.clone(),
            visible,
            content_slice,
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
            source_block_offset: GridFragmentBlockOffset::new(source_block_offset),
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
        }
    }

    fn break_opportunity(self) -> FragmentBreakOpportunity {
        FragmentBreakOpportunity {
            source_block_offset: self.source_block_offset.points(),
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

    fn fragmentainer(block_size: f32, available_size: f32) -> Fragmentainer {
        Fragmentainer::new(layout_pt(block_size), layout_pt(available_size))
    }

    const fn offset(points: f32) -> GridFragmentBlockOffset {
        GridFragmentBlockOffset::new(points)
    }

    fn content_capacity(initial: f32, continuation: f32) -> GridFragmentContentCapacity {
        GridFragmentContentCapacity::new(
            GridFragmentBlockSize::new(initial),
            GridFragmentBlockSize::new(continuation),
        )
    }

    #[test]
    fn grid_fragment_plan_prefers_row_boundaries() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(200.0, 150.0),
            300.0,
            &[0.0, 80.0, 160.0, 300.0],
        );

        assert!(plan.requires_multiple_fragments());
        assert!(!plan.starts_after_fragmentainer_break());
        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(80.0),
                    break_after: GridFragmentBreak::RowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: offset(80.0),
                    source_block_end: offset(160.0),
                    break_after: GridFragmentBreak::RowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: offset(160.0),
                    source_block_end: offset(300.0),
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_uses_typed_remaining_capacity_at_source_boundary() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(120.0, 30.0),
            100.0,
            &[0.0, 100.0],
        );

        assert_eq!(
            plan.slices()[0],
            GridFragmentSlice {
                source_block_start: offset(0.0),
                source_block_end: offset(30.0),
                break_after: GridFragmentBreak::SlicedRowBand,
            }
        );
    }

    #[test]
    fn grid_fragment_plan_commits_forced_row_boundary_even_when_grid_fits() {
        let mut boundaries = GridRowBreakBoundary::neutral_boundaries(&[0.0, 80.0, 160.0, 240.0]);
        boundaries[1].break_before = PageBreak::Page;
        let plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Page,
            content_capacity(300.0, 300.0),
            240.0,
            &boundaries,
        );

        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(80.0),
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: offset(80.0),
                    source_block_end: offset(240.0),
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
            content_capacity(300.0, 300.0),
            240.0,
            &boundaries,
        );
        let column_plan = GridFragmentPlan::from_break_boundaries(
            FragmentainerKind::Column,
            content_capacity(300.0, 300.0),
            240.0,
            &boundaries,
        );

        assert_eq!(
            page_plan.slices(),
            &[GridFragmentSlice {
                source_block_start: offset(0.0),
                source_block_end: offset(240.0),
                break_after: GridFragmentBreak::None,
            }]
        );
        assert_eq!(
            column_plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(80.0),
                    break_after: GridFragmentBreak::ForcedRowBoundary,
                },
                GridFragmentSlice {
                    source_block_start: offset(80.0),
                    source_block_end: offset(240.0),
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
            content_capacity(150.0, 300.0),
            300.0,
            &boundaries,
        );

        assert_eq!(
            plan.slices()[0],
            GridFragmentSlice {
                source_block_start: offset(0.0),
                source_block_end: offset(140.0),
                break_after: GridFragmentBreak::RowBoundary,
            }
        );
    }

    #[test]
    fn grid_fragment_plan_maps_fragments_to_fragmentainer_offsets() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(100.0, 0.0),
            180.0,
            &[0.0, 90.0, 180.0],
        );

        assert_eq!(plan.fragment_record_for_offset(0), None);
        assert_eq!(
            plan.fragment_record_for_offset(1),
            Some(GridFragmentRecord {
                fragmentainer_offset: 1,
                slice: GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(90.0),
                    break_after: GridFragmentBreak::RowBoundary,
                },
                transition_before_fragment: Some(GridFragmentTransition::initial_overflow(
                    FragmentainerKind::Page,
                    offset(0.0),
                )),
            })
        );
        assert_eq!(
            plan.fragment_record_for_offset(2),
            Some(GridFragmentRecord {
                fragmentainer_offset: 2,
                slice: GridFragmentSlice {
                    source_block_start: offset(90.0),
                    source_block_end: offset(180.0),
                    break_after: GridFragmentBreak::None,
                },
                transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                    FragmentainerKind::Page,
                    offset(90.0),
                ),),
            })
        );
    }

    #[test]
    fn grid_fragment_plan_commits_fragment_records_and_transitions() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(120.0, 100.0),
            260.0,
            &[0.0, 260.0],
        );

        assert_eq!(
            plan.fragment_records(),
            vec![
                GridFragmentRecord {
                    fragmentainer_offset: 0,
                    slice: GridFragmentSlice {
                        source_block_start: offset(0.0),
                        source_block_end: offset(100.0),
                        break_after: GridFragmentBreak::SlicedRowBand,
                    },
                    transition_before_fragment: None,
                },
                GridFragmentRecord {
                    fragmentainer_offset: 1,
                    slice: GridFragmentSlice {
                        source_block_start: offset(100.0),
                        source_block_end: offset(220.0),
                        break_after: GridFragmentBreak::SlicedRowBand,
                    },
                    transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                        FragmentainerKind::Page,
                        offset(100.0),
                    ),),
                },
                GridFragmentRecord {
                    fragmentainer_offset: 2,
                    slice: GridFragmentSlice {
                        source_block_start: offset(220.0),
                        source_block_end: offset(260.0),
                        break_after: GridFragmentBreak::None,
                    },
                    transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                        FragmentainerKind::Page,
                        offset(220.0),
                    ),),
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_commits_initial_overflow_transition() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(100.0, 0.0),
            60.0,
            &[0.0, 60.0],
        );

        assert_eq!(
            plan.fragment_record_for_offset(1),
            Some(GridFragmentRecord {
                fragmentainer_offset: 1,
                slice: GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(60.0),
                    break_after: GridFragmentBreak::None,
                },
                transition_before_fragment: Some(GridFragmentTransition::initial_overflow(
                    FragmentainerKind::Page,
                    offset(0.0),
                )),
            })
        );
    }

    #[test]
    fn grid_fragment_transitions_preserve_fragmentainer_kind() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Column,
            fragmentainer(100.0, 0.0),
            180.0,
            &[0.0, 90.0, 180.0],
        );

        let fragments = plan.fragment_records();

        assert_eq!(
            fragments[0].transition_before_fragment,
            Some(GridFragmentTransition::initial_overflow(
                FragmentainerKind::Column,
                offset(0.0),
            ))
        );
        assert_eq!(
            fragments[1].transition_before_fragment,
            Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Column,
                offset(90.0),
            ))
        );
    }

    #[test]
    fn grid_fragment_record_projects_slice_from_cursor() {
        let fragment_record = GridFragmentRecord {
            fragmentainer_offset: 1,
            slice: GridFragmentSlice {
                source_block_start: offset(25.0),
                source_block_end: offset(75.0),
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                offset(25.0),
            )),
        };
        let cursor = fragment_record.cursor(PageTopBlockPosition::new(300.0));
        let clip = fragment_record.paint_clip(PageInlineSpan::new(10.0, 120.0), cursor);

        assert_eq!(clip.x(), 10.0);
        assert_eq!(clip.y(), 250.0);
        assert_eq!(clip.width(), 120.0);
        assert_eq!(clip.height(), 50.0);
        assert_eq!(
            cursor.source_block_y(offset(75.0)),
            PageTopBlockPosition::new(250.0)
        );
        assert_eq!(
            cursor.grid_container_origin(10.0),
            PageTopPoint::new(10.0, 325.0)
        );
        assert_eq!(fragment_record.source_range(), (offset(25.0), offset(75.0)));
    }

    #[test]
    fn grid_fragment_record_builds_visible_item_fragments() {
        let fragment_record = GridFragmentRecord {
            fragmentainer_offset: 1,
            slice: GridFragmentSlice {
                source_block_start: offset(50.0),
                source_block_end: offset(150.0),
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                offset(50.0),
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
            fragments[0]
                .visible
                .page_top_rect(PageTopPoint::new(100.0, 300.0)),
            PageTopRect::new(110.0, 250.0, 60.0, 75.0)
        );
        assert_eq!(
            fragments[0].content_slice,
            GridFragmentItemContentSlice {
                block_start: offset(25.0),
                block_end: offset(100.0),
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
                block_start: offset(0.0),
                block_end: offset(10.0),
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
                source_block_start: offset(40.0),
                source_block_end: offset(100.0),
                break_after: GridFragmentBreak::RowBoundary,
            },
            transition_before_fragment: Some(GridFragmentTransition::slice_continuation(
                FragmentainerKind::Page,
                offset(40.0),
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
            fragmentainer(120.0, 100.0),
            260.0,
            &[0.0, 260.0],
        );

        assert_eq!(
            plan.slices(),
            &[
                GridFragmentSlice {
                    source_block_start: offset(0.0),
                    source_block_end: offset(100.0),
                    break_after: GridFragmentBreak::SlicedRowBand,
                },
                GridFragmentSlice {
                    source_block_start: offset(100.0),
                    source_block_end: offset(220.0),
                    break_after: GridFragmentBreak::SlicedRowBand,
                },
                GridFragmentSlice {
                    source_block_start: offset(220.0),
                    source_block_end: offset(260.0),
                    break_after: GridFragmentBreak::None,
                },
            ]
        );
    }

    #[test]
    fn grid_fragment_plan_advances_when_current_fragmentainer_has_no_space() {
        let plan = GridFragmentPlan::from_row_boundaries(
            FragmentainerKind::Page,
            fragmentainer(100.0, 0.0),
            60.0,
            &[0.0, 60.0],
        );

        assert!(plan.starts_after_fragmentainer_break());
        assert_eq!(
            plan.slices(),
            &[GridFragmentSlice {
                source_block_start: offset(0.0),
                source_block_end: offset(60.0),
                break_after: GridFragmentBreak::None,
            }]
        );
    }
}
