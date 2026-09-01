use std::cell::RefCell;
use std::rc::Rc;

use super::super::*;

/// Table-wrapper part that consumed a fragmentainer slice.
#[allow(dead_code)] // Caption/chrome entries are added by their layout recorders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableWrapperTimelineKind {
    TopCaption,
    GridStartChrome,
    GridBody,
    GridEndChrome,
    BottomCaption,
}

/// One committed wrapper source/destination slice.
///
/// Every entry is retained in logical source order and carries the actual
/// destination fragmentainer selected by layout. This makes it impossible to
/// reconstruct a table-root continuation from a physical Y cursor.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperFragmentSlice {
    pub(in crate::layout::table) kind: TableWrapperTimelineKind,
    pub(in crate::layout::table) source: TableWrapperBlockInterval,
    /// The table-grid source interval, when this wrapper entry exposes grid
    /// content.  Wrapper offsets include captions; grid offsets deliberately
    /// do not.
    pub(in crate::layout::table) grid_source_start: Option<TableGridBlockOffset>,
    pub(in crate::layout::table) destination: TableFragmentainerPlacement,
    /// The concrete destination page/column instance. Horizontal page
    /// fragments can share identical geometry, so placement alone cannot
    /// distinguish their separate table-root decoration clips.
    pub(in crate::layout::table) destination_page_index: Option<usize>,
    pub(in crate::layout::table) destination_grid_start: TableGridBlockOffset,
}

#[derive(Debug, Default)]
pub(in crate::layout::table) struct TableWrapperFragmentTimelineState {
    pub(in crate::layout::table) slices: Vec<TableWrapperFragmentSlice>,
    pub(in crate::layout::table) grid_start: Option<TableWrapperGridStart>,
    initial_destination_grid_placement: Option<TableGridPlacement>,
}

#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableWrapperFragmentTimeline {
    pub(in crate::layout::table) state: Rc<RefCell<TableWrapperFragmentTimelineState>>,
}

/// A rollback boundary in the table wrapper's committed paint timeline.
///
/// Break-avoid selection may restore the layout builder to an earlier row
/// boundary. The wrapper timeline is reference-counted across the candidate
/// fragment and the active fragment, so it needs an explicit transactional
/// boundary rather than relying on a clone to undo later row records.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperTimelineCheckpoint(usize);

impl TableWrapperFragmentTimeline {
    /// Start a wrapper-local recorder before caption layout.  Its first grid
    /// placement cannot be known yet: a split top caption can move the grid
    /// into a successor fragmentainer.
    pub(in crate::layout::table) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TableWrapperFragmentTimelineState::default())),
        }
    }

    pub(in crate::layout::table) fn checkpoint(&self) -> TableWrapperTimelineCheckpoint {
        TableWrapperTimelineCheckpoint(self.state.borrow().slices.len())
    }

    pub(in crate::layout::table) fn rewind(&self, checkpoint: TableWrapperTimelineCheckpoint) {
        self.state.borrow_mut().slices.truncate(checkpoint.0);
    }

    /// The grid's actual starting placement in the fragmentainer which
    /// contains the tail of a split top caption.
    #[cfg(test)]
    pub(in crate::layout::table) fn initial_destination_grid_placement(
        &self,
    ) -> TableGridPlacement {
        self.state
            .borrow()
            .initial_destination_grid_placement
            .expect("table wrapper recorder must commit its grid start before row layout")
    }

    pub(in crate::layout::table) fn root_source_frame(
        &self,
        root_rect: TableGridRect,
    ) -> TableWrapperLocalRootSourceFrame {
        self.state
            .borrow()
            .grid_start
            .expect("table wrapper root source requires a committed grid start")
            .root_source_frame(root_rect)
    }

    /// Commit the wrapper progress consumed by top captions and the actual
    /// placement at which the grid starts.  The progress is measured through
    /// the fragmentainer/table placement adapter, rather than reconstructed
    /// from a page-Y cursor in table-root paint.
    #[allow(dead_code)] // Test shorthand for one unsplit caption interval.
    pub(in crate::layout::table) fn record_top_caption_progress(
        &self,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_placement: TableGridPlacement,
        root_block_start_chrome: TableRootBlockStartChrome,
    ) {
        self.record_top_caption_slices(
            &[],
            source_size,
            destination,
            destination_grid_placement,
            root_block_start_chrome,
        );
    }

    /// Record the table wrapper's top-caption source intervals before the
    /// grid starts.  A vertically fragmented caption can have multiple
    /// table-local destinations; retaining every interval prevents the final
    /// grid placement from retroactively becoming the caption's destination.
    pub(in crate::layout::table) fn record_top_caption_slices(
        &self,
        caption_slices: &[TableCaptionPaintSlice],
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_placement: TableGridPlacement,
        root_block_start_chrome: TableRootBlockStartChrome,
    ) {
        // The destination grid placement starts after table-root block-start
        // border/padding/edge spacing. The wrapper composite starts at the
        // root border edge, so strip only that named grid-start chrome before
        // using the progress as a sliced-decoration phase.
        let destination_grid_start = TableGridBlockOffset::new(TableGridLength::new(
            (destination
                .grid_block_progress(destination_grid_placement)
                .length()
                .get()
                - root_block_start_chrome.length().get())
            .max(0.0),
        ));
        let mut state = self.state.borrow_mut();
        if source_size.get() > 0.0 {
            if caption_slices.is_empty() {
                Self::push_slice(
                    &mut state,
                    TableWrapperFragmentSlice {
                        kind: TableWrapperTimelineKind::TopCaption,
                        source: TableWrapperBlockInterval::new(
                            TableWrapperBlockOffset::zero(),
                            source_size,
                        ),
                        grid_source_start: None,
                        destination,
                        destination_page_index: destination.outer_fragmentainer_ordinal(),
                        destination_grid_start,
                    },
                );
            } else {
                for caption in caption_slices {
                    let source_start = TableWrapperBlockOffset::zero()
                        .add(TableGridLength::new(caption.source_block_start.points()));
                    Self::push_slice(
                        &mut state,
                        TableWrapperFragmentSlice {
                            kind: TableWrapperTimelineKind::TopCaption,
                            source: TableWrapperBlockInterval::new(
                                source_start,
                                TableGridLength::new(caption.block_size.points()),
                            ),
                            grid_source_start: None,
                            destination: caption.destination,
                            // Distinct anonymous columns can share the same
                            // scratch geometry. Retain their concrete outer
                            // ordinal so later wrapper scheduling cannot
                            // collapse caption continuations into one
                            // destination merely because their rectangles
                            // compare equal.
                            destination_page_index: Some(caption.page_index),
                            destination_grid_start: TableGridBlockOffset::new(
                                TableGridLength::new(0.0),
                            ),
                        },
                    );
                }
            }
        }
        let caption_end = TableWrapperBlockOffset::zero().add(source_size);
        if root_block_start_chrome.length().get() > 0.0 {
            Self::push_slice(
                &mut state,
                TableWrapperFragmentSlice {
                    kind: TableWrapperTimelineKind::GridStartChrome,
                    source: TableWrapperBlockInterval::new(
                        caption_end,
                        root_block_start_chrome.length(),
                    ),
                    grid_source_start: None,
                    destination,
                    destination_page_index: destination.outer_fragmentainer_ordinal(),
                    destination_grid_start,
                },
            );
        }
        state.grid_start = Some(TableWrapperGridStart::new(
            caption_end,
            root_block_start_chrome,
        ));
        state.initial_destination_grid_placement = Some(destination_grid_placement);
    }

    /// Record an already-committed body slice. The body owns its source-grid
    /// interval; the wrapper timeline owns the order and destination.
    pub(in crate::layout::table) fn record_grid_body_slice(
        &self,
        destination: TableFragmentainerPlacement,
        destination_page_index: usize,
        source_start: TableGridBlockOffset,
        source_size: TableGridLength,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        if let Some(outer) = destination.outer_fragmentainer() {
            debug_assert_eq!(
                destination_page_index,
                outer.ordinal(),
                "table grid slices must use the same outer fragmentainer ordinal as captions"
            );
            debug_assert!(outer.logical_block_capacity() >= 0.0);
        }
        let slice = TableWrapperFragmentSlice {
            kind: TableWrapperTimelineKind::GridBody,
            source: TableWrapperBlockInterval::new(
                self.state
                    .borrow()
                    .grid_start
                    .expect("table wrapper grid start must be committed before body slices")
                    .grid_body_start(source_start),
                source_size,
            ),
            grid_source_start: Some(source_start),
            destination,
            destination_page_index: Some(destination_page_index),
            destination_grid_start,
        };
        Self::push_slice(&mut self.state.borrow_mut(), slice);
    }

    /// Record table-root block-end chrome after all grid source content.
    ///
    /// Captions remain outside the grid source, but their wrapper interval
    /// follows this chrome in the same destination sequence.
    pub(in crate::layout::table) fn record_grid_end_chrome(
        &self,
        grid_source_extent: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        let state = &mut *self.state.borrow_mut();
        let grid_start = state
            .grid_start
            .expect("table wrapper grid start must be committed before trailing chrome");
        let source = TableWrapperBlockInterval::new(
            grid_start.grid_content_start.add(grid_source_extent),
            source_size,
        );
        Self::push_slice(
            state,
            TableWrapperFragmentSlice {
                kind: TableWrapperTimelineKind::GridEndChrome,
                source,
                grid_source_start: None,
                destination,
                destination_page_index: destination.outer_fragmentainer_ordinal(),
                destination_grid_start,
            },
        );
    }

    /// Record a bottom-caption wrapper interval after table grid and trailing
    /// chrome. Captions deliberately have no table-grid source offset.
    #[allow(dead_code)] // Test shorthand for one unsplit caption interval.
    pub(in crate::layout::table) fn record_bottom_caption_progress(
        &self,
        grid_source_extent: TableGridLength,
        trailing_chrome: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        self.record_bottom_caption_slices(
            &[],
            grid_source_extent,
            trailing_chrome,
            source_size,
            destination,
            destination_grid_start,
        );
    }

    /// Record bottom-caption slices after the grid's immutable source range
    /// and trailing chrome.  Their source intervals are wrapper-local, so no
    /// caption entry can acquire a grid-source offset.
    pub(in crate::layout::table) fn record_bottom_caption_slices(
        &self,
        caption_slices: &[TableCaptionPaintSlice],
        grid_source_extent: TableGridLength,
        trailing_chrome: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        let state = &mut *self.state.borrow_mut();
        let grid_start = state
            .grid_start
            .expect("table wrapper grid start must be committed before bottom captions");
        let caption_start = grid_start
            .grid_content_start
            .add(grid_source_extent)
            .add(trailing_chrome);
        if caption_slices.is_empty() {
            Self::push_slice(
                state,
                TableWrapperFragmentSlice {
                    kind: TableWrapperTimelineKind::BottomCaption,
                    source: TableWrapperBlockInterval::new(caption_start, source_size),
                    grid_source_start: None,
                    destination,
                    destination_page_index: destination.outer_fragmentainer_ordinal(),
                    destination_grid_start,
                },
            );
        } else {
            for caption in caption_slices {
                Self::push_slice(
                    state,
                    TableWrapperFragmentSlice {
                        kind: TableWrapperTimelineKind::BottomCaption,
                        source: TableWrapperBlockInterval::new(
                            caption_start
                                .add(TableGridLength::new(caption.source_block_start.points())),
                            TableGridLength::new(caption.block_size.points()),
                        ),
                        grid_source_start: None,
                        destination: caption.destination,
                        destination_page_index: Some(caption.page_index),
                        destination_grid_start: TableGridBlockOffset::new(TableGridLength::new(
                            0.0,
                        )),
                    },
                );
            }
        }
    }

    fn push_slice(state: &mut TableWrapperFragmentTimelineState, slice: TableWrapperFragmentSlice) {
        if let Some(previous) = state.slices.last() {
            // A table layout may revisit the current row while resolving a
            // deferred fragment boundary. It has not advanced either source
            // or destination in that case, so retain one committed slice
            // rather than treating the idempotent replay as an overlap.
            if previous.kind == slice.kind
                && previous.source == slice.source
                && previous.grid_source_start == slice.grid_source_start
                && previous.destination == slice.destination
                && previous.destination_page_index == slice.destination_page_index
                && previous.destination_grid_start == slice.destination_grid_start
            {
                return;
            }
            debug_assert!(
                slice.source.start.0.get()
                    >= previous.source.start.0.get() + previous.source.size().get() - 0.01,
                "table-wrapper source slices must remain ordered and non-overlapping"
            );
        }
        state.slices.push(slice);
    }

    /// Return every grid-body intersection committed in one destination
    /// fragmentainer. Root decoration deliberately ignores caption entries:
    /// captions affect placement, not the table-root positioning area.
    ///
    /// A final page can contain both the tail of a sliced row and a following
    /// row. Table-root decoration must cover both source intervals rather
    /// than only the last one recorded before the fragment is finalized.
    ///
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
    pub(in crate::layout::table) fn grid_body_slices_for(
        &self,
        destination: TableFragmentainerPlacement,
        destination_page_index: usize,
    ) -> Vec<TableWrapperFragmentSlice> {
        self.state
            .borrow()
            .slices
            .iter()
            .filter(|slice| {
                slice.kind == TableWrapperTimelineKind::GridBody
                    && slice.destination == destination
                    && slice.destination_page_index == Some(destination_page_index)
            })
            .copied()
            .collect()
    }

    /// Total logical grid block span committed before `slice`.
    ///
    /// This is the table-root analogue of a generic fragmented block's
    /// preceding fragment spans. Captions select the grid's first
    /// fragmentainer but are outside the table-root background positioning
    /// area, so they are deliberately excluded here.
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>
    pub(in crate::layout::table) fn preceding_grid_body_block_span(
        &self,
        slice: TableWrapperFragmentSlice,
    ) -> TableGridLength {
        TableGridLength::new(
            self.state
                .borrow()
                .slices
                .iter()
                .filter(|candidate| {
                    candidate.kind == TableWrapperTimelineKind::GridBody
                        && candidate.source.start().points() < slice.source.start().points() - 0.01
                })
                .map(|candidate| candidate.source.size().get())
                .sum(),
        )
    }

    /// Whether this wrapper has committed any grid-body source interval.
    /// A later table fragment with no matching local ledger entry must not
    /// fall back to painting the complete root rectangle: that was only valid
    /// before table-local structural slices were committed.
    pub(in crate::layout::table) fn has_grid_body_slices(&self) -> bool {
        self.state
            .borrow()
            .slices
            .iter()
            .any(|slice| slice.kind == TableWrapperTimelineKind::GridBody)
    }
}
