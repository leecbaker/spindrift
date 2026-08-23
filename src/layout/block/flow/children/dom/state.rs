use super::*;

/// Mutable source and flow state that survives recursive direct-DOM child
/// layout. Keeping this controller heap-owned prevents every recursive DOM
/// traversal frame from reserving storage for its replay and float state.
pub(in crate::layout) struct DomFlowTraversalState {
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    pub(in crate::layout) adjoining_margin_set_boundary: BlockMarginCollapseBoundary,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: FirstInFlowChildState,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) first_formatted_line: FirstFormattedLineState,
    pub(in crate::layout) out_of_flow_static_source: Option<StaticPositionRectangle>,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) avoid_run_candidate: Option<AvoidBreakRunCandidate>,
    pub(in crate::layout) previous_break_after: PageBreak,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) adjoining_float_replay: Option<AdjoiningFloatReplayCandidate>,
    pub(in crate::layout) replaying_adjoining_until: Option<usize>,
    pub(in crate::layout) avoid_run_preflight_cache: AvoidRunPreflightCache,
    pub(in crate::layout) automatic_marker_replay_target: Option<usize>,
    pub(in crate::layout) automatic_marker_candidate:
        Option<Box<DomAutomaticBlockSizeReplayCheckpoint>>,
    pub(in crate::layout) child_node_index: usize,
}

impl DomFlowTraversalState {
    pub(in crate::layout) fn new(
        first_formatted_line: FirstFormattedLineState,
        out_of_flow_static_source: Option<StaticPositionRectangle>,
        float_run: FloatRunState,
        trim_block_start_adjoining_margins: bool,
    ) -> Self {
        Self {
            collapsed_end_margin: false,
            pending_end_margin_collapse: None,
            collapsed_start_margin_offset: layout_pt(0.0),
            adjoining_margin_set_boundary: BlockMarginCollapseBoundary::Adjoining,
            previous_flow_bottom_margin: None,
            seen_flow_child: FirstInFlowChildState::NotSeen,
            trim_block_start_adjoining_margins,
            first_formatted_line,
            out_of_flow_static_source,
            element_index: 0,
            float_run,
            avoid_run_candidate: None,
            previous_break_after: PageBreak::Auto,
            previous_child_page_end: None,
            adjoining_float_replay: None,
            replaying_adjoining_until: None,
            avoid_run_preflight_cache: AvoidRunPreflightCache::default(),
            automatic_marker_replay_target: None,
            automatic_marker_candidate: None,
            child_node_index: 0,
        }
    }

    /// Capture the controller immediately before a child can become the
    /// automatic block-size marker host. This intentionally stays outside
    /// the recursive DOM traversal frame: `snapshot()` materializes a large
    /// rollback value before it becomes heap-owned by the checkpoint.
    #[inline(never)]
    pub(in crate::layout) fn capture_automatic_marker_checkpoint(
        &self,
        builder: &LayoutBuilder<'_>,
        traversal_state: &BlockFlowChildTraversalState,
    ) -> Option<Box<DomAutomaticBlockSizeReplayCheckpoint>> {
        traversal_state.has_automatic_block_size_clamp().then(|| {
            Box::new(DomAutomaticBlockSizeReplayCheckpoint {
                state: AutomaticBlockSizeReplayState {
                    snapshot: builder.snapshot(),
                    previous_flow_bottom_margin: self.previous_flow_bottom_margin,
                    seen_flow_child: self.seen_flow_child,
                    trim_block_start_adjoining_margins: self.trim_block_start_adjoining_margins,
                    collapsed_end_margin: self.collapsed_end_margin,
                    pending_end_margin_collapse: self.pending_end_margin_collapse,
                    previous_child_page_end: self.previous_child_page_end.clone(),
                    float_run: self.float_run,
                    previous_break_after: self.previous_break_after,
                    first_formatted_line: self.first_formatted_line,
                    traversal_state: traversal_state.clone(),
                },
                child_node_index: self.child_node_index,
                element_index: self.element_index,
            })
        })
    }

    /// Restore the complete source cursor and flow controller captured before
    /// an automatic block-size marker candidate enters recursive layout.
    pub(in crate::layout) fn restore_automatic_marker_checkpoint(
        &mut self,
        checkpoint: Box<DomAutomaticBlockSizeReplayCheckpoint>,
        builder: &mut LayoutBuilder<'_>,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) {
        let DomAutomaticBlockSizeReplayCheckpoint {
            state:
                AutomaticBlockSizeReplayState {
                    snapshot,
                    previous_flow_bottom_margin,
                    seen_flow_child,
                    trim_block_start_adjoining_margins,
                    collapsed_end_margin,
                    pending_end_margin_collapse,
                    previous_child_page_end,
                    float_run,
                    previous_break_after,
                    first_formatted_line,
                    traversal_state: saved_traversal_state,
                },
            child_node_index,
            element_index,
        } = *checkpoint;
        builder.restore(snapshot);
        self.child_node_index = child_node_index;
        self.element_index = element_index;
        self.previous_flow_bottom_margin = previous_flow_bottom_margin;
        self.seen_flow_child = seen_flow_child;
        self.trim_block_start_adjoining_margins = trim_block_start_adjoining_margins;
        self.collapsed_end_margin = collapsed_end_margin;
        self.pending_end_margin_collapse = pending_end_margin_collapse;
        self.previous_child_page_end = previous_child_page_end;
        self.float_run = float_run;
        self.previous_break_after = previous_break_after;
        self.first_formatted_line = first_formatted_line;
        *traversal_state = saved_traversal_state;
        self.automatic_marker_replay_target = Some(child_node_index);
        self.avoid_run_candidate = None;
        self.adjoining_float_replay = None;
        self.replaying_adjoining_until = None;
    }
}
