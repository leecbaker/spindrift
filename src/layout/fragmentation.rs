use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::layout::assets::FragmentainerOrdinal;

/// One unfragmented item source canvas retained for later projection through
/// committed container fragments.
///
/// The frozen used item box determines the formatting context used to produce
/// this capture; `source_height` instead records the physical content extent
/// occupied by its visible in-flow source.  Keeping both facts in one owned
/// artifact prevents continuation replay from re-running layout against a
/// page-local fragmentainer.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
#[derive(Debug)]
pub(in crate::layout) struct ContinuousSourceReplay {
    pub(in crate::layout) paint: PaintFragment,
    pub(in crate::layout) effects: DeferredLayoutSideEffects,
    pub(in crate::layout) source_height: PhysicalContentHeight,
    pub(in crate::layout) scratch_top: f32,
}

/// A committed source-to-destination mapping for one container fragment.
///
/// A source slice and its destination fragmentainer are inseparable once
/// fragmentation commits.  Most importantly, a destination that owns the
/// principal box cannot be represented without its border box: callers must
/// distinguish it from a continuation that only replays descendant overflow.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CommittedContainerFragment<SourceSlice> {
    fragmentainer: FragmentainerOrdinal,
    source_slice: SourceSlice,
    kind: ContainerFragmentKind,
}

impl<SourceSlice> CommittedContainerFragment<SourceSlice> {
    pub(in crate::layout) fn principal(
        fragmentainer: FragmentainerOrdinal,
        source_slice: SourceSlice,
        border_box: PaintClip,
        decoration: FragmentDecoration,
    ) -> Self {
        Self {
            fragmentainer,
            source_slice,
            kind: ContainerFragmentKind::Principal(DecoratedBoxFragment {
                border_box,
                decoration,
            }),
        }
    }

    pub(in crate::layout) fn descendant_overflow_only(
        fragmentainer: FragmentainerOrdinal,
        source_slice: SourceSlice,
    ) -> Self {
        Self {
            fragmentainer,
            source_slice,
            kind: ContainerFragmentKind::DescendantOverflowOnly,
        }
    }

    pub(in crate::layout) const fn fragmentainer(&self) -> FragmentainerOrdinal {
        self.fragmentainer
    }

    pub(in crate::layout) fn source_slice(&self) -> &SourceSlice {
        &self.source_slice
    }

    pub(in crate::layout) const fn kind(&self) -> &ContainerFragmentKind {
        &self.kind
    }

    pub(in crate::layout) fn kind_mut(&mut self) -> &mut ContainerFragmentKind {
        &mut self.kind
    }
}

/// Whether a destination fragment owns the container's principal box.
///
/// A descendant-only continuation is real flow content, but it is not a box
/// fragment of its ancestor and must never recreate that ancestor's
/// background, border, outline, or shadow.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) enum ContainerFragmentKind {
    Principal(DecoratedBoxFragment),
    DescendantOverflowOnly,
}

impl ContainerFragmentKind {
    pub(in crate::layout) const fn principal_box(&self) -> Option<&DecoratedBoxFragment> {
        match self {
            Self::Principal(fragment) => Some(fragment),
            Self::DescendantOverflowOnly => None,
        }
    }

    pub(in crate::layout) fn principal_box_mut(&mut self) -> Option<&mut DecoratedBoxFragment> {
        match self {
            Self::Principal(fragment) => Some(fragment),
            Self::DescendantOverflowOnly => None,
        }
    }
}

/// The mandatory paint geometry and decoration policy of a principal box
/// fragment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct DecoratedBoxFragment {
    border_box: PaintClip,
    decoration: FragmentDecoration,
}

impl DecoratedBoxFragment {
    pub(in crate::layout) const fn new(
        border_box: PaintClip,
        decoration: FragmentDecoration,
    ) -> Self {
        Self {
            border_box,
            decoration,
        }
    }

    pub(in crate::layout) const fn border_box(&self) -> PaintClip {
        self.border_box
    }

    pub(in crate::layout) fn set_border_box(&mut self, border_box: PaintClip) {
        self.border_box = border_box;
    }

    pub(in crate::layout) const fn decoration(&self) -> FragmentDecoration {
        self.decoration
    }

    pub(in crate::layout) fn decoration_mut(&mut self) -> &mut FragmentDecoration {
        &mut self.decoration
    }
}

/// Decoration ownership for one principal box fragment.
///
/// `clone` has no representable missing edges: every clone fragment owns both
/// broken block edges. Only `slice` needs first/last-fragment edge state.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentDecoration {
    Clone,
    Slice(SliceFragmentEdges),
}

impl FragmentDecoration {
    pub(in crate::layout) const fn for_box_decoration_break(
        decoration_break: css::BoxDecorationBreak,
        owns_block_start: bool,
        owns_block_end: bool,
    ) -> Self {
        match decoration_break {
            css::BoxDecorationBreak::Clone => Self::Clone,
            css::BoxDecorationBreak::Slice => Self::Slice(SliceFragmentEdges {
                owns_block_start,
                owns_block_end,
            }),
        }
    }

    pub(in crate::layout) const fn owns_block_start(self) -> bool {
        match self {
            Self::Clone => true,
            Self::Slice(edges) => edges.owns_block_start,
        }
    }

    pub(in crate::layout) const fn owns_block_end(self) -> bool {
        match self {
            Self::Clone => true,
            Self::Slice(edges) => edges.owns_block_end,
        }
    }

    pub(in crate::layout) const fn is_clone(self) -> bool {
        matches!(self, Self::Clone)
    }

    pub(in crate::layout) fn clear_block_end_for_slice(&mut self) {
        if let Self::Slice(edges) = self {
            edges.owns_block_end = false;
        }
    }
}

/// The block-axis space a principal box fragment reserves for cloned
/// decoration before its content can make fragmentation progress.
///
/// CSS Fragmentation fills a fragmentainer with the box's content box while
/// leaving room for cloned borders and padding. Margins have separate
/// block-layout truncation rules, so they deliberately do not enter this
/// content-capacity calculation.
/// <https://www.w3.org/TR/css-break-3/#breaks>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentDecorationReservation {
    block_start: NonContentLength,
    block_end: NonContentLength,
}

impl FragmentDecorationReservation {
    /// Resolve the reservations owned by one principal box fragment.
    ///
    /// `clone` always retains both edges. `slice` uses only the edges carried
    /// by [`FragmentDecoration`], so callers cannot reserve a broken slice
    /// edge by accident.
    pub(in crate::layout) fn new(
        decoration: FragmentDecoration,
        block_start: NonContentLength,
        block_end: NonContentLength,
    ) -> Self {
        Self {
            block_start: if decoration.owns_block_start() {
                block_start
            } else {
                non_content_pt(0.0)
            },
            block_end: if decoration.owns_block_end() {
                block_end
            } else {
                non_content_pt(0.0)
            },
        }
    }

    /// The block-start inset needed when a continuation enters a fresh
    /// fragmentainer.
    pub(in crate::layout) const fn block_start(&self) -> NonContentLength {
        self.block_start
    }

    /// The block-end inset that must remain available for this fragment's
    /// owned border and padding.
    pub(in crate::layout) const fn block_end(&self) -> NonContentLength {
        self.block_end
    }

    /// Content capacity once the cursor has already crossed the fragment's
    /// block-start decoration.
    pub(in crate::layout) fn remaining_content_extent(
        self,
        raw_remaining_extent: LayoutLength,
    ) -> LayoutLength {
        layout_pt((raw_remaining_extent.points() - self.block_end.points()).max(0.0))
    }

    /// Content capacity of a fresh fragmentainer, before the fragment's
    /// block-start decoration has been consumed.
    pub(in crate::layout) fn fresh_content_extent(
        self,
        raw_fragmentainer_extent: LayoutLength,
    ) -> LayoutLength {
        layout_pt(
            (raw_fragmentainer_extent.points()
                - self.block_start.points()
                - self.block_end.points())
            .max(0.0),
        )
    }
}

/// First/last-fragment ownership used only by sliced decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct SliceFragmentEdges {
    owns_block_start: bool,
    owns_block_end: bool,
}

/// One committed destination selected by generic fragmentation.
///
/// Layout algorithms normally consume this transition immediately through
/// their own cursor. Table captions are wrapper-flow siblings, however, and
/// must hand the exact destination fragmentainer to table-grid layout after
/// generic caption content has finished. This record deliberately exposes
/// page-top bounds rather than a mutable cursor pair.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[allow(dead_code)] // Legacy generic observer retained for non-table instrumentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerTransitionRecord {
    pub(in crate::layout) kind: FragmentainerKind,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) content_bounds: PageTopRect,
}

/// Scoped observer for committed generic fragmentainer transitions.
///
/// This is intentionally an internal recorder rather than a callback: it
/// cannot alter pagination while a generic block formatter owns the
/// transition. Callers receive a stable ordered snapshot only after closing
/// their scope.
#[allow(dead_code)] // Table wrapper flow no longer uses generic transition handoff.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct FragmentainerTransitionRecorder(
    Rc<RefCell<Vec<FragmentainerTransitionRecord>>>,
);

#[allow(dead_code)]
impl FragmentainerTransitionRecorder {
    pub(in crate::layout) fn records(&self) -> Vec<FragmentainerTransitionRecord> {
        self.0.borrow().clone()
    }

    fn push(&self, record: FragmentainerTransitionRecord) {
        self.0.borrow_mut().push(record);
    }

    pub(in crate::layout) fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub(in crate::layout) fn truncate(&self, len: usize) {
        self.0.borrow_mut().truncate(len);
    }
}

/// Maximum anonymous column fragmentainers retained for one committed replay.
///
/// Continuous multicol overflow can contain an authored block size that would
/// imply millions of columns, while only a finite prefix can intersect a PDF
/// page or its bounded nested-fragment replay. Layout retains that prefix and
/// carries the logical tail arithmetically instead of allocating one temporary
/// [`Page`] per off-canvas column.
/// <https://www.w3.org/TR/css-multicol-1/#overflow>
pub(in crate::layout) const MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS: usize = 256;

/// Maximum page fragments retained while consuming one monolithic definite
/// block size. This is a resource boundary for pathological CSS lengths; the
/// remaining logical extent is carried arithmetically rather than allocating
/// unbounded empty PDF pages.
pub(in crate::layout) const MAX_MATERIALIZED_PAGE_FRAGMENTAINERS: usize = 256;

/// Maximum conceptual columns considered by the multicol balancing probe.
///
/// Balancing repeatedly lays out the same content. Large overflow is still
/// represented by the committed continuation plan, but probing a result whose
/// balance would require more than this many fragmentainers cannot improve the
/// visible bounded prefix and would multiply work by the binary-search count.
pub(in crate::layout) const MAX_MULTICOL_BALANCE_PROBE_FRAGMENTAINERS: usize = 4;

/// Bounded materialization for a run of equal-size column continuations.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ColumnContinuationMaterialization {
    pub(in crate::layout) pages_to_push: usize,
    pub(in crate::layout) last_fragment_used_block_size: LayoutLength,
    pub(in crate::layout) has_unmaterialized_tail: bool,
}

/// Plan column continuations with a caller-proven materialized prefix.
///
/// The caller may lower this limit only for a positioned subtree whose
/// non-scrollable overflow clip makes the remaining logical tail unreachable
/// in static output. All other callers retain the normal multicol limit.
#[cfg(test)]
pub(in crate::layout) fn column_continuation_materialization_with_limit(
    remaining_block_size: LayoutLength,
    continuation_block_size: LayoutLength,
    already_materialized: usize,
    materialization_limit: usize,
) -> ColumnContinuationMaterialization {
    continuation_materialization(
        remaining_block_size,
        continuation_block_size,
        already_materialized,
        materialization_limit,
    )
}

/// Plan a bounded continuation run for any equal-size fragmentainer sequence.
#[cfg(test)]
pub(in crate::layout) fn continuation_materialization(
    remaining_block_size: LayoutLength,
    continuation_block_size: LayoutLength,
    already_materialized: usize,
    materialization_limit: usize,
) -> ColumnContinuationMaterialization {
    let continuation_block_size = continuation_block_size.points().max(css::CSS_PX_TO_PT);
    let remaining_block_size = remaining_block_size.points().max(0.0);
    let required =
        ((remaining_block_size - 0.01).max(0.0) / continuation_block_size).ceil() as usize;
    let available = materialization_limit.saturating_sub(already_materialized.max(1));
    let pages_to_push = required.min(available);
    let last_fragment_used_block_size = if required == 0 {
        layout_pt(0.0)
    } else {
        let preceding = continuation_block_size * required.saturating_sub(1) as f32;
        layout_pt((remaining_block_size - preceding).clamp(0.0, continuation_block_size))
    };
    ColumnContinuationMaterialization {
        pages_to_push,
        last_fragment_used_block_size,
        has_unmaterialized_tail: required > pages_to_push,
    }
}

/// Finite capacity for a CSS fragmentainer in the block direction.
///
/// CSS Fragmentation lays content into fragmentainers with a finite block-size.
/// This type carries the fragmentainer's empty block-size and current remaining
/// block-size so layout modes can share overflow and slice-boundary arithmetic
/// while keeping mode-specific reservations, such as repeated table chrome,
/// local:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct Fragmentainer {
    fragmentainer_block_size: LayoutLength,
    available_block_size: LayoutLength,
}

/// Fragmentation context targeted by a break decision.
///
/// CSS Break defines common break values across fragmentation contexts, but
/// target-specific values only apply to their matching fragmentainer type. The
/// shared layout code uses this kind to keep page and column decisions on the
/// same algorithm while preserving `avoid-page`/`avoid-column` and
/// `page`/`column` scoping:
/// <https://www.w3.org/TR/css-break-3/#break-types>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentainerKind {
    Page,
    Column,
}

/// The reason a layout algorithm advances to its next fragmentainer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentainerAdvance {
    Unforced,
    Forced(PageBreak),
}

impl LayoutBuilder<'_> {
    /// Begin observing generic fragmentainer destinations for one scoped
    /// caller. The current fragmentainer is always the first record, so an
    /// unfragmented caption and a fragmented caption share the same outcome
    /// shape.
    #[allow(dead_code)]
    pub(in crate::layout) fn begin_fragmentainer_transition_recording(
        &mut self,
    ) -> FragmentainerTransitionRecorder {
        let recorder = FragmentainerTransitionRecorder::default();
        self.fragmentainer_transition_recorders
            .push(recorder.clone());
        self.record_current_fragmentainer_destination();
        recorder
    }

    /// Close the most-recent recorder scope and return its ordered records.
    #[allow(dead_code)]
    pub(in crate::layout) fn finish_fragmentainer_transition_recording(
        &mut self,
        recorder: FragmentainerTransitionRecorder,
    ) -> Vec<FragmentainerTransitionRecord> {
        let active = self
            .fragmentainer_transition_recorders
            .pop()
            .expect("fragmentainer transition recording must be closed in scope order");
        debug_assert!(Rc::ptr_eq(&active.0, &recorder.0));
        recorder.records()
    }

    /// Publish the active fragmentainer after a committed pagination change.
    pub(in crate::layout) fn record_current_fragmentainer_destination(&mut self) {
        if self.fragmentainer_transition_recorders.is_empty() {
            return;
        }
        let context = self.current_page_context;
        let record = FragmentainerTransitionRecord {
            kind: self.active_fragmentainer_kind(),
            page_index: self.pages.len(),
            content_bounds: PageTopRect::new(
                context.left(),
                context.top(),
                context.area_width(),
                context.area_height(),
            ),
        };
        for recorder in &self.fragmentainer_transition_recorders {
            recorder.push(record);
        }
    }

    /// Build a fragmentainer from a page-top cursor position and the current
    /// page's block-end edge.
    pub(in crate::layout) fn fragmentainer_from_page_cursor(
        &self,
        content_block_start: PageTopBlockPosition,
    ) -> Fragmentainer {
        Fragmentainer::from_page_cursor_bounds(
            layout_pt(self.page_area_height()),
            content_block_start,
            PageTopBlockPosition::new(self.page_bottom()),
        )
    }

    /// Split a fixed principal block through root paged-media fragmentainers
    /// whose fragmentation direction is horizontal.
    ///
    /// The paged-media page is always a physical rectangle, but CSS
    /// Fragmentation selects the root flow's logical block direction. A
    /// vertical principal flow therefore fills the current physical X track
    /// and continues on a new page once that track is exhausted. The legacy
    /// `cursor_y` remains the logical inline cursor; callers retain it for
    /// line layout and do not reinterpret it as an X coordinate.
    ///
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
    pub(in crate::layout) fn consume_vertical_root_page_block_size(
        &mut self,
        block_size: LayoutLength,
        inline_block_start: PageTopBlockPosition,
    ) -> Option<VerticalRootPageFragmentation> {
        if self.active_fragmentainer_kind() != FragmentainerKind::Page
            || !WritingModeAxes::new(
                self.principal_flow.writing_mode,
                self.principal_flow.used_direction(),
            )
            .swaps_physical_axes()
            || self.containing_block_writing_mode != self.principal_flow.writing_mode
        {
            return None;
        }

        let mut remaining = block_size.points().max(0.0);
        let initial_available = self
            .current_page_context
            .logical_block_size(self.principal_flow.writing_mode);
        if remaining <= initial_available + 0.01 {
            return None;
        }

        let block_start_side = FlowAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        )
        .block_start_side();
        debug_assert!(matches!(
            block_start_side,
            PhysicalSide::Left | PhysicalSide::Right
        ));

        let mut source_block_start = 0.0;
        let mut fragments = Vec::new();
        loop {
            let context = self.current_page_context;
            let available = self
                .current_page_context
                .logical_block_size(self.principal_flow.writing_mode);
            // CSS Fragmentation gives fragmentainers a one-pixel minimum
            // block size for progress. A zero-width page area cannot expose
            // useful paint, so do not materialize an unbounded empty run.
            if available <= 0.01 {
                break;
            }
            let used = remaining.min(available);
            let destination_block_start = match block_start_side {
                PhysicalSide::Left => 0.0,
                PhysicalSide::Right => 0.0,
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical root flow has a horizontal block direction")
                }
            };
            fragments.push(VerticalRootPageFragmentSlice {
                page_index: self.pages.len(),
                source_block_start: layout_pt(source_block_start),
                block_size: layout_pt(used),
                destination_context: context,
                // Root vertical page fragments restart at the page area's
                // logical inline start. The physical `cursor_y` belongs to
                // the legacy vertical inline layout path and is not a root
                // page-fragmentation coordinate. The first source fragment
                // retains its inline-start margin; continuation fragments
                // begin at the destination fragmentainer edge.
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: 0.0,
                    block: available,
                },
                destination_block_start: layout_pt(destination_block_start),
            });
            remaining -= used;
            source_block_start += used;
            match block_start_side {
                PhysicalSide::Left => {
                    self.content_left = (context.left() + used).min(context.right())
                }
                PhysicalSide::Right => {
                    self.content_right = (context.right() - used).max(context.left())
                }
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical root flow has a horizontal block direction")
                }
            }
            if remaining <= 0.01 {
                break;
            }
            // This source fragment is structural flow occupancy even when
            // its own decoration is deferred until used-size resolution.
            self.mark_current_page_flow_content();
            self.push_page();
        }
        Some(VerticalRootPageFragmentation {
            fragments,
            first_inline_origin: inline_block_start,
        })
    }

    /// Paint fixed vertical-root block fragments after their logical source
    /// ranges have been assigned to page fragmentainers.
    ///
    /// [`FragmentainerProjection`] owns the logical-to-physical conversion,
    /// including the `vertical-rl` block-direction reversal. The page painter
    /// therefore receives only destination-local physical rectangles.
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) fn vertical_root_block_fragment_paint(
        &mut self,
        slices: &[VerticalRootPageFragmentSlice],
        style: &ComputedStyle,
        source_border_rect: PaintRect,
    ) {
        if slices.is_empty() || style.visibility != Visibility::Visible {
            return;
        }
        let axes = FlowAxes::new(style.writing_mode, style.used_direction());
        let source_extent = LogicalSize {
            inline: source_border_rect.size.height,
            block: source_border_rect.size.width,
        };
        let source_origin = PageTopPoint::new(
            source_border_rect.origin.x,
            source_border_rect.origin.y + source_border_rect.size.height,
        );
        for (index, slice) in slices.iter().enumerate() {
            let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
                source_axes: axes,
                source_origin,
                source_extent,
                source_slice: LogicalRect {
                    origin: LogicalPoint {
                        inline: 0.0,
                        block: slice.source_block_start.points(),
                    },
                    size: LogicalSize {
                        inline: source_extent.inline,
                        block: slice.block_size.points(),
                    },
                },
                destination_axes: axes,
                destination_origin: slice.destination_origin,
                destination_extent: LogicalSize {
                    inline: source_extent.inline,
                    block: slice.destination_extent.block,
                },
                destination_slice: LogicalRect {
                    origin: LogicalPoint {
                        inline: 0.0,
                        block: slice.destination_block_start.points(),
                    },
                    size: LogicalSize {
                        inline: source_extent.inline,
                        block: slice.block_size.points(),
                    },
                },
                destination_page_area: PageTopRect::new(
                    slice.destination_context.left(),
                    slice.destination_context.top(),
                    slice.destination_context.area_width(),
                    slice.destination_context.area_height(),
                ),
            });
            let mut fragment_style = style.clone();
            crate::layout::block::suppress_fragmented_box_edges(
                &mut fragment_style,
                index == 0,
                index + 1 == slices.len(),
            );
            let border_rect = projection.destination_clip().paint_rect();
            let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                self.box_background_primitives(border_rect, &fragment_style),
            );
            fragment.append_primitives_in_band(
                PaintBand::Outline,
                self.box_outline_primitives(border_rect, &fragment_style),
            );
            fragment.promote_background_border_to_in_flow_block();
            fragment.promote_outline_to_in_flow_outline();
            // A terminal box fragment can contain only the final decoration.
            // Keep that page materialized even when the source document has
            // no descendant paint tree on it.
            if slice.page_index < self.pages.len() {
                self.pages[slice.page_index]
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
            if index + 1 == slices.len() {
                if slice.page_index < self.pages.len() {
                    self.pages[slice.page_index].mark_fragmentation_content();
                } else {
                    self.mark_current_page_flow_content();
                    self.current_page.mark_fragmentation_content();
                }
            }
        }
    }

    /// Replay a captured paint subtree through vertical root page slices.
    ///
    /// The source subtree stays in the original page-local physical space;
    /// each destination uses the same logical slice projection as the
    /// principal box decoration. This is intentionally separate from
    /// `PaintFragment::clipped_to_rect`, which clips both axes and would
    /// incorrectly constrain logical inline overflow.
    pub(in crate::layout) fn project_vertical_root_fragment_paint(
        &self,
        source: PaintFragment,
        slices: &[VerticalRootPageFragmentSlice],
        axes: FlowAxes,
        source_origin: PageTopPoint,
        source_extent: LogicalSize,
    ) -> Vec<(usize, PaintFragment)> {
        slices
            .iter()
            .filter_map(|slice| {
                let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
                    source_axes: axes,
                    source_origin,
                    source_extent,
                    source_slice: LogicalRect {
                        origin: LogicalPoint {
                            inline: 0.0,
                            block: slice.source_block_start.points(),
                        },
                        size: LogicalSize {
                            inline: source_extent.inline,
                            block: slice.block_size.points(),
                        },
                    },
                    destination_axes: axes,
                    destination_origin: slice.destination_origin,
                    destination_extent: LogicalSize {
                        inline: source_extent.inline,
                        block: slice.destination_extent.block,
                    },
                    destination_slice: LogicalRect {
                        origin: LogicalPoint {
                            inline: 0.0,
                            block: slice.destination_block_start.points(),
                        },
                        size: LogicalSize {
                            inline: source_extent.inline,
                            block: slice.block_size.points(),
                        },
                    },
                    destination_page_area: PageTopRect::new(
                        slice.destination_context.left(),
                        slice.destination_context.top(),
                        slice.destination_context.area_width(),
                        slice.destination_context.area_height(),
                    ),
                });
                let fragment = source
                    .clone()
                    .with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
                        css::PhysicalAxis::Horizontal,
                        projection.source_clip(),
                        true,
                    )
                    .translated(projection.destination_translation());
                (!fragment.is_empty()).then_some((slice.page_index, fragment))
            })
            .collect()
    }

    /// Materialize the next anonymous column while retaining the completed
    /// source column as a structural fragmentainer.
    ///
    /// A valid class-A forced break may follow a paintless box. The empty
    /// source column still precedes the destination in the fragmentation
    /// context, so it must not be replaced by `push_page`'s empty-page
    /// coalescing. Break authorization remains with the caller: this helper
    /// only records a column once a transition has already been selected.
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>
    /// <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
    pub(in crate::layout) fn materialize_column_continuation(&mut self) {
        debug_assert!(
            self.fragmentainer_override
                .is_some_and(|override_| override_.kind == FragmentainerKind::Column)
        );
        if !self.current_page_has_content() {
            self.mark_current_page_flow_content();
        }
        self.push_page();
    }

    /// Materialize a layout algorithm's transition to another fragmentainer.
    ///
    /// CSS Fragmentation defines the distinction between ordinary and forced
    /// breaks in [§ 3.1](https://www.w3.org/TR/css-break-3/#breaking-controls).
    pub(in crate::layout) fn materialize_fragmentainer_advance(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        advance: FragmentainerAdvance,
    ) -> Option<f32> {
        match fragmentainer_kind {
            // A multicolumn layout uses temporary pages as its concrete
            // fragmentainers.  `push_page` already selects the next
            // `FragmentainerOverride` context and records the source column
            // for later projection, so flex/grid fragmentation must advance
            // it just as ordinary block flow does.  Treating a column break
            // as metadata-only leaves a committed source slice with no
            // destination fragmentainer in which to replay it.
            // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
            FragmentainerKind::Column
                if !self
                    .fragmentainer_override
                    .is_some_and(|override_| override_.kind == FragmentainerKind::Column) =>
            {
                return None;
            }
            FragmentainerKind::Column => match advance {
                FragmentainerAdvance::Unforced => self.materialize_column_continuation(),
                FragmentainerAdvance::Forced(page_break) => {
                    self.apply_forced_break_in(fragmentainer_kind, page_break);
                }
            },
            FragmentainerKind::Page => match advance {
                FragmentainerAdvance::Unforced => self.push_page(),
                FragmentainerAdvance::Forced(page_break) => {
                    self.apply_forced_break_in(fragmentainer_kind, page_break);
                }
            },
        }

        Some(self.cursor_y)
    }
}

/// Resolved break values around one fragmentable source box.
///
/// CSS Fragmentation evaluates forced and avoided breaks at class A
/// opportunities between adjacent boxes. This context keeps the pending
/// incoming break, current box `break-before`, current box `break-after`, and
/// next sibling `break-before` together so layout modes do not mix authored
/// break sources while choosing forced or avoid-constrained boundaries:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentBreakContext {
    pub(in crate::layout) pending_before: PageBreak,
    pub(in crate::layout) before: PageBreak,
    pub(in crate::layout) after: PageBreak,
    pub(in crate::layout) next_before: PageBreak,
}

/// Cross-sibling forced break state carried while planning fragments.
///
/// CSS Fragmentation resolves forced breaks between adjacent boxes at class A
/// break opportunities. A source box's `break-after` becomes the next sibling's
/// pending `break-before` while siblings remain, or leaves the fragmenting
/// container as the outgoing forced break when the source list is exhausted:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ForcedBreakCarryState {
    fragmentainer_kind: FragmentainerKind,
    before_next_box: PageBreak,
    after_source_boxes: PageBreak,
}

/// Shared decision for arming and consuming an avoid-run break candidate.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints at class A break opportunities. Layout modes that can roll an
/// avoid-constrained sibling run to the next fragmentainer need the same
/// inputs: whether the source box participates in the relevant flow, the
/// current adjacent-box break context, the committed break opportunity before
/// the source box, optional next sibling avoid state, and whether a rollback
/// candidate already exists:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentAvoidRunStartDecision {
    pub(in crate::layout) should_arm_start_candidate: bool,
    pub(in crate::layout) is_avoid_boundary: bool,
    pub(in crate::layout) seeds_later_avoid_boundary: bool,
}

/// A class A break opportunity at a source block-axis boundary.
///
/// CSS Fragmentation resolves forced and avoided breaks at boundaries between
/// in-flow boxes. Layout modes provide the source boundary geometry, while the
/// shared fragmentation layer applies target-aware break value scoping for
/// pages, columns, and future fragmentainer types:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks> and
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentBreakOpportunity {
    pub(in crate::layout) source_block_offset: f32,
    pub(in crate::layout) break_before: PageBreak,
    pub(in crate::layout) break_after: PageBreak,
    pub(in crate::layout) break_inside_avoid: bool,
}

/// Source-range query for choosing a committed break boundary.
///
/// CSS Fragmentation first determines which possible break points are
/// available in the current fragmentainer, then chooses forced breaks before
/// unforced breaks. Layout modes pass their ordered source boundaries here and
/// keep any mode-specific replay metadata outside the shared chooser:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks> and
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentBreakOpportunitySearch<'a> {
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) opportunities: &'a [FragmentBreakOpportunity],
    pub(in crate::layout) source_block_start: f32,
    pub(in crate::layout) available_block_end: f32,
    pub(in crate::layout) content_block_end: f32,
}

/// Which side of an adjacent-box boundary contributes an avoid constraint.
///
/// CSS Fragmentation evaluates `break-after` from the previous box and
/// `break-before` from the following box at the same class A break
/// opportunity. Some layout modes need to know the side, not just whether the
/// boundary is avoided, because rollback metadata belongs to the source that
/// started the keep-together run:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FragmentAvoidBoundarySide {
    None,
    Previous,
    Current,
}

pub(in crate::layout) struct FragmentAvoidRunStartInput {
    pub(in crate::layout) participates_in_flow: bool,
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) break_context: FragmentBreakContext,
    pub(in crate::layout) break_opportunity: FragmentBreakOpportunity,
    pub(in crate::layout) next_break_before: Option<PageBreak>,
    pub(in crate::layout) has_avoid_run_candidate: bool,
}

/// Fragment-local slice chosen from a fragmentable source range.
///
/// CSS Fragmentation lets a box that is larger than a fragmentainer be split
/// across fragmentainers. Layout modes still own their source geometry and
/// paint/replay metadata, but the arithmetic for choosing the current
/// source-range end and detecting "advance before painting" is common:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentSourceSliceDecision {
    pub(in crate::layout) slice_start: f32,
    pub(in crate::layout) slice_end: f32,
    pub(in crate::layout) advance_before_slice: bool,
}

pub(in crate::layout) struct FragmentSourceSliceInput {
    pub(in crate::layout) break_is_applicable: bool,
    pub(in crate::layout) source_is_oversized: bool,
    pub(in crate::layout) source_block_end: f32,
    pub(in crate::layout) slice_start: f32,
    pub(in crate::layout) available_block_end: f32,
}

/// Maps one logical fragmentainer slice from its captured source canvas to
/// its destination fragmentainer canvas.
///
/// The generic fragmentation algorithms choose source ranges in logical block
/// coordinates.  Paint replay, however, must clip and translate physical PDF
/// primitives.  Keeping that conversion here means multicol, tables, and
/// future fragmenting layout modes share the same Writing Modes boundary
/// instead of each matching physical `x`/`y` axes independently:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerProjection {
    source_clip: PaintClip,
    destination_clip: PaintClip,
    destination_translation: PaintTranslation,
    destination_page_clip_in_source_space: PaintClip,
}

/// Inputs for projecting an equal logical source/destination fragment slice.
///
/// Source and destination containers may have different logical extents: a
/// temporary multicol source can retain overflow slices while the destination
/// belongs to a wrapped column row.  The two slice rectangles describe the
/// same source content in their respective containers.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentainerProjectionInput {
    /// The fragmented source's coordinate system. Source ranges are selected
    /// in this flow's logical block axis.
    pub(in crate::layout) source_axes: FlowAxes,
    pub(in crate::layout) source_origin: PageTopPoint,
    pub(in crate::layout) source_extent: LogicalSize,
    pub(in crate::layout) source_slice: LogicalRect,
    /// The destination fragmentainer's coordinate system. A nested
    /// orthogonal flow can be painted into a parent fragmentainer whose
    /// logical axes differ from the source flow.
    pub(in crate::layout) destination_axes: FlowAxes,
    pub(in crate::layout) destination_origin: PageTopPoint,
    pub(in crate::layout) destination_extent: LogicalSize,
    pub(in crate::layout) destination_slice: LogicalRect,
    pub(in crate::layout) destination_page_area: PageTopRect,
}

/// One physical page destination for a continuous vertical root block.
///
/// The source range and destination range are logical block-axis lengths.
/// Physical clipping and translation are deliberately deferred to
/// [`FragmentainerProjection`], which is the single Writing Modes boundary
/// for fragmented paint.
#[derive(Debug, Clone)]
pub(in crate::layout) struct VerticalRootPageFragmentation {
    pub(in crate::layout) fragments: Vec<VerticalRootPageFragmentSlice>,
    pub(in crate::layout) first_inline_origin: PageTopBlockPosition,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct VerticalRootPageFragmentSlice {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) source_block_start: LayoutLength,
    pub(in crate::layout) block_size: LayoutLength,
    pub(in crate::layout) destination_context: PageContext,
    pub(in crate::layout) destination_origin: PageTopPoint,
    /// The logical block capacity of this destination's active root track.
    /// `inline` is filled from the source box when projecting paint.
    pub(in crate::layout) destination_extent: LogicalSize,
    pub(in crate::layout) destination_block_start: LayoutLength,
}

/// One continuous root-flow source range assigned to a page fragmentainer.
///
/// This remains logical even though the destination page is physical: a
/// vertical root maps these ranges to physical X, while a horizontal root maps
/// the equivalent ranges to physical Y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RootPageBlockSlice {
    pub(in crate::layout) source_block_start: LayoutLength,
    pub(in crate::layout) block_size: LayoutLength,
}

/// Partition a continuous root block range into equal-capacity page slices.
///
/// Page continuation and deferred overflow both use this source-range plan;
/// paint projection remains responsible for the root flow's physical-axis
/// conversion. A bounded prefix avoids allocating unbounded page state for
/// pathological CSS lengths.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout) fn root_page_block_slices(
    source_block_size: LayoutLength,
    page_block_capacity: LayoutLength,
) -> Vec<RootPageBlockSlice> {
    let capacity = page_block_capacity.points().max(0.0);
    if capacity <= 0.01 {
        return Vec::new();
    }
    let mut remaining = source_block_size.points().max(0.0);
    let mut source_block_start = 0.0;
    let mut slices = Vec::new();
    while remaining > 0.01 && slices.len() < MAX_MATERIALIZED_PAGE_FRAGMENTAINERS {
        let block_size = remaining.min(capacity);
        slices.push(RootPageBlockSlice {
            source_block_start: layout_pt(source_block_start),
            block_size: layout_pt(block_size),
        });
        remaining -= block_size;
        source_block_start += block_size;
    }
    slices
}

impl FragmentainerProjection {
    pub(in crate::layout) fn new(input: FragmentainerProjectionInput) -> Self {
        let source = page_top_rect_from_logical_fragment(
            input.source_axes,
            input.source_origin,
            input.source_extent,
            input.source_slice,
        );
        let destination = page_top_rect_from_logical_fragment(
            input.destination_axes,
            input.destination_origin,
            input.destination_extent,
            input.destination_slice,
        );
        let destination_translation = PaintTranslation::new(
            destination.x() - source.x(),
            destination.top_y() - source.top_y(),
        );
        Self {
            source_clip: source.paint_clip(),
            destination_clip: destination.paint_clip(),
            destination_translation,
            destination_page_clip_in_source_space: PageTopRect::new(
                input.destination_page_area.x() - destination_translation.x,
                input.destination_page_area.top_y() - destination_translation.y,
                input.destination_page_area.width(),
                input.destination_page_area.height(),
            )
            .paint_clip(),
        }
    }

    pub(in crate::layout) fn source_clip(self) -> PaintClip {
        self.source_clip
    }

    /// Exact destination-local paint clip for this logical fragment slice.
    ///
    /// The destination rectangle is retained independently from the source
    /// clip because a positioned principal first maps continuous source paint
    /// into the temporary source fragmentainer before it is translated to the
    /// committed destination.
    pub(in crate::layout) fn destination_clip(self) -> PaintClip {
        self.destination_clip
    }

    pub(in crate::layout) fn destination_translation(self) -> PaintTranslation {
        self.destination_translation
    }

    pub(in crate::layout) fn destination_page_clip_in_source_space(self) -> PaintClip {
        self.destination_page_clip_in_source_space
    }
}

fn page_top_rect_from_logical_fragment(
    axes: FlowAxes,
    origin: PageTopPoint,
    extent: LogicalSize,
    logical: LogicalRect,
) -> PageTopRect {
    let physical_extent = axes.physical_size_from_logical(extent);
    let physical = axes.rect_from_logical(
        ContainerRect::new(ContainerPoint::new(0.0, 0.0), physical_extent),
        logical,
    );
    PageTopRect::new(
        origin.x() + physical.origin.x,
        origin.top_y() - physical.origin.y,
        physical.size.width,
        physical.size.height,
    )
}

/// Whole-source prebreak decision for a keepable unit.
///
/// CSS Fragmentation may choose an unforced break before a source box or run
/// when the break avoids overflow in the current fragmentainer and the kept
/// unit fits in an empty fragmentainer. Layout modes provide the
/// source-specific sizes and the empty fragmentainer because repeated table
/// chrome or other fragment-local reservations can make fresh capacity differ
/// from the current fragmentainer's nominal block-size:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentPrebreakDecision {
    pub(in crate::layout) should_break: bool,
}

pub(in crate::layout) struct FragmentPrebreakInput {
    pub(in crate::layout) can_advance: bool,
    pub(in crate::layout) current_fragmentainer: Fragmentainer,
    pub(in crate::layout) required_block_size: LayoutLength,
    pub(in crate::layout) empty_fragmentainer: Fragmentainer,
    pub(in crate::layout) empty_fit_block_size: LayoutLength,
}

/// Decision to advance to another fragmentainer before a source unit.
///
/// CSS Fragmentation may break before a unit when it overflows, provided the
/// layout mode can make forward progress. Layout modes
/// still own target-specific transition metadata such as table repeated chrome
/// or flex source offsets; this shared decision keeps the common advance gate
/// from being restated in each mode:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentAdvanceDecision {
    pub(in crate::layout) should_advance: bool,
}

pub(in crate::layout) struct FragmentAdvanceInput {
    pub(in crate::layout) break_is_applicable: bool,
    pub(in crate::layout) overflows: bool,
    pub(in crate::layout) can_advance: bool,
}

impl Fragmentainer {
    pub(in crate::layout) fn new(
        fragmentainer_block_size: LayoutLength,
        available_block_size: LayoutLength,
    ) -> Self {
        Self {
            fragmentainer_block_size,
            available_block_size: layout_pt(available_block_size.points().max(0.0)),
        }
    }

    /// Build a fragmentainer from the current physical cursor bounds.
    ///
    /// Quire's paged-media replay currently measures remaining block capacity
    /// from the current content block-start cursor down to the fragmentainer
    /// block-end edge. The arithmetic is shared by any fragmentainer whose
    /// physical cursor uses the same block-axis coordinates; callers remain
    /// responsible for passing target-specific bounds:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn from_page_cursor_bounds(
        fragmentainer_block_size: LayoutLength,
        content_block_start: PageTopBlockPosition,
        fragmentainer_block_end: PageTopBlockPosition,
    ) -> Self {
        Self::new(
            fragmentainer_block_size,
            content_block_start.block_extent_to(fragmentainer_block_end),
        )
    }

    pub(in crate::layout) fn fragmentainer_block_size(self) -> LayoutLength {
        self.fragmentainer_block_size
    }

    pub(in crate::layout) fn available_block_size(self) -> LayoutLength {
        self.available_block_size
    }

    pub(in crate::layout) fn available_block_size_after_reservation(
        self,
        reserved_block_size: LayoutLength,
    ) -> LayoutLength {
        layout_pt((self.available_block_size.points() - reserved_block_size.points()).max(0.0))
    }

    pub(in crate::layout) fn required_block_size_overflows(self, block_size: LayoutLength) -> bool {
        block_size.points() > self.available_block_size.points() + 0.01
    }

    pub(in crate::layout) fn block_size_fits_empty(self, block_size: LayoutLength) -> bool {
        block_size.points() <= self.fragmentainer_block_size.points() + 0.01
    }
}

impl FragmentainerKind {
    pub(in crate::layout) fn is_forced_break(self, value: PageBreak) -> bool {
        match self {
            Self::Page => value.is_forced(),
            Self::Column => matches!(value, PageBreak::Column),
        }
    }

    pub(in crate::layout) fn is_avoid_break(self, value: PageBreak) -> bool {
        match self {
            Self::Page => value.avoids_page(),
            Self::Column => value.avoids_column(),
        }
    }

    /// Return whether this fragmentainer kind is currently materialized by the
    /// paged-media page cursor.
    ///
    /// CSS Fragmentation uses the same break selection model for page and
    /// column fragmentation. Quire currently has concrete cursor materialization
    /// only for pages in these replay paths; column-targeted transitions remain
    /// committed break decisions but must not mutate paged-media state:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn materializes_page_cursor(self) -> bool {
        matches!(self, Self::Page)
    }

    /// Combine break values contributed by boxes in a layout unit.
    ///
    /// CSS Fragmentation treats forced breaks as stronger than avoid breaks,
    /// and target-specific break values only apply to their matching
    /// fragmentation context. Layout modes that aggregate child break values
    /// before exposing one class A boundary use this method to keep page and
    /// column scoping consistent:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks> and
    /// <https://www.w3.org/TR/css-break-3/#break-types>.
    pub(in crate::layout) fn combine_break(
        self,
        current: PageBreak,
        candidate: PageBreak,
    ) -> PageBreak {
        if self.is_forced_break(current) {
            current
        } else if self.is_forced_break(candidate) || self.is_avoid_break(candidate) {
            candidate
        } else {
            current
        }
    }

    /// Return whether `break-inside` avoids this fragmentation context.
    ///
    /// CSS Break lets `break-inside: avoid` apply to every fragmentation
    /// context, while `avoid-page` and `avoid-column` are target-specific.
    /// Layout consumes the canonical computed value through the active
    /// fragmentainer kind so page and column fragmentation share one call
    /// shape:
    /// <https://www.w3.org/TR/css-break-3/#propdef-break-inside>.
    pub(in crate::layout) fn avoids_break_inside(self, style: &ComputedStyle) -> bool {
        match self {
            Self::Page => style.break_inside.avoids_page(),
            Self::Column => style.break_inside.avoids_column(),
        }
    }
}

impl FragmentBreakContext {
    pub(in crate::layout) fn new(
        pending_before: PageBreak,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> Self {
        Self {
            pending_before,
            before,
            after,
            next_before,
        }
    }

    /// Build a break context for a single generated box boundary.
    ///
    /// CSS Fragmentation resolves `break-before` before the generated box and
    /// `break-after` after it. Containers that are not currently carrying an
    /// adjacent sibling break use this context to keep those standalone box
    /// breaks on the same target-aware path as sibling break opportunities:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn for_standalone_box(style: &ComputedStyle) -> Self {
        Self::new(
            PageBreak::Auto,
            style.break_before,
            style.break_after,
            PageBreak::Auto,
        )
    }

    /// Return the forced break to apply before this box in the target context.
    ///
    /// CSS Fragmentation resolves multiple forced breaks at the same class A
    /// break point by taking the value latest in flow. The following box's
    /// `break-before` therefore wins over the previous box's carried
    /// `break-after` for the same fragmentainer kind:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn forced_break_before_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        if kind.is_forced_break(self.before) {
            Some(self.before)
        } else if kind.is_forced_break(self.pending_before) {
            Some(self.pending_before)
        } else {
            None
        }
    }

    pub(in crate::layout) fn forced_break_after_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        kind.is_forced_break(self.after).then_some(self.after)
    }

    pub(in crate::layout) fn forced_break_after_or_in(
        self,
        kind: FragmentainerKind,
        fallback: PageBreak,
    ) -> PageBreak {
        self.forced_break_after_in(kind).unwrap_or(fallback)
    }

    /// Returns whether the following sibling's forced `break-before` wins
    /// over this box's forced `break-after` at their shared class A boundary.
    ///
    /// CSS Fragmentation resolves forced breaks at the latest declaration in
    /// flow order, so an adjacent following box's `break-before` takes
    /// precedence over the preceding box's `break-after`:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn next_forced_break_supersedes_after_in(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.is_forced_break(self.after) && kind.is_forced_break(self.next_before)
    }

    pub(in crate::layout) fn effective_break_before_in(self, kind: FragmentainerKind) -> PageBreak {
        if let Some(forced_break) = self.forced_break_before_in(kind) {
            forced_break
        } else if kind.is_avoid_break(self.pending_before) {
            self.pending_before
        } else {
            self.before
        }
    }

    /// Return which side avoids the boundary before this box.
    ///
    /// CSS Break combines the previous box's `break-after` and this box's
    /// `break-before` at a class A opportunity. `previous_break_after` is
    /// passed separately because some layout modes keep avoid state outside
    /// forced-break carry state while planning rollback candidates.
    pub(in crate::layout) fn avoid_boundary_side_before_box_in(
        self,
        kind: FragmentainerKind,
        previous_break_after: PageBreak,
    ) -> FragmentAvoidBoundarySide {
        if kind.is_avoid_break(previous_break_after) || kind.is_avoid_break(self.pending_before) {
            FragmentAvoidBoundarySide::Previous
        } else if kind.is_avoid_break(self.before) {
            FragmentAvoidBoundarySide::Current
        } else {
            FragmentAvoidBoundarySide::None
        }
    }

    pub(in crate::layout) fn seeds_later_avoid_boundary_in(
        self,
        kind: FragmentainerKind,
        next_break_before: Option<PageBreak>,
    ) -> bool {
        self.avoid_after_in(kind).is_some()
            || next_break_before.is_some_and(|value| kind.is_avoid_break(value))
    }

    pub(in crate::layout) fn avoid_after_in(self, kind: FragmentainerKind) -> Option<PageBreak> {
        kind.is_avoid_break(self.after).then_some(self.after)
    }

    pub(in crate::layout) fn next_avoid_before_in(
        self,
        kind: FragmentainerKind,
    ) -> Option<PageBreak> {
        kind.is_avoid_break(self.next_before)
            .then_some(self.next_before)
    }

    /// Return whether this adjacent-box boundary has authored break pressure
    /// for the requested fragmentation context.
    ///
    /// This remains useful to layout modes that need to decide whether a
    /// speculative class A boundary is required even though the former simple
    /// multicol support gate no longer consumes it.
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>
    #[cfg(test)]
    pub(in crate::layout) fn needs_class_a_break_decision_in(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.is_forced_break(self.effective_break_before_in(kind))
            || kind.is_forced_break(self.after)
            || kind.is_avoid_break(self.pending_before)
            || kind.is_avoid_break(self.before)
            || kind.is_avoid_break(self.after)
            || kind.is_avoid_break(self.next_before)
    }

    pub(in crate::layout) fn seeds_later_avoid_boundary_in_context_for(
        self,
        kind: FragmentainerKind,
    ) -> bool {
        self.seeds_later_avoid_boundary_in(kind, Some(self.next_before))
    }

    pub(in crate::layout) fn forced_after_source_boxes_in(
        self,
        kind: FragmentainerKind,
        has_next_box: bool,
    ) -> Option<PageBreak> {
        (kind.is_forced_break(self.after) && !has_next_box).then_some(self.after)
    }

    pub(in crate::layout) fn forced_before_next_box_in(
        self,
        kind: FragmentainerKind,
        has_next_box: bool,
    ) -> Option<PageBreak> {
        (kind.is_forced_break(self.after) && has_next_box).then_some(self.after)
    }
}

impl FragmentAvoidRunStartDecision {
    pub(in crate::layout) fn choose(input: FragmentAvoidRunStartInput) -> Self {
        let break_boundary_avoid = input
            .break_opportunity
            .avoids_break_in(input.fragmentainer_kind);
        let next_break_before_avoid = input
            .next_break_before
            .map(|value| input.fragmentainer_kind.is_avoid_break(value));
        let is_avoid_boundary = input.participates_in_flow && break_boundary_avoid;
        let seeds_later_avoid_boundary = input.participates_in_flow
            && input
                .break_context
                .seeds_later_avoid_boundary_in(input.fragmentainer_kind, input.next_break_before);
        let should_arm_start_candidate = input.participates_in_flow
            && (input
                .break_context
                .avoid_after_in(input.fragmentainer_kind)
                .is_some()
                || next_break_before_avoid.unwrap_or(true)
                || (break_boundary_avoid && !input.has_avoid_run_candidate));
        Self {
            should_arm_start_candidate,
            is_avoid_boundary,
            seeds_later_avoid_boundary,
        }
    }
}

impl FragmentBreakOpportunity {
    /// Construct the class A boundary before a source box.
    ///
    /// CSS Fragmentation combines the previous sibling's `break-after`, the
    /// current box's `break-before`, and ancestor/current `break-inside`
    /// constraints at the boundary before the current box. Forced breaks are
    /// already carried through `FragmentBreakContext`; avoid breaks from the
    /// previous sibling remain target-scoped and are represented as the
    /// boundary's `break-after` side:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn before_box_boundary(
        kind: FragmentainerKind,
        source_block_offset: f32,
        break_context: FragmentBreakContext,
        previous_break_after: PageBreak,
        break_inside_avoid: bool,
    ) -> Self {
        let effective_break_before = break_context.effective_break_before_in(kind);
        Self {
            source_block_offset,
            break_before: if kind.is_forced_break(effective_break_before)
                || kind.is_avoid_break(effective_break_before)
            {
                effective_break_before
            } else {
                PageBreak::Auto
            },
            break_after: if kind.is_avoid_break(previous_break_after) {
                previous_break_after
            } else {
                PageBreak::Auto
            },
            break_inside_avoid,
        }
    }

    pub(in crate::layout) fn has_forced_break_in(self, kind: FragmentainerKind) -> bool {
        kind.is_forced_break(self.break_before) || kind.is_forced_break(self.break_after)
    }

    pub(in crate::layout) fn avoids_break_in(self, kind: FragmentainerKind) -> bool {
        self.break_inside_avoid
            || kind.is_avoid_break(self.break_before)
            || kind.is_avoid_break(self.break_after)
    }

    pub(in crate::layout) fn first_forced_in(
        search: FragmentBreakOpportunitySearch<'_>,
    ) -> Option<Self> {
        search
            .opportunities_in_fragmentainer()
            .filter(|opportunity| opportunity.has_forced_break_in(search.fragmentainer_kind))
            .min_by(|a, b| a.source_block_offset.total_cmp(&b.source_block_offset))
    }

    pub(in crate::layout) fn latest_unforced_in(
        search: FragmentBreakOpportunitySearch<'_>,
        allow_avoids: bool,
    ) -> Option<Self> {
        search
            .opportunities_in_fragmentainer()
            .filter(|opportunity| {
                allow_avoids || !opportunity.avoids_break_in(search.fragmentainer_kind)
            })
            .max_by(|a, b| a.source_block_offset.total_cmp(&b.source_block_offset))
    }

    pub(in crate::layout) fn latest_unforced_preferring_allowed_in(
        search: FragmentBreakOpportunitySearch<'_>,
    ) -> Option<Self> {
        Self::latest_unforced_in(search, false).or_else(|| Self::latest_unforced_in(search, true))
    }
}

impl<'a> FragmentBreakOpportunitySearch<'a> {
    fn opportunities_in_fragmentainer(self) -> impl Iterator<Item = FragmentBreakOpportunity> + 'a {
        self.opportunities
            .iter()
            .cloned()
            .filter(move |opportunity| {
                opportunity.source_block_offset > self.source_block_start + 0.01
                    && opportunity.source_block_offset <= self.available_block_end + 0.01
                    && opportunity.source_block_offset < self.content_block_end - 0.01
            })
    }
}

impl FragmentSourceSliceDecision {
    pub(in crate::layout) fn choose(input: FragmentSourceSliceInput) -> Self {
        let slice_end = if input.break_is_applicable
            && input.source_is_oversized
            && input.source_block_end > input.available_block_end + 0.01
        {
            input
                .available_block_end
                .min(input.source_block_end)
                .max(input.slice_start)
        } else {
            input.source_block_end
        };
        let advance_before_slice = input.break_is_applicable
            && slice_end <= input.slice_start + 0.01
            && input.source_block_end > input.slice_start + 0.01;
        Self {
            slice_start: input.slice_start,
            slice_end,
            advance_before_slice,
        }
    }

    pub(in crate::layout) fn paints_slice(self) -> bool {
        !self.advance_before_slice
    }
}

impl FragmentPrebreakDecision {
    pub(in crate::layout) fn choose(input: FragmentPrebreakInput) -> Self {
        let should_break = input.can_advance
            && input
                .current_fragmentainer
                .required_block_size_overflows(input.required_block_size)
            && input
                .empty_fragmentainer
                .block_size_fits_empty(input.empty_fit_block_size);
        Self { should_break }
    }
}

impl FragmentAdvanceDecision {
    pub(in crate::layout) fn choose(input: FragmentAdvanceInput) -> Self {
        Self {
            should_advance: input.break_is_applicable
                && input.can_advance
                // `avoid` restricts which unforced boundary may be selected
                // after content overflows; it never creates a fragmentation
                // break on its own.  Advancing merely because a heading asks
                // to avoid a following break strands otherwise-fitting flex
                // lines on later pages.
                // <https://www.w3.org/TR/css-break-3/#avoid-breaks>
                && input.overflows,
        }
    }
}

impl Default for ForcedBreakCarryState {
    fn default() -> Self {
        Self::new(FragmentainerKind::Page)
    }
}

impl ForcedBreakCarryState {
    pub(in crate::layout) fn new(fragmentainer_kind: FragmentainerKind) -> Self {
        Self {
            fragmentainer_kind,
            before_next_box: PageBreak::Auto,
            after_source_boxes: PageBreak::Auto,
        }
    }

    fn box_context(
        self,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> FragmentBreakContext {
        FragmentBreakContext::new(self.before_next_box, before, after, next_before)
    }

    pub(in crate::layout) fn take_box_context(
        &mut self,
        before: PageBreak,
        after: PageBreak,
        next_before: PageBreak,
    ) -> FragmentBreakContext {
        let context = self.box_context(before, after, next_before);
        self.clear_before_next_box();
        context
    }

    fn clear_before_next_box(&mut self) {
        self.before_next_box = PageBreak::Auto;
    }

    pub(in crate::layout) fn finish_box(
        &mut self,
        break_context: FragmentBreakContext,
        has_next_box: bool,
    ) {
        if let Some(forced_before_next_box) =
            break_context.forced_before_next_box_in(self.fragmentainer_kind, has_next_box)
        {
            self.before_next_box = forced_before_next_box;
        }
        self.after_source_boxes = break_context
            .forced_after_source_boxes_in(self.fragmentainer_kind, has_next_box)
            .unwrap_or(PageBreak::Auto);
    }

    pub(in crate::layout) fn outgoing_source_break(self) -> PageBreak {
        self.after_source_boxes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::block::{
        AvoidRunPrebreakInput, AvoidRunRetryContext, AvoidRunSourceFragmentainerOccupancy,
        should_move_avoid_break_run_to_next_fragmentainer,
    };

    fn fragmentainer(block_size: f32, available_size: f32) -> Fragmentainer {
        Fragmentainer::new(layout_pt(block_size), layout_pt(available_size))
    }

    #[test]
    fn cloned_container_fragment_always_owns_both_block_edges() {
        let fragment = CommittedContainerFragment::principal(
            FragmentainerOrdinal::new(3),
            "source slice",
            PaintClip::new(10.0, 20.0, 30.0, 0.0),
            FragmentDecoration::for_box_decoration_break(
                css::BoxDecorationBreak::Clone,
                false,
                false,
            ),
        );

        let principal = fragment
            .kind()
            .principal_box()
            .expect("clone creates a principal box fragment");
        assert!(principal.decoration().owns_block_start());
        assert!(principal.decoration().owns_block_end());
        assert_eq!(
            principal.border_box(),
            PaintClip::new(10.0, 20.0, 30.0, 0.0)
        );
    }

    #[test]
    fn sliced_container_fragment_retains_only_its_owned_edges() {
        let fragment = CommittedContainerFragment::principal(
            FragmentainerOrdinal::new(1),
            (),
            PaintClip::new(0.0, 0.0, 10.0, 20.0),
            FragmentDecoration::for_box_decoration_break(
                css::BoxDecorationBreak::Slice,
                true,
                false,
            ),
        );

        let decoration = fragment
            .kind()
            .principal_box()
            .expect("slice still owns a principal box")
            .decoration();
        assert!(decoration.owns_block_start());
        assert!(!decoration.owns_block_end());
    }

    #[test]
    fn descendant_overflow_fragment_has_no_decoration_geometry() {
        let fragment = CommittedContainerFragment::descendant_overflow_only(
            FragmentainerOrdinal::new(2),
            42_usize,
        );

        assert_eq!(fragment.fragmentainer(), FragmentainerOrdinal::new(2));
        assert_eq!(fragment.source_slice(), &42);
        assert!(fragment.kind().principal_box().is_none());
    }

    fn test_layout_builder<'a, Collection: crate::css::StylesheetCollection + ?Sized>(
        options: &'a RenderOptions,
        stylesheets: &'a Collection,
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        let stylesheets = crate::css::StylesheetCollection::stylesheet_view(stylesheets);
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            target_references: crate::layout::TargetReferenceSnapshot::default(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn fragmentainer_advance_materializes_unforced_and_forced_page_transitions() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);

        builder.mark_current_page_flow_content();
        let page_count = builder.pages.len();
        let content_top = builder
            .materialize_fragmentainer_advance(
                FragmentainerKind::Page,
                FragmentainerAdvance::Unforced,
            )
            .expect("page fragmentainer materializes a cursor");
        assert_eq!(builder.pages.len(), page_count + 1);
        assert_eq!(content_top, builder.cursor_y);

        builder.mark_current_page_flow_content();
        let page_count = builder.pages.len();
        let content_top = builder
            .materialize_fragmentainer_advance(
                FragmentainerKind::Page,
                FragmentainerAdvance::Forced(PageBreak::Page),
            )
            .expect("forced page break materializes a cursor");
        assert_eq!(builder.pages.len(), page_count + 1);
        assert_eq!(content_top, builder.cursor_y);
    }

    #[test]
    fn fragmentainer_advance_leaves_nonmaterialized_cursor_unchanged() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let page_count = builder.pages.len();
        let cursor_y = builder.cursor_y;

        assert_eq!(
            builder.materialize_fragmentainer_advance(
                FragmentainerKind::Column,
                FragmentainerAdvance::Unforced,
            ),
            None
        );
        assert_eq!(builder.pages.len(), page_count);
        assert_eq!(builder.cursor_y, cursor_y);
    }

    #[test]
    fn fragmentainer_advance_materializes_column_override_continuations() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial_context = builder.current_page_context;
        builder.fragmentainer_override = Some(FragmentainerOverride {
            kind: FragmentainerKind::Column,
            initial_context,
            initial_fragmentainer_count: 1,
            context: initial_context,
            relax_widows_orphans: false,
        });
        builder.mark_current_page_flow_content();
        let page_count = builder.pages.len();

        let content_top = builder
            .materialize_fragmentainer_advance(
                FragmentainerKind::Column,
                FragmentainerAdvance::Unforced,
            )
            .expect("column override materializes its temporary page cursor");

        assert_eq!(builder.pages.len(), page_count + 1);
        assert_eq!(content_top, builder.cursor_y);
    }

    #[test]
    fn forced_column_break_retains_an_empty_source_column() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial_context = builder.current_page_context;
        builder.fragmentainer_override = Some(FragmentainerOverride {
            kind: FragmentainerKind::Column,
            initial_context,
            initial_fragmentainer_count: 1,
            context: initial_context,
            relax_widows_orphans: false,
        });
        let page_count = builder.pages.len();

        builder.apply_forced_break_in(FragmentainerKind::Column, PageBreak::Column);

        assert_eq!(builder.pages.len(), page_count + 1);
        assert!(!builder.current_page_has_content());
    }

    #[test]
    fn vertical_root_page_fragment_slices_project_in_opposite_block_directions() {
        let context = PageContext {
            size: PageSize::from_points(360.0, 216.0),
            margins: PageMargins::all_points(36.0),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        };
        let source_extent = LogicalSize {
            inline: 72.0,
            block: context.logical_block_size(WritingMode::VerticalRl),
        };
        let source_slice = LogicalRect {
            origin: LogicalPoint {
                inline: 0.0,
                block: 0.0,
            },
            size: LogicalSize {
                inline: source_extent.inline,
                block: 100.0,
            },
        };
        let input = |writing_mode| FragmentainerProjectionInput {
            source_axes: FlowAxes::new(writing_mode, Direction::Ltr),
            source_origin: PageTopPoint::new(context.left(), context.top()),
            source_extent,
            source_slice,
            destination_axes: FlowAxes::new(writing_mode, Direction::Ltr),
            destination_origin: PageTopPoint::new(context.left(), context.top()),
            destination_extent: source_extent,
            destination_slice: source_slice,
            destination_page_area: PageTopRect::new(
                context.left(),
                context.top(),
                context.area_width(),
                context.area_height(),
            ),
        };

        let vertical_rl = FragmentainerProjection::new(input(WritingMode::VerticalRl));
        let vertical_lr = FragmentainerProjection::new(input(WritingMode::VerticalLr));

        assert_eq!(
            vertical_rl.destination_clip().paint_rect().origin.x,
            context.right() - 100.0
        );
        assert_eq!(
            vertical_lr.destination_clip().paint_rect().origin.x,
            context.left()
        );
        assert_eq!(
            vertical_rl.destination_clip().paint_rect().size.width,
            100.0
        );
        assert_eq!(
            vertical_lr.destination_clip().paint_rect().size.width,
            100.0
        );
    }

    #[test]
    fn root_page_block_slices_keep_fixed_and_overflow_ranges_separate() {
        let principal = root_page_block_slices(layout_pt(360.0), layout_pt(144.0));
        let overflow = root_page_block_slices(layout_pt(576.0), layout_pt(144.0));

        assert_eq!(
            principal,
            vec![
                RootPageBlockSlice {
                    source_block_start: layout_pt(0.0),
                    block_size: layout_pt(144.0),
                },
                RootPageBlockSlice {
                    source_block_start: layout_pt(144.0),
                    block_size: layout_pt(144.0),
                },
                RootPageBlockSlice {
                    source_block_start: layout_pt(288.0),
                    block_size: layout_pt(72.0),
                },
            ]
        );
        assert_eq!(overflow.len(), 4);
        assert_eq!(overflow[2].source_block_start, layout_pt(288.0));
        assert_eq!(overflow[3].source_block_start, layout_pt(432.0));
    }

    #[test]
    fn fragmentainer_capacity_uses_empty_and_remaining_block_sizes() {
        let fragmentainer = fragmentainer(100.0, 40.0);

        assert_eq!(fragmentainer.fragmentainer_block_size(), layout_pt(100.0));
        assert_eq!(fragmentainer.available_block_size(), layout_pt(40.0));
        assert!(fragmentainer.block_size_fits_empty(layout_pt(100.0)));
        assert!(!fragmentainer.block_size_fits_empty(layout_pt(101.0)));
        assert!(fragmentainer.required_block_size_overflows(layout_pt(41.0)));
        assert_eq!(
            fragmentainer.available_block_size_after_reservation(layout_pt(15.0)),
            layout_pt(25.0)
        );
        assert_eq!(
            fragmentainer.available_block_size_after_reservation(layout_pt(80.0)),
            layout_pt(0.0)
        );
    }

    #[test]
    fn decoration_reservation_models_clone_and_slice_content_capacity() {
        let clone = FragmentDecorationReservation::new(
            FragmentDecoration::Clone,
            non_content_pt(7.5),
            non_content_pt(7.5),
        );
        assert_eq!(
            clone.remaining_content_extent(layout_pt(67.5)),
            layout_pt(60.0)
        );
        assert_eq!(clone.fresh_content_extent(layout_pt(75.0)), layout_pt(60.0));
        assert_eq!(clone.fresh_content_extent(layout_pt(10.0)), layout_pt(0.0));

        let first_slice = FragmentDecorationReservation::new(
            FragmentDecoration::Slice(SliceFragmentEdges {
                owns_block_start: true,
                owns_block_end: false,
            }),
            non_content_pt(7.5),
            non_content_pt(7.5),
        );
        assert_eq!(
            first_slice.fresh_content_extent(layout_pt(75.0)),
            layout_pt(67.5)
        );

        let last_slice = FragmentDecorationReservation::new(
            FragmentDecoration::Slice(SliceFragmentEdges {
                owns_block_start: false,
                owns_block_end: true,
            }),
            non_content_pt(7.5),
            non_content_pt(7.5),
        );
        assert_eq!(
            last_slice.remaining_content_extent(layout_pt(67.5)),
            layout_pt(60.0)
        );
    }

    #[test]
    fn fragmentainer_capacity_derives_remaining_size_from_page_cursor_bounds() {
        let fragmentainer = Fragmentainer::from_page_cursor_bounds(
            layout_pt(200.0),
            PageTopBlockPosition::new(640.0),
            PageTopBlockPosition::new(500.0),
        );

        assert_eq!(fragmentainer.fragmentainer_block_size(), layout_pt(200.0));
        assert_eq!(fragmentainer.available_block_size(), layout_pt(140.0));
    }

    #[test]
    fn fragmentainer_capacity_clamps_when_page_cursor_is_past_block_end() {
        let fragmentainer = Fragmentainer::from_page_cursor_bounds(
            layout_pt(200.0),
            PageTopBlockPosition::new(480.0),
            PageTopBlockPosition::new(500.0),
        );

        assert_eq!(fragmentainer.available_block_size(), layout_pt(0.0));
    }

    #[test]
    fn source_slice_paints_available_oversized_piece() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: true,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 75.0,
        });

        assert!(decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 75.0);
        assert!(!decision.advance_before_slice);
    }

    #[test]
    fn source_slice_advances_when_no_progress_is_possible() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: true,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 40.0,
        });

        assert!(!decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 40.0);
        assert!(decision.advance_before_slice);
    }

    #[test]
    fn source_slice_keeps_unfragmented_end_when_breaks_are_not_applicable() {
        let decision = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: false,
            source_is_oversized: true,
            source_block_end: 120.0,
            slice_start: 40.0,
            available_block_end: 75.0,
        });

        assert!(decision.paints_slice());
        assert_eq!(decision.slice_start, 40.0);
        assert_eq!(decision.slice_end, 120.0);
        assert!(!decision.advance_before_slice);
    }

    #[test]
    fn prebreak_moves_keepable_unit_that_overflows_remaining_space() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: fragmentainer(100.0, 40.0),
            required_block_size: layout_pt(60.0),
            empty_fragmentainer: fragmentainer(100.0, 100.0),
            empty_fit_block_size: layout_pt(80.0),
        });

        assert!(decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_unit_fits_remaining_space() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: fragmentainer(100.0, 40.0),
            required_block_size: layout_pt(40.0),
            empty_fragmentainer: fragmentainer(100.0, 100.0),
            empty_fit_block_size: layout_pt(80.0),
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_kept_unit_cannot_fit_empty_fragmentainer() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: fragmentainer(100.0, 40.0),
            required_block_size: layout_pt(60.0),
            empty_fragmentainer: fragmentainer(100.0, 100.0),
            empty_fit_block_size: layout_pt(120.0),
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_uses_explicit_empty_fragmentainer_capacity() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer: fragmentainer(100.0, 40.0),
            required_block_size: layout_pt(60.0),
            empty_fragmentainer: fragmentainer(50.0, 50.0),
            empty_fit_block_size: layout_pt(80.0),
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn prebreak_stays_when_fragmentainer_cannot_advance() {
        let decision = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: false,
            current_fragmentainer: fragmentainer(100.0, 40.0),
            required_block_size: layout_pt(60.0),
            empty_fragmentainer: fragmentainer(100.0, 100.0),
            empty_fit_block_size: layout_pt(80.0),
        });

        assert!(!decision.should_break);
    }

    #[test]
    fn avoid_run_prebreak_rejects_equal_capacity_empty_fragmentainer_retry() {
        assert!(!should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(20.0),
                next_block_extent: layout_pt(60.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(100.0, 40.0),
                    empty_destination_fragmentainer: fragmentainer(100.0, 100.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Empty,
                },
            }
        ));
    }

    #[test]
    fn avoid_run_prebreak_uses_strictly_larger_empty_destination() {
        assert!(should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(20.0),
                next_block_extent: layout_pt(60.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(100.0, 40.0),
                    empty_destination_fragmentainer: fragmentainer(120.0, 120.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Empty,
                },
            }
        ));
    }

    #[test]
    fn avoid_run_prebreak_keeps_oversized_run_in_ordinary_fragmentation() {
        assert!(!should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(80.0),
                next_block_extent: layout_pt(60.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(100.0, 40.0),
                    empty_destination_fragmentainer: fragmentainer(120.0, 120.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Empty,
                },
            }
        ));
    }

    #[test]
    fn avoid_run_prebreak_keeps_occupied_source_behavior() {
        assert!(should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(20.0),
                next_block_extent: layout_pt(60.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(100.0, 40.0),
                    empty_destination_fragmentainer: fragmentainer(100.0, 100.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Occupied,
                },
            }
        ));
    }

    #[test]
    fn avoid_run_prebreak_accepts_a_logical_min_block_extent_in_its_destination() {
        // In vertical writing the caller projects `min-block-size: 40px` to
        // this fragmentainer-block extent (physical X). The current 30px
        // column cannot retain the child, while the empty 40px destination
        // can, so the class-A avoid retry is selected.
        assert!(should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(0.0),
                next_block_extent: layout_pt(40.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(30.0, 30.0),
                    empty_destination_fragmentainer: fragmentainer(40.0, 40.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Occupied,
                },
            }
        ));
    }

    #[test]
    fn avoid_run_prebreak_relaxes_an_oversized_logical_min_block_extent() {
        // No empty column can contain the 40px logical block extent. Avoidance
        // must not manufacture a run of empty columns instead of allowing
        // normal fragmentation to make progress.
        assert!(!should_move_avoid_break_run_to_next_fragmentainer(
            AvoidRunPrebreakInput {
                run_block_extent: layout_pt(0.0),
                next_block_extent: layout_pt(40.0),
                retry_context: AvoidRunRetryContext {
                    current_fragmentainer: fragmentainer(30.0, 30.0),
                    empty_destination_fragmentainer: fragmentainer(30.0, 30.0),
                    source_occupancy: AvoidRunSourceFragmentainerOccupancy::Occupied,
                },
            }
        ));
    }

    #[test]
    fn advance_decision_moves_only_for_overflow() {
        assert!(
            FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: true,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: false,
                can_advance: true,
            })
            .should_advance
        );
    }

    #[test]
    fn advance_decision_stays_without_applicable_break_or_pressure() {
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: false,
                overflows: true,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: false,
                can_advance: true,
            })
            .should_advance
        );
        assert!(
            !FragmentAdvanceDecision::choose(FragmentAdvanceInput {
                break_is_applicable: true,
                overflows: true,
                can_advance: false,
            })
            .should_advance
        );
    }

    #[test]
    fn break_opportunity_selects_first_forced_boundary_for_target_fragmentainer() {
        let opportunities = [
            FragmentBreakOpportunity {
                source_block_offset: 40.0,
                break_before: PageBreak::Column,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 80.0,
                break_before: PageBreak::Page,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 120.0,
                break_before: PageBreak::Left,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
        ];

        let page_search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Page,
            opportunities: &opportunities,
            source_block_start: 0.0,
            available_block_end: 150.0,
            content_block_end: 200.0,
        };
        let column_search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Column,
            ..page_search
        };

        assert_eq!(
            FragmentBreakOpportunity::first_forced_in(page_search)
                .map(|boundary| { boundary.source_block_offset }),
            Some(80.0)
        );
        assert_eq!(
            FragmentBreakOpportunity::first_forced_in(column_search)
                .map(|boundary| { boundary.source_block_offset }),
            Some(40.0)
        );
    }

    #[test]
    fn fragmentainer_break_combiner_scopes_forced_and_avoid_values() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Column
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::AvoidColumn
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Left, PageBreak::Page),
            PageBreak::Left
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Column, PageBreak::Avoid),
            PageBreak::Column
        );
    }

    #[test]
    fn fragmentainer_kind_page_cursor_materialization_is_target_specific() {
        assert!(FragmentainerKind::Page.materializes_page_cursor());
        assert!(!FragmentainerKind::Column.materializes_page_cursor());
    }

    #[test]
    fn avoid_boundary_side_preserves_boundary_source_precedence() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::Current
        );
        assert_eq!(
            context
                .avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::AvoidPage),
            FragmentAvoidBoundarySide::Previous
        );
        assert_eq!(
            FragmentBreakContext::new(
                PageBreak::AvoidPage,
                PageBreak::AvoidPage,
                PageBreak::Auto,
                PageBreak::Auto,
            )
            .avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::Previous
        );
    }

    #[test]
    fn avoid_boundary_side_scopes_avoid_values_to_fragmentainer() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Page, PageBreak::Auto),
            FragmentAvoidBoundarySide::None
        );
        assert_eq!(
            context.avoid_boundary_side_before_box_in(FragmentainerKind::Column, PageBreak::Auto),
            FragmentAvoidBoundarySide::Current
        );
    }

    #[test]
    fn break_context_returns_target_specific_avoid_values() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::AvoidPage,
        );

        assert_eq!(context.avoid_after_in(FragmentainerKind::Page), None);
        assert_eq!(
            context.avoid_after_in(FragmentainerKind::Column),
            Some(PageBreak::AvoidColumn)
        );
        assert_eq!(
            context.next_avoid_before_in(FragmentainerKind::Page),
            Some(PageBreak::AvoidPage)
        );
        assert_eq!(
            context.next_avoid_before_in(FragmentainerKind::Column),
            None
        );
    }

    #[test]
    fn before_box_break_opportunity_preserves_target_specific_previous_avoid() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let page_opportunity = FragmentBreakOpportunity::before_box_boundary(
            FragmentainerKind::Page,
            40.0,
            context,
            PageBreak::AvoidColumn,
            false,
        );
        let column_opportunity = FragmentBreakOpportunity::before_box_boundary(
            FragmentainerKind::Column,
            40.0,
            context,
            PageBreak::AvoidColumn,
            false,
        );

        assert!(page_opportunity.avoids_break_in(FragmentainerKind::Page));
        assert!(!page_opportunity.avoids_break_in(FragmentainerKind::Column));
        assert!(column_opportunity.avoids_break_in(FragmentainerKind::Column));
        assert!(!column_opportunity.avoids_break_in(FragmentainerKind::Page));
    }

    #[test]
    fn avoid_run_start_decision_consumes_target_specific_break_opportunity() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 40.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::AvoidColumn,
            break_inside_avoid: false,
        };

        let page_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: FragmentainerKind::Page,
            break_context: context,
            break_opportunity: opportunity,
            next_break_before: Some(PageBreak::Auto),
            has_avoid_run_candidate: false,
        });
        let column_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            fragmentainer_kind: FragmentainerKind::Column,
            ..FragmentAvoidRunStartInput {
                participates_in_flow: true,
                fragmentainer_kind: FragmentainerKind::Page,
                break_context: context,
                break_opportunity: opportunity,
                next_break_before: Some(PageBreak::Auto),
                has_avoid_run_candidate: false,
            }
        });

        assert!(!page_decision.is_avoid_boundary);
        assert!(!page_decision.should_arm_start_candidate);
        assert!(column_decision.is_avoid_boundary);
        assert!(column_decision.should_arm_start_candidate);
    }

    #[test]
    fn avoid_run_start_decision_scopes_next_break_before_to_fragmentainer() {
        let context = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 40.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
        };

        let page_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: FragmentainerKind::Page,
            break_context: context,
            break_opportunity: opportunity,
            next_break_before: Some(PageBreak::AvoidColumn),
            has_avoid_run_candidate: false,
        });
        let column_decision = FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            fragmentainer_kind: FragmentainerKind::Column,
            ..FragmentAvoidRunStartInput {
                participates_in_flow: true,
                fragmentainer_kind: FragmentainerKind::Page,
                break_context: context,
                break_opportunity: opportunity,
                next_break_before: Some(PageBreak::AvoidColumn),
                has_avoid_run_candidate: false,
            }
        });

        assert!(!page_decision.seeds_later_avoid_boundary);
        assert!(!page_decision.should_arm_start_candidate);
        assert!(column_decision.seeds_later_avoid_boundary);
        assert!(column_decision.should_arm_start_candidate);
    }

    #[test]
    fn break_opportunity_prefers_latest_non_avoid_boundary_before_avoidable_boundary() {
        let opportunities = [
            FragmentBreakOpportunity {
                source_block_offset: 40.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 80.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::AvoidPage,
                break_inside_avoid: false,
            },
            FragmentBreakOpportunity {
                source_block_offset: 120.0,
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: true,
            },
        ];
        let search = FragmentBreakOpportunitySearch {
            fragmentainer_kind: FragmentainerKind::Page,
            opportunities: &opportunities,
            source_block_start: 0.0,
            available_block_end: 150.0,
            content_block_end: 200.0,
        };

        assert_eq!(
            FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(search)
                .map(|boundary| boundary.source_block_offset),
            Some(40.0)
        );
        assert_eq!(
            FragmentBreakOpportunity::latest_unforced_preferring_allowed_in(
                FragmentBreakOpportunitySearch {
                    source_block_start: 40.0,
                    ..search
                },
            )
            .map(|boundary| boundary.source_block_offset),
            Some(120.0)
        );
    }

    #[test]
    fn target_specific_break_context_keeps_page_and_column_values_separate() {
        let page_context = FragmentBreakContext::new(
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Page,
            PageBreak::Auto,
        );
        assert!(page_context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(!page_context.needs_class_a_break_decision_in(FragmentainerKind::Column));

        let column_context = FragmentBreakContext::new(
            PageBreak::AvoidColumn,
            PageBreak::Auto,
            PageBreak::Column,
            PageBreak::Auto,
        );
        assert!(!column_context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(column_context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn effective_break_before_ignores_other_fragmentainer_pending_breaks() {
        let context = FragmentBreakContext::new(
            PageBreak::Column,
            PageBreak::Page,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Page),
            PageBreak::Page
        );
        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Column),
            PageBreak::Column
        );
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn forced_break_before_uses_latest_break_at_boundary() {
        let context = FragmentBreakContext::new(
            PageBreak::Page,
            PageBreak::Left,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Page),
            Some(PageBreak::Left)
        );
        assert_eq!(
            context.effective_break_before_in(FragmentainerKind::Page),
            PageBreak::Left
        );
    }

    #[test]
    fn standalone_box_break_context_scopes_forced_box_boundaries() {
        let mut style = ComputedStyle::initial();
        style.break_before = PageBreak::Column;
        style.break_after = PageBreak::Page;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Page),
            None
        );
        assert_eq!(
            context.forced_break_before_in(FragmentainerKind::Column),
            Some(PageBreak::Column)
        );
        assert_eq!(
            context.forced_break_after_in(FragmentainerKind::Page),
            Some(PageBreak::Page)
        );
        assert_eq!(
            context.forced_break_after_in(FragmentainerKind::Column),
            None
        );
    }

    #[test]
    fn standalone_box_break_after_can_fall_back_to_descendant_outgoing_break() {
        let mut style = ComputedStyle::initial();
        style.break_after = PageBreak::Auto;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_after_or_in(FragmentainerKind::Page, PageBreak::Left),
            PageBreak::Left
        );

        style.break_after = PageBreak::Right;
        let context = FragmentBreakContext::for_standalone_box(&style);

        assert_eq!(
            context.forced_break_after_or_in(FragmentainerKind::Page, PageBreak::Left),
            PageBreak::Right
        );
    }

    #[test]
    fn generic_avoid_applies_to_every_fragmentainer_kind() {
        let context = FragmentBreakContext::new(
            PageBreak::Avoid,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Page));
        assert!(context.needs_class_a_break_decision_in(FragmentainerKind::Column));
    }

    #[test]
    fn break_inside_avoid_is_target_specific() {
        let mut style = ComputedStyle::initial();
        style.break_inside = css::BreakInsideAvoidance::AvoidPage;

        assert!(FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(!FragmentainerKind::Column.avoids_break_inside(&style));

        style.break_inside = css::BreakInsideAvoidance::AvoidColumn;

        assert!(!FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(FragmentainerKind::Column.avoids_break_inside(&style));

        style.break_inside = css::BreakInsideAvoidance::Avoid;

        assert!(FragmentainerKind::Page.avoids_break_inside(&style));
        assert!(FragmentainerKind::Column.avoids_break_inside(&style));
    }

    #[test]
    fn forced_break_carry_is_target_specific() {
        let mut page_carry = ForcedBreakCarryState::new(FragmentainerKind::Page);
        let page_context =
            page_carry.take_box_context(PageBreak::Auto, PageBreak::Column, PageBreak::Auto);
        page_carry.finish_box(page_context, true);
        let next_page_context =
            page_carry.take_box_context(PageBreak::Auto, PageBreak::Auto, PageBreak::Auto);
        assert_eq!(next_page_context.pending_before, PageBreak::Auto);

        let mut column_carry = ForcedBreakCarryState::new(FragmentainerKind::Column);
        let column_context =
            column_carry.take_box_context(PageBreak::Auto, PageBreak::Column, PageBreak::Auto);
        column_carry.finish_box(column_context, true);
        let next_column_context =
            column_carry.take_box_context(PageBreak::Auto, PageBreak::Auto, PageBreak::Auto);
        assert_eq!(next_column_context.pending_before, PageBreak::Column);

        let mut outgoing_column_carry = ForcedBreakCarryState::new(FragmentainerKind::Column);
        let outgoing_context = outgoing_column_carry.take_box_context(
            PageBreak::Auto,
            PageBreak::Column,
            PageBreak::Auto,
        );
        outgoing_column_carry.finish_box(outgoing_context, false);
        assert_eq!(
            outgoing_column_carry.outgoing_source_break(),
            PageBreak::Column
        );
    }

    #[test]
    fn column_continuation_plan_materializes_normal_runs_exactly() {
        let plan = column_continuation_materialization_with_limit(
            layout_pt(250.0),
            layout_pt(100.0),
            1,
            MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS,
        );

        assert_eq!(plan.pages_to_push, 3);
        assert_eq!(plan.last_fragment_used_block_size, layout_pt(50.0));
        assert!(!plan.has_unmaterialized_tail);
    }

    #[test]
    fn column_continuation_plan_keeps_empty_runs_empty() {
        let plan = column_continuation_materialization_with_limit(
            layout_pt(0.0),
            layout_pt(100.0),
            1,
            MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS,
        );

        assert_eq!(plan.pages_to_push, 0);
        assert_eq!(plan.last_fragment_used_block_size, layout_pt(0.0));
        assert!(!plan.has_unmaterialized_tail);
    }

    #[test]
    fn column_continuation_plan_bounds_extreme_authored_lengths() {
        let plan = column_continuation_materialization_with_limit(
            layout_pt(1_000_000_000.0),
            layout_pt(100.0),
            1,
            MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS,
        );

        assert_eq!(
            plan.pages_to_push,
            MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS - 1
        );
        assert!(plan.last_fragment_used_block_size > layout_pt(0.0));
        assert!(plan.last_fragment_used_block_size <= layout_pt(100.0));
        assert!(plan.has_unmaterialized_tail);
    }

    #[test]
    fn fragmentainer_projection_preserves_horizontal_column_replay_coordinates() {
        let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
            source_axes: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            source_origin: PageTopPoint::new(10.0, 200.0),
            source_extent: LogicalSize {
                inline: 100.0,
                block: 200.0,
            },
            source_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 0.0,
                    block: 100.0,
                },
                size: LogicalSize {
                    inline: 100.0,
                    block: 100.0,
                },
            },
            destination_axes: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            destination_origin: PageTopPoint::new(10.0, 200.0),
            destination_extent: LogicalSize {
                inline: 400.0,
                block: 100.0,
            },
            destination_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 100.0,
                    block: 0.0,
                },
                size: LogicalSize {
                    inline: 100.0,
                    block: 100.0,
                },
            },
            destination_page_area: PageTopRect::new(0.0, 300.0, 400.0, 300.0),
        });

        assert_eq!(
            projection.source_clip(),
            PageTopRect::new(10.0, 100.0, 100.0, 100.0).paint_clip()
        );
        assert_eq!(
            projection.destination_translation(),
            PaintTranslation::new(100.0, 100.0)
        );
        assert_eq!(
            projection.destination_clip(),
            PageTopRect::new(110.0, 200.0, 100.0, 100.0).paint_clip(),
            "destination clipping remains authoritative instead of being reconstructed from source geometry",
        );
    }

    #[test]
    fn fragmentainer_projection_maps_source_and_destination_with_independent_axes() {
        let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
            source_axes: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            source_origin: PageTopPoint::new(10.0, 200.0),
            source_extent: LogicalSize {
                inline: 100.0,
                block: 40.0,
            },
            source_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 0.0,
                    block: 0.0,
                },
                size: LogicalSize {
                    inline: 100.0,
                    block: 40.0,
                },
            },
            destination_axes: FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr),
            destination_origin: PageTopPoint::new(10.0, 200.0),
            destination_extent: LogicalSize {
                inline: 100.0,
                block: 40.0,
            },
            destination_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 0.0,
                    block: 0.0,
                },
                size: LogicalSize {
                    inline: 100.0,
                    block: 40.0,
                },
            },
            destination_page_area: PageTopRect::new(0.0, 300.0, 300.0, 300.0),
        });

        assert_eq!(
            projection.source_clip(),
            PageTopRect::new(10.0, 200.0, 100.0, 40.0).paint_clip()
        );
        assert_eq!(
            projection.destination_clip(),
            PageTopRect::new(10.0, 200.0, 40.0, 100.0).paint_clip()
        );
        assert_eq!(
            projection.destination_translation(),
            PaintTranslation::identity(),
            "independent axis projection changes the destination extent, not its shared logical origin",
        );
    }

    #[test]
    fn fragmentainer_projection_maps_vertical_rl_rtl_block_slices_to_x() {
        let source_slice = LogicalRect {
            origin: LogicalPoint {
                inline: 0.0,
                block: 20.0,
            },
            size: LogicalSize {
                inline: 100.0,
                block: 40.0,
            },
        };
        let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
            source_axes: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            source_origin: PageTopPoint::new(10.0, 500.0),
            source_extent: LogicalSize {
                inline: 400.0,
                block: 100.0,
            },
            source_slice,
            destination_axes: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            destination_origin: PageTopPoint::new(10.0, 500.0),
            destination_extent: LogicalSize {
                inline: 400.0,
                block: 100.0,
            },
            destination_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 100.0,
                    block: 20.0,
                },
                size: source_slice.size,
            },
            destination_page_area: PageTopRect::new(0.0, 600.0, 100.0, 400.0),
        });

        assert_eq!(
            projection.source_clip(),
            PageTopRect::new(50.0, 200.0, 40.0, 100.0).paint_clip()
        );
        assert_eq!(
            projection.destination_translation(),
            PaintTranslation::new(0.0, 100.0)
        );
    }

    #[test]
    fn fragmentainer_projection_maps_vertical_lr_rtl_block_slices_from_left() {
        let source_slice = LogicalRect {
            origin: LogicalPoint {
                inline: 0.0,
                block: 20.0,
            },
            size: LogicalSize {
                inline: 100.0,
                block: 40.0,
            },
        };
        let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
            source_axes: FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            source_origin: PageTopPoint::new(10.0, 500.0),
            source_extent: LogicalSize {
                inline: 400.0,
                block: 100.0,
            },
            source_slice,
            destination_axes: FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            destination_origin: PageTopPoint::new(10.0, 500.0),
            destination_extent: LogicalSize {
                inline: 400.0,
                block: 100.0,
            },
            destination_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 100.0,
                    block: 20.0,
                },
                size: source_slice.size,
            },
            destination_page_area: PageTopRect::new(0.0, 600.0, 100.0, 400.0),
        });

        assert_eq!(
            projection.source_clip(),
            PageTopRect::new(30.0, 200.0, 40.0, 100.0).paint_clip()
        );
        assert_eq!(
            projection.destination_translation(),
            PaintTranslation::new(0.0, 100.0)
        );
    }

    #[test]
    fn transition_recorder_preserves_order_and_rewinds_speculative_records() {
        let recorder = FragmentainerTransitionRecorder::default();
        let first = FragmentainerTransitionRecord {
            kind: FragmentainerKind::Column,
            page_index: 0,
            content_bounds: PageTopRect::new(10.0, 200.0, 40.0, 100.0),
        };
        let second = FragmentainerTransitionRecord {
            kind: FragmentainerKind::Column,
            page_index: 1,
            content_bounds: PageTopRect::new(10.0, 200.0, 40.0, 100.0),
        };
        recorder.push(first);
        let snapshot_len = recorder.len();
        recorder.push(second);
        recorder.truncate(snapshot_len);

        assert_eq!(recorder.records(), vec![first]);
    }
}
