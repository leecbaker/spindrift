use std::collections::{HashMap, HashSet};

use super::*;
use crate::layout::assets::{DocumentPageIndex, PositionedPaginationState};

/// Moves durable document output out of a builder before a speculative
/// layout traversal.
///
/// A [`LayoutSnapshot`] only restores mutable state. This transaction also
/// gives the traversal fresh pagination and paint ownership, then restores
/// durable document artifacts verbatim. Use it for every speculative pass
/// that can lay out descendants, including floats, tables, flex/grid items,
/// positioned boxes, or fragmentainers.
///
/// The CSS layout specifications permit these descendants to execute their
/// ordinary algorithms while sizing an ancestor. Their resulting document
/// output must nevertheless be discarded: CSS sizing uses geometry, not an
/// extra painted copy of the subtree.
#[must_use]
pub(in crate::layout) struct SpeculativeLayoutTransaction {
    pagination: PositionedPaginationState,
    rollback: LayoutSnapshot,
    pending_outside_marker_anchors: SuspendedOutsideMarkerAnchors,
    speculative_table_height_estimates: HashMap<TableHeightEstimateCacheKey, f32>,
    speculative_table_height_plans: HashMap<TableHeightPlanCacheKey, table::TableHeightPlan>,
    speculative_auto_float_margin_box_heights: HashMap<AutoFloatMeasurementKey, MarginBoxLength>,
    fragmentainer_transition_recorders: Vec<FragmentainerTransitionRecorder>,
    committed_inline_floats: HashMap<InlineFloatId, CommittedInlineFloat>,
    positioned_layers: Vec<PositionedPaintLayer>,
    fixed_layers: Vec<FixedPaintLayer>,
    committed_positioned_paint_identities: HashSet<(DocumentPageIndex, PositionedPaintCommitKey)>,
    deferred_multicol_positioned_children: Vec<DeferredMulticolPositionedChild>,
    multicol_positioned_containing_block_spans: Vec<MulticolPositionedContainingBlockSpan>,
    next_multicol_positioned_containing_block_span_id: u64,
    multicol_positioned_replay_capture_depth: usize,
    execution_purpose: LayoutExecutionPurpose,
    page_value_scope_depth: usize,
    containing_block_depth: usize,
    assignment_capture_depth: usize,
}

impl SpeculativeLayoutTransaction {
    /// Start a replay whose output is discarded when [`Self::restore`] is
    /// called. The caller may read geometry from the scratch replay, but may
    /// not retain its paint or deferred side effects.
    pub(in crate::layout) fn begin(layout: &mut LayoutBuilder<'_>) -> Self {
        let page_value_scope_depth = layout.page_value_scope_stack.len();
        let containing_block_depth = layout.containing_blocks.len();
        let assignment_capture_depth = layout.assignment_capture_stack.len();
        let pagination = layout.take_positioned_pagination_state();
        let speculative_table_height_estimates =
            std::mem::take(&mut layout.speculative_table_height_estimates);
        let speculative_table_height_plans =
            std::mem::take(&mut layout.speculative_table_height_plans);
        let speculative_auto_float_margin_box_heights =
            std::mem::take(&mut layout.speculative_auto_float_margin_box_heights);
        let fragmentainer_transition_recorders =
            std::mem::take(&mut layout.fragmentainer_transition_recorders);
        let committed_inline_floats = std::mem::take(&mut layout.committed_inline_floats);
        let positioned_layers = std::mem::take(&mut layout.positioned_layers);
        let fixed_layers = std::mem::take(&mut layout.fixed_layers);
        let committed_positioned_paint_identities =
            std::mem::take(&mut layout.committed_positioned_paint_identities);
        let deferred_multicol_positioned_children =
            std::mem::take(&mut layout.deferred_multicol_positioned_children);
        let multicol_positioned_containing_block_spans =
            std::mem::take(&mut layout.multicol_positioned_containing_block_spans);
        let next_multicol_positioned_containing_block_span_id =
            layout.next_multicol_positioned_containing_block_span_id;
        let multicol_positioned_replay_capture_depth =
            layout.multicol_positioned_replay_capture_depth;
        let execution_purpose = layout.execution_purpose;
        debug_assert!(layout.committed_inline_floats.is_empty());
        debug_assert!(layout.positioned_layers.is_empty());
        debug_assert!(layout.fixed_layers.is_empty());
        debug_assert!(layout.deferred_multicol_positioned_children.is_empty());
        // A discarded replay can generate descendant lines, but none is an
        // accepted principal line of the surrounding document.
        let pending_outside_marker_anchors = layout.pending_outside_marker_anchors.suspend();
        let rollback = layout.snapshot();
        layout.execution_purpose = LayoutExecutionPurpose::Speculative;
        Self {
            pagination,
            rollback,
            pending_outside_marker_anchors,
            speculative_table_height_estimates,
            speculative_table_height_plans,
            speculative_auto_float_margin_box_heights,
            fragmentainer_transition_recorders,
            committed_inline_floats,
            positioned_layers,
            fixed_layers,
            committed_positioned_paint_identities,
            deferred_multicol_positioned_children,
            multicol_positioned_containing_block_spans,
            next_multicol_positioned_containing_block_span_id,
            multicol_positioned_replay_capture_depth,
            execution_purpose,
            page_value_scope_depth,
            containing_block_depth,
            assignment_capture_depth,
        }
    }

    /// Discard scratch output and restore the document state that preceded
    /// [`Self::begin`].
    pub(in crate::layout) fn restore(self, layout: &mut LayoutBuilder<'_>) {
        debug_assert_eq!(
            layout.execution_purpose,
            LayoutExecutionPurpose::Speculative,
            "a speculative transaction must retain its execution purpose"
        );
        layout.restore(self.rollback);
        layout
            .pending_outside_marker_anchors
            .restore(self.pending_outside_marker_anchors);
        debug_assert!(layout.committed_inline_floats.is_empty());
        debug_assert!(layout.positioned_layers.is_empty());
        debug_assert!(layout.fixed_layers.is_empty());
        debug_assert!(layout.committed_positioned_paint_identities.is_empty());
        debug_assert!(layout.deferred_multicol_positioned_children.is_empty());
        // Scratch replay can establish multicol positioned containing blocks.
        // They belong to discarded output and must not resolve a later
        // committed descendant against a non-existent fragmentainer.
        let discarded_scratch_spans =
            std::mem::take(&mut layout.multicol_positioned_containing_block_spans);
        debug_assert!(
            layout
                .active_multicol_positioned_containing_block_spans
                .is_empty()
        );
        drop(discarded_scratch_spans);
        layout.restore_positioned_pagination_state(self.pagination);
        layout.speculative_table_height_estimates = self.speculative_table_height_estimates;
        layout.speculative_table_height_plans = self.speculative_table_height_plans;
        layout.speculative_auto_float_margin_box_heights =
            self.speculative_auto_float_margin_box_heights;
        debug_assert!(layout.fragmentainer_transition_recorders.is_empty());
        layout.fragmentainer_transition_recorders = self.fragmentainer_transition_recorders;
        layout.committed_inline_floats = self.committed_inline_floats;
        layout.positioned_layers = self.positioned_layers;
        layout.fixed_layers = self.fixed_layers;
        layout.committed_positioned_paint_identities = self.committed_positioned_paint_identities;
        layout.deferred_multicol_positioned_children = self.deferred_multicol_positioned_children;
        layout.multicol_positioned_containing_block_spans =
            self.multicol_positioned_containing_block_spans;
        layout.next_multicol_positioned_containing_block_span_id =
            self.next_multicol_positioned_containing_block_span_id;
        layout.multicol_positioned_replay_capture_depth =
            self.multicol_positioned_replay_capture_depth;
        layout.execution_purpose = self.execution_purpose;
        debug_assert_eq!(
            layout.page_value_scope_stack.len(),
            self.page_value_scope_depth,
            "discarded replay must restore page-value scopes"
        );
        debug_assert_eq!(
            layout.containing_blocks.len(),
            self.containing_block_depth,
            "discarded replay must restore containing-block scopes"
        );
        debug_assert_eq!(
            layout.assignment_capture_stack.len(),
            self.assignment_capture_depth,
            "discarded replay must restore assignment-capture scopes"
        );
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Run ordinary layout in scratch output and return only its explicit
    /// geometry result.
    ///
    /// All document output channels are moved into the transaction before the
    /// closure starts. Consequently no descendant can retain paint, pages, or
    /// deferred page side effects in the committed document by accident.
    pub(in crate::layout) fn with_speculative_layout<T>(
        &mut self,
        layout: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let transaction = SpeculativeLayoutTransaction::begin(self);
        let result = layout(self);
        transaction.restore(self);
        result
    }
}
