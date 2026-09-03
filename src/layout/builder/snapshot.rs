use std::collections::{HashMap, HashSet};

use super::*;
use crate::layout::assets::{DocumentPageIndex, PendingPositionedFragmentation};
use crate::layout::block::{DirectBlockLayoutConstraint, FloatReplayClearanceBoundary};

/// Mutable layout state owned by a speculative layout pass.
///
/// Persistent source caches, pass configuration, and durable replay artifacts
/// deliberately do not belong here. New mutable builder state must be placed
/// in an explicit ownership class rather than silently omitted from replay
/// rollback.
#[derive(Debug, Clone)]
pub(in crate::layout) struct SpeculativeLayoutState {
    pub(in crate::layout) pages: Vec<Page>,
    pub(in crate::layout) page_names: Vec<Option<String>>,
    pub(in crate::layout) page_blanks: Vec<bool>,
    pub(in crate::layout) page_name_scope_suppression: usize,
    pub(in crate::layout) page_name_element_scope_suppression: usize,
    pub(in crate::layout) page_value_scope_stack: Vec<Option<String>>,
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) suppressed_named_strings_before:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) suppressed_named_strings_after:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_source_positions: HashMap<String, PaintPoint>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) page_anchor_counters: HashMap<String, HashMap<String, Vec<i32>>>,
    pub(in crate::layout) has_normal_flow_target_references: bool,
    pub(in crate::layout) document_canvas_background: Option<DocumentCanvasBackground>,
    pub(in crate::layout) document_canvas_scroll_translation: PaintTranslation,
    pub(in crate::layout) document_canvas_root_positioning_area: Option<PaintBackgroundArea>,
    pub(in crate::layout) document_canvas_overflow: DocumentCanvasResolution,
    pub(in crate::layout) document_canvas_fragment_insets: Vec<FragmentOffsets>,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    pub(in crate::layout) current_page_has_named_page_flow_content: bool,
    pub(in crate::layout) current_page_selected_name: Option<String>,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) last_principal_transform_box: Option<assets::TransformReferenceBox>,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    pub(in crate::layout) initial_viewport_context: PageContext,
    pub(in crate::layout) fragmentainer_override: Option<FragmentainerOverride>,
    pub(in crate::layout) footnote_measurements: Vec<FootnoteMeasurement>,
    pub(in crate::layout) rendered_footnote_measurements: Vec<FootnoteMeasurement>,
    pub(in crate::layout) measured_footnotes: HashSet<ElementId>,
    pub(in crate::layout) committed_inline_floats: HashMap<InlineFloatId, CommittedInlineFloat>,
    pub(in crate::layout) rendered_footnotes: HashSet<ElementId>,
    pub(in crate::layout) footnote_call_minimum_page_indices: HashMap<ElementId, usize>,
    pub(in crate::layout) footnote_measurement_depth: usize,
    pub(in crate::layout) fragmentation_suppression_depth: usize,
    pub(in crate::layout) multicol_spanner_fragmentation_depth: usize,
    pub(in crate::layout) multicol_spanner_speculation_depth: usize,
    pub(in crate::layout) multicol_balance_probe_depth: usize,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    pub(in crate::layout) table_cell_content_coordinate_contexts:
        Vec<table::TableCellContentCoordinateContext>,
    pub(in crate::layout) principal_body_block_end_inset: LayoutLength,
    pub(in crate::layout) root_principal_flow_context: RootPrincipalFlowContext,
    /// Recorder lengths at a speculative-layout boundary. The recorder
    /// scopes themselves remain live, while rejected pagination replays must
    /// discard their tentative destination transitions.
    pub(in crate::layout) fragmentainer_transition_recorder_lengths: Vec<usize>,
    pub(in crate::layout) root_pseudo_block_projection: Option<RootPseudoBlockProjection>,
    pub(in crate::layout) direct_block_layout_constraint: Option<DirectBlockLayoutConstraint>,
    pub(in crate::layout) inline_split_float_exclusion_query_offset: RelativeOffset,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout) container_unit_contexts: Vec<ContainerUnitContext>,
    pub(in crate::layout) multicol_column_containing_blocks: Vec<MulticolColumnContainingBlock>,
    pub(in crate::layout) intrinsic_inline_percentage_basis_stack:
        Vec<IntrinsicInlinePercentageBasis>,
    pub(in crate::layout) inline_static_position: Option<StaticPositionCapture>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    pub(in crate::layout) clamp_line_slot_captures: Vec<ClampLineSlotCapture>,
    pub(in crate::layout) positioned_inline_layout_suppression_depth: usize,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    pub(in crate::layout) pending_outside_marker_anchors: PendingOutsideMarkerAnchors,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    pub(in crate::layout) grid_positioning_scopes: Vec<grid::GridPositioningScope>,
    pub(in crate::layout) pending_subgrid_contexts: Vec<Option<grid::ResolvedSubgridContext>>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) active_atomic_inline_coordinate_spaces:
        Vec<AtomicInlineCoordinateSpaceId>,
    pub(in crate::layout) escaped_atom_positioning_context: Option<EscapedAtomPositioningContext>,
    pub(in crate::layout) containing_block_direction: Direction,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    pub(in crate::layout) fragment_top_offsets: Vec<FragmentTopOffset>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout) normal_flow_relative_containing_blocks:
        Vec<NormalFlowRelativeContainingBlock>,
    pub(in crate::layout) static_position_containing_blocks: Vec<StaticPositionContainingBlock>,
    pub(in crate::layout) block_percentage_context_stack: BlockPercentageContextStack,
    pub(in crate::layout) replayed_flex_item_percentage_height_bases:
        Vec<Option<BlockSizePercentageBasis>>,
    pub(in crate::layout) table_wrapper_block_size_overrides: Vec<Option<BorderBoxLength>>,
    pub(in crate::layout) positioned_table_sizing: Vec<Option<PositionedTableSizing>>,
    pub(in crate::layout) multicol_text_box_trim_end_child_indices: Option<Vec<usize>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) layout_pass_kind: LayoutPassKind,
    pub(in crate::layout) execution_purpose: LayoutExecutionPurpose,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    pub(in crate::layout) positioned_generated_source: Option<InlineStaticPositionSourceId>,
    pub(in crate::layout) containing_blocks: Vec<PositionedContainingBlockContext>,
    pub(in crate::layout) fixed_containing_blocks: Vec<PositionedContainingBlockContext>,
    pub(in crate::layout) active_multicol_positioned_containing_block_spans: Vec<u64>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) counter_plan: CounterPlan,
    pub(in crate::layout) current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) current_page_running_elements:
        HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) next_assignment_id: usize,
    pub(in crate::layout) assignment_capture_stack: Vec<Vec<AssignmentId>>,
    pub(in crate::layout) quote_depth: usize,
    pub(in crate::layout) ancestors: Vec<ElementSignature>,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    pub(in crate::layout) committed_positioned_paint_identities:
        HashSet<(DocumentPageIndex, PositionedPaintCommitKey)>,
    pub(in crate::layout) positioned_paint_transaction_depth: usize,
    pub(in crate::layout) positioned_scratch_page_limit: Option<usize>,
    pub(in crate::layout) positioned_scratch_page_origin: Option<DocumentPageIndex>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    pub(in crate::layout) absolute_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) pending_positioned_fragmentation: PendingPositionedFragmentation,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) active_scroll_snap_scopes: Vec<scroll_snap::ActiveScrollSnapScope>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    pub(in crate::layout) float_replay_clearance_scopes: Vec<Option<FloatReplayClearanceBoundary>>,
    pub(in crate::layout) float_fragment_parent_inline_spans: Vec<PageInlineSpan>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_paint_fragments: Vec<PendingPaintFragment>,
    pub(in crate::layout) pending_page_side_effects: Vec<PendingPageSideEffects>,
    pub(in crate::layout) float_paint_capture_depth: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
    pub(in crate::layout) pending_page_footnotes: Vec<ElementId>,
}

/// Opaque checkpoint for one speculative layout pass.
///
/// The checkpoint owns the complete speculative state, but exposes only the
/// read-only boundary facts that replay consumers need.  Keeping the payload
/// private prevents a caller from manufacturing an incomplete checkpoint or
/// coupling new rollback fields to arbitrary layout modules.
#[derive(Debug, Clone)]
pub(in crate::layout) struct LayoutSnapshot {
    speculative: SpeculativeLayoutState,
}

impl LayoutSnapshot {
    pub(in crate::layout) fn page_count(&self) -> usize {
        self.speculative.pages.len()
    }

    pub(in crate::layout) fn current_page_context(&self) -> PageContext {
        self.speculative.current_page_context
    }

    pub(in crate::layout) fn cursor_y(&self) -> f32 {
        self.speculative.cursor_y
    }

    pub(in crate::layout) fn content_left(&self) -> f32 {
        self.speculative.content_left
    }

    pub(in crate::layout) fn content_right(&self) -> f32 {
        self.speculative.content_right
    }

    pub(in crate::layout) fn current_page_has_flow_content(&self) -> bool {
        self.speculative.current_page_has_flow_content
    }

    pub(in crate::layout) fn float_contexts(&self) -> &[FloatContext] {
        &self.speculative.float_contexts
    }

    pub(in crate::layout) fn has_page_anchor(&self, target: &str) -> bool {
        self.speculative.page_anchors.contains_key(target)
    }

    pub(in crate::layout) fn has_page_anchor_source_position(&self, target: &str) -> bool {
        self.speculative
            .page_anchor_source_positions
            .contains_key(target)
    }

    pub(in crate::layout) fn has_page_anchor_text(&self, target: &str) -> bool {
        self.speculative.page_anchor_text.contains_key(target)
    }

    pub(in crate::layout) fn has_page_anchor_counters(&self, target: &str) -> bool {
        self.speculative.page_anchor_counters.contains_key(target)
    }

    pub(in crate::layout) fn bookmark_count(&self) -> usize {
        self.speculative.bookmarks.len()
    }

    pub(in crate::layout) fn current_page_named_strings(
        &self,
    ) -> &HashMap<String, Vec<NamedStringAssignment>> {
        &self.speculative.current_page_named_strings
    }

    pub(in crate::layout) fn current_page_running_elements(
        &self,
    ) -> &HashMap<String, Vec<NamedStringAssignment>> {
        &self.speculative.current_page_running_elements
    }

    pub(in crate::layout) fn current_page_links(&self) -> &Vec<RenderedLink> {
        &self.speculative.current_page.links
    }

    pub(in crate::layout) fn fragmentainer_override(&self) -> Option<FragmentainerOverride> {
        self.speculative.fragmentainer_override
    }

    pub(in crate::layout) fn containing_block_direction(&self) -> Direction {
        self.speculative.containing_block_direction
    }

    pub(in crate::layout) fn containing_block_writing_mode(&self) -> WritingMode {
        self.speculative.containing_block_writing_mode
    }

    pub(in crate::layout) fn fragment_top_offsets(&self) -> &[FragmentTopOffset] {
        &self.speculative.fragment_top_offsets
    }

    fn into_speculative(self) -> SpeculativeLayoutState {
        self.speculative
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            speculative: SpeculativeLayoutState {
                pages: self.pages.clone(),
                page_names: self.page_names.clone(),
                page_blanks: self.page_blanks.clone(),
                page_name_scope_suppression: self.page_name_scope_suppression,
                page_name_element_scope_suppression: self.page_name_element_scope_suppression,
                page_value_scope_stack: self.page_value_scope_stack.clone(),
                page_named_strings: self.page_named_strings.clone(),
                page_running_elements: self.page_running_elements.clone(),
                suppressed_named_strings_before: self.suppressed_named_strings_before.clone(),
                suppressed_named_strings_after: self.suppressed_named_strings_after.clone(),
                page_anchors: self.page_anchors.clone(),
                page_anchor_source_positions: self.page_anchor_source_positions.clone(),
                page_anchor_text: self.page_anchor_text.clone(),
                page_anchor_counters: self.page_anchor_counters.clone(),
                has_normal_flow_target_references: self.has_normal_flow_target_references,
                document_canvas_background: self.document_canvas_background.clone(),
                document_canvas_scroll_translation: self.document_canvas_scroll_translation,
                document_canvas_root_positioning_area: self.document_canvas_root_positioning_area,
                document_canvas_overflow: self.document_canvas_overflow,
                document_canvas_fragment_insets: self.document_canvas_fragment_insets.clone(),
                current_page: self.current_page.clone(),
                current_page_has_flow_content: self.current_page_has_flow_content,
                current_page_has_named_page_flow_content: self
                    .current_page_has_named_page_flow_content,
                current_page_selected_name: self.current_page_selected_name.clone(),
                last_block_layout_outcome: self.last_block_layout_outcome,
                last_principal_transform_box: self.last_principal_transform_box,
                current_page_name: self.current_page_name.clone(),
                current_page_context: self.current_page_context,
                initial_viewport_context: self.initial_viewport_context,
                fragmentainer_override: self.fragmentainer_override,
                footnote_measurements: self.footnote_measurements.clone(),
                rendered_footnote_measurements: self.rendered_footnote_measurements.clone(),
                measured_footnotes: self.measured_footnotes.clone(),
                committed_inline_floats: self.committed_inline_floats.clone(),
                rendered_footnotes: self.rendered_footnotes.clone(),
                footnote_call_minimum_page_indices: self.footnote_call_minimum_page_indices.clone(),
                footnote_measurement_depth: self.footnote_measurement_depth,
                fragmentation_suppression_depth: self.fragmentation_suppression_depth,
                multicol_spanner_fragmentation_depth: self.multicol_spanner_fragmentation_depth,
                multicol_spanner_speculation_depth: self.multicol_spanner_speculation_depth,
                multicol_balance_probe_depth: self.multicol_balance_probe_depth,
                cursor_y: self.cursor_y,
                content_left: self.content_left,
                content_right: self.content_right,
                table_cell_content_coordinate_contexts: self
                    .table_cell_content_coordinate_contexts
                    .clone(),
                principal_body_block_end_inset: self.principal_body_block_end_inset,
                root_principal_flow_context: self.root_principal_flow_context,
                fragmentainer_transition_recorder_lengths: self
                    .fragmentainer_transition_recorders
                    .iter()
                    .map(FragmentainerTransitionRecorder::len)
                    .collect(),
                root_pseudo_block_projection: self.root_pseudo_block_projection,
                direct_block_layout_constraint: self.direct_block_layout_constraint,
                inline_split_float_exclusion_query_offset: self
                    .inline_split_float_exclusion_query_offset,
                content_logical_inline_size_stack: self.content_logical_inline_size_stack.clone(),
                container_unit_contexts: self.container_unit_contexts.clone(),
                multicol_column_containing_blocks: self.multicol_column_containing_blocks.clone(),
                intrinsic_inline_percentage_basis_stack: self
                    .intrinsic_inline_percentage_basis_stack
                    .clone(),
                inline_static_position: self.inline_static_position,
                text_box_line_trim_stack: self.text_box_line_trim_stack.clone(),
                clamp_line_slot_captures: self.clamp_line_slot_captures.clone(),
                positioned_inline_layout_suppression_depth: self
                    .positioned_inline_layout_suppression_depth,
                last_in_flow_line_baseline_y: self.last_in_flow_line_baseline_y,
                pending_outside_marker_anchors: self.pending_outside_marker_anchors.clone(),
                block_static_position_y_offset: self.block_static_position_y_offset,
                absolute_static_position: self.absolute_static_position,
                grid_positioning_scopes: self.grid_positioning_scopes.clone(),
                pending_subgrid_contexts: self.pending_subgrid_contexts.clone(),
                escaped_atom_positioning_depth: self.escaped_atom_positioning_depth,
                active_atomic_inline_coordinate_spaces: self
                    .active_atomic_inline_coordinate_spaces
                    .clone(),
                escaped_atom_positioning_context: self.escaped_atom_positioning_context,
                containing_block_direction: self.containing_block_direction,
                containing_block_writing_mode: self.containing_block_writing_mode,
                fragment_top_offsets: self.fragment_top_offsets.clone(),
                child_available_space_stack: self.child_available_space_stack.clone(),
                normal_flow_relative_containing_blocks: self
                    .normal_flow_relative_containing_blocks
                    .clone(),
                static_position_containing_blocks: self.static_position_containing_blocks.clone(),
                block_percentage_context_stack: self.block_percentage_context_stack.clone(),
                replayed_flex_item_percentage_height_bases: self
                    .replayed_flex_item_percentage_height_bases
                    .clone(),
                table_wrapper_block_size_overrides: self.table_wrapper_block_size_overrides.clone(),
                positioned_table_sizing: self.positioned_table_sizing.clone(),
                multicol_text_box_trim_end_child_indices: self
                    .multicol_text_box_trim_end_child_indices
                    .clone(),
                truncate_page_start_margins: self.truncate_page_start_margins,
                avoid_inside_retry_depth: self.avoid_inside_retry_depth,
                out_of_flow_prebreak_suppression_depth: self.out_of_flow_prebreak_suppression_depth,
                layout_pass_kind: self.layout_pass_kind,
                execution_purpose: self.execution_purpose,
                element_side_effect_suppression_depth: self.element_side_effect_suppression_depth,
                positioned_generated_source: self.positioned_generated_source,
                containing_blocks: self.containing_blocks.clone(),
                fixed_containing_blocks: self.fixed_containing_blocks.clone(),
                active_multicol_positioned_containing_block_spans: self
                    .active_multicol_positioned_containing_block_spans
                    .clone(),
                counter_set: self.counter_set.clone(),
                counter_plan: self.counter_plan.clone(),
                quote_depth: self.quote_depth,
                current_page_named_strings: self.current_page_named_strings.clone(),
                current_page_running_elements: self.current_page_running_elements.clone(),
                next_assignment_id: self.next_assignment_id,
                assignment_capture_stack: self.assignment_capture_stack.clone(),
                ancestors: self.ancestors.clone(),
                page_counter_initial_values: self.page_counter_initial_values.clone(),
                bookmarks: self.bookmarks.clone(),
                positioned_layers: self.positioned_layers.clone(),
                committed_positioned_paint_identities: self
                    .committed_positioned_paint_identities
                    .clone(),
                positioned_paint_transaction_depth: self.positioned_paint_transaction_depth,
                positioned_scratch_page_limit: self.positioned_scratch_page_limit,
                positioned_scratch_page_origin: self.positioned_scratch_page_origin,
                fixed_layers: self.fixed_layers.clone(),
                absolute_positioned_page_span_target: self.absolute_positioned_page_span_target,
                pending_positioned_fragmentation: self.pending_positioned_fragmentation,
                next_paint_source_order: self.next_paint_source_order,
                overflow_clips: self.overflow_clips.clone(),
                active_scroll_snap_scopes: self.active_scroll_snap_scopes.clone(),
                next_float_id: self.next_float_id,
                float_contexts: self.float_contexts.clone(),
                float_replay_clearance_scopes: self.float_replay_clearance_scopes.clone(),
                float_fragment_parent_inline_spans: self.float_fragment_parent_inline_spans.clone(),
                adjoining_float_origin_y: self.adjoining_float_origin_y,
                pending_paint_fragments: self.pending_paint_fragments.clone(),
                pending_page_side_effects: self.pending_page_side_effects.clone(),
                float_paint_capture_depth: self.float_paint_capture_depth,
                preserve_scoped_paint_public_order: self.preserve_scoped_paint_public_order,
                defer_next_block_decoration_promotion: self.defer_next_block_decoration_promotion,
                pending_page_footnotes: self.pending_page_footnotes.clone(),
            },
        }
    }

    pub(in crate::layout) fn restore(&mut self, snapshot: LayoutSnapshot) {
        let snapshot = snapshot.into_speculative();
        self.pages = snapshot.pages;
        self.page_names = snapshot.page_names;
        self.page_blanks = snapshot.page_blanks;
        self.page_name_scope_suppression = snapshot.page_name_scope_suppression;
        self.page_name_element_scope_suppression = snapshot.page_name_element_scope_suppression;
        self.page_value_scope_stack = snapshot.page_value_scope_stack;
        self.page_named_strings = snapshot.page_named_strings;
        self.page_running_elements = snapshot.page_running_elements;
        self.suppressed_named_strings_before = snapshot.suppressed_named_strings_before;
        self.suppressed_named_strings_after = snapshot.suppressed_named_strings_after;
        self.page_anchors = snapshot.page_anchors;
        self.page_anchor_source_positions = snapshot.page_anchor_source_positions;
        self.page_anchor_text = snapshot.page_anchor_text;
        self.page_anchor_counters = snapshot.page_anchor_counters;
        self.has_normal_flow_target_references = snapshot.has_normal_flow_target_references;
        self.document_canvas_background = snapshot.document_canvas_background;
        self.document_canvas_scroll_translation = snapshot.document_canvas_scroll_translation;
        self.document_canvas_root_positioning_area = snapshot.document_canvas_root_positioning_area;
        self.document_canvas_overflow = snapshot.document_canvas_overflow;
        self.document_canvas_fragment_insets = snapshot.document_canvas_fragment_insets;
        self.current_page = snapshot.current_page;
        self.current_page_has_flow_content = snapshot.current_page_has_flow_content;
        self.current_page_has_named_page_flow_content =
            snapshot.current_page_has_named_page_flow_content;
        self.current_page_selected_name = snapshot.current_page_selected_name;
        self.last_block_layout_outcome = snapshot.last_block_layout_outcome;
        self.last_principal_transform_box = snapshot.last_principal_transform_box;
        self.current_page_name = snapshot.current_page_name;
        self.current_page_context = snapshot.current_page_context;
        self.initial_viewport_context = snapshot.initial_viewport_context;
        self.fragmentainer_override = snapshot.fragmentainer_override;
        self.footnote_measurements = snapshot.footnote_measurements;
        self.rendered_footnote_measurements = snapshot.rendered_footnote_measurements;
        self.measured_footnotes = snapshot.measured_footnotes;
        self.committed_inline_floats = snapshot.committed_inline_floats;
        self.rendered_footnotes = snapshot.rendered_footnotes;
        self.footnote_call_minimum_page_indices = snapshot.footnote_call_minimum_page_indices;
        self.footnote_measurement_depth = snapshot.footnote_measurement_depth;
        self.fragmentation_suppression_depth = snapshot.fragmentation_suppression_depth;
        self.multicol_spanner_fragmentation_depth = snapshot.multicol_spanner_fragmentation_depth;
        self.multicol_spanner_speculation_depth = snapshot.multicol_spanner_speculation_depth;
        self.multicol_balance_probe_depth = snapshot.multicol_balance_probe_depth;
        self.cursor_y = snapshot.cursor_y;
        self.content_left = snapshot.content_left;
        self.content_right = snapshot.content_right;
        self.table_cell_content_coordinate_contexts =
            snapshot.table_cell_content_coordinate_contexts;
        self.principal_body_block_end_inset = snapshot.principal_body_block_end_inset;
        self.root_principal_flow_context = snapshot.root_principal_flow_context;
        debug_assert!(
            self.fragmentainer_transition_recorders.len()
                >= snapshot.fragmentainer_transition_recorder_lengths.len(),
            "a speculative restore may not discard an outer transition-recorder scope"
        );
        for (recorder, len) in self
            .fragmentainer_transition_recorders
            .iter()
            .zip(snapshot.fragmentainer_transition_recorder_lengths)
        {
            recorder.truncate(len);
        }
        self.root_pseudo_block_projection = snapshot.root_pseudo_block_projection;
        self.direct_block_layout_constraint = snapshot.direct_block_layout_constraint;
        self.inline_split_float_exclusion_query_offset =
            snapshot.inline_split_float_exclusion_query_offset;
        self.content_logical_inline_size_stack = snapshot.content_logical_inline_size_stack;
        self.container_unit_contexts = snapshot.container_unit_contexts;
        self.multicol_column_containing_blocks = snapshot.multicol_column_containing_blocks;
        self.intrinsic_inline_percentage_basis_stack =
            snapshot.intrinsic_inline_percentage_basis_stack;
        self.inline_static_position = snapshot.inline_static_position;
        self.text_box_line_trim_stack = snapshot.text_box_line_trim_stack;
        self.clamp_line_slot_captures = snapshot.clamp_line_slot_captures;
        self.positioned_inline_layout_suppression_depth =
            snapshot.positioned_inline_layout_suppression_depth;
        self.last_in_flow_line_baseline_y = snapshot.last_in_flow_line_baseline_y;
        self.pending_outside_marker_anchors = snapshot.pending_outside_marker_anchors;
        self.block_static_position_y_offset = snapshot.block_static_position_y_offset;
        self.absolute_static_position = snapshot.absolute_static_position;
        self.grid_positioning_scopes = snapshot.grid_positioning_scopes;
        self.pending_subgrid_contexts = snapshot.pending_subgrid_contexts;
        self.escaped_atom_positioning_depth = snapshot.escaped_atom_positioning_depth;
        self.active_atomic_inline_coordinate_spaces =
            snapshot.active_atomic_inline_coordinate_spaces;
        self.escaped_atom_positioning_context = snapshot.escaped_atom_positioning_context;
        self.containing_block_direction = snapshot.containing_block_direction;
        self.containing_block_writing_mode = snapshot.containing_block_writing_mode;
        self.fragment_top_offsets = snapshot.fragment_top_offsets;
        self.child_available_space_stack = snapshot.child_available_space_stack;
        self.normal_flow_relative_containing_blocks =
            snapshot.normal_flow_relative_containing_blocks;
        self.static_position_containing_blocks = snapshot.static_position_containing_blocks;
        self.block_percentage_context_stack = snapshot.block_percentage_context_stack;
        self.replayed_flex_item_percentage_height_bases =
            snapshot.replayed_flex_item_percentage_height_bases;
        self.table_wrapper_block_size_overrides = snapshot.table_wrapper_block_size_overrides;
        self.positioned_table_sizing = snapshot.positioned_table_sizing;
        self.multicol_text_box_trim_end_child_indices =
            snapshot.multicol_text_box_trim_end_child_indices;
        self.truncate_page_start_margins = snapshot.truncate_page_start_margins;
        self.avoid_inside_retry_depth = snapshot.avoid_inside_retry_depth;
        self.out_of_flow_prebreak_suppression_depth =
            snapshot.out_of_flow_prebreak_suppression_depth;
        self.layout_pass_kind = snapshot.layout_pass_kind;
        self.execution_purpose = snapshot.execution_purpose;
        self.element_side_effect_suppression_depth = snapshot.element_side_effect_suppression_depth;
        self.containing_blocks = snapshot.containing_blocks;
        self.fixed_containing_blocks = snapshot.fixed_containing_blocks;
        self.active_multicol_positioned_containing_block_spans =
            snapshot.active_multicol_positioned_containing_block_spans;
        self.counter_set = snapshot.counter_set;
        self.counter_plan = snapshot.counter_plan;
        self.quote_depth = snapshot.quote_depth;
        self.positioned_generated_source = snapshot.positioned_generated_source;
        self.current_page_named_strings = snapshot.current_page_named_strings;
        self.current_page_running_elements = snapshot.current_page_running_elements;
        self.next_assignment_id = snapshot.next_assignment_id;
        self.assignment_capture_stack = snapshot.assignment_capture_stack;
        self.ancestors = snapshot.ancestors;
        self.page_counter_initial_values = snapshot.page_counter_initial_values;
        self.bookmarks = snapshot.bookmarks;
        self.positioned_layers = snapshot.positioned_layers;
        self.committed_positioned_paint_identities = snapshot.committed_positioned_paint_identities;
        self.positioned_paint_transaction_depth = snapshot.positioned_paint_transaction_depth;
        self.positioned_scratch_page_limit = snapshot.positioned_scratch_page_limit;
        self.positioned_scratch_page_origin = snapshot.positioned_scratch_page_origin;
        self.fixed_layers = snapshot.fixed_layers;
        self.absolute_positioned_page_span_target = snapshot.absolute_positioned_page_span_target;
        self.pending_positioned_fragmentation = snapshot.pending_positioned_fragmentation;
        self.next_paint_source_order = snapshot.next_paint_source_order;
        self.overflow_clips = snapshot.overflow_clips;
        self.active_scroll_snap_scopes = snapshot.active_scroll_snap_scopes;
        self.next_float_id = snapshot.next_float_id;
        self.float_contexts = snapshot.float_contexts;
        self.float_replay_clearance_scopes = snapshot.float_replay_clearance_scopes;
        self.float_fragment_parent_inline_spans = snapshot.float_fragment_parent_inline_spans;
        self.adjoining_float_origin_y = snapshot.adjoining_float_origin_y;
        self.pending_paint_fragments = snapshot.pending_paint_fragments;
        self.pending_page_side_effects = snapshot.pending_page_side_effects;
        self.float_paint_capture_depth = snapshot.float_paint_capture_depth;
        self.preserve_scoped_paint_public_order = snapshot.preserve_scoped_paint_public_order;
        self.defer_next_block_decoration_promotion = snapshot.defer_next_block_decoration_promotion;
        self.pending_page_footnotes = snapshot.pending_page_footnotes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Css;
    use crate::css::{
        ComputedBoxValues, ComputedLengthPercentage, ComputedLengthPercentageOrAuto,
        ComputedLineHeight, PhysicalEdges,
    };
    use crate::document::paint::display_list::PaintBand;
    use crate::document::paint::shapes::RenderedRect;
    use crate::layout::block::FloatReplayClearanceBoundary;

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
            // The builder retains this reference for its lifetime; tests that do
            // not exercise iframes use one immutable empty fixture.
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            target_references: crate::layout::TargetReferenceSnapshot::default(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn speculative_layout_discards_pages_paint_and_float_state_after_a_committed_page() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let committed_page = Page::new(120.0, 180.0);
        builder.pages.push(committed_page.clone());
        builder.current_page.push_rect_in_band(
            PaintBand::InFlowBlock,
            RenderedRect::from_paint_rect(paint_space_rect(4.0, 5.0, 6.0, 7.0), None),
        );
        let committed_current_page = builder.current_page.clone();

        let geometry = builder.with_speculative_layout(|layout| {
            assert_eq!(
                layout.execution_purpose,
                LayoutExecutionPurpose::Speculative
            );
            layout.current_page.push_rect_in_band(
                PaintBand::InFlowBlock,
                RenderedRect::from_paint_rect(paint_space_rect(40.0, 50.0, 60.0, 70.0), None),
            );
            layout.pages.push(Page::new(240.0, 360.0));
            layout.push_float_context();
            layout.pop_float_context();
            42.0
        });

        assert_eq!(geometry, 42.0);
        assert_eq!(builder.execution_purpose, LayoutExecutionPurpose::Committed);
        assert_eq!(builder.pages, vec![committed_page]);
        assert_eq!(builder.current_page, committed_current_page);
    }

    #[test]
    fn resolves_font_metric_lengths_in_typographic_pseudo_styles() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle {
            font_size: 20.0,
            ..ComputedStyle::initial()
        };
        style.first_line_style = Some(Box::new(ComputedStyle {
            line_height_value: ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(2.0)),
            ..style.clone()
        }));
        style.first_letter_style = Some(Box::new(ComputedStyle {
            box_values: ComputedBoxValues {
                margin: PhysicalEdges {
                    left: ComputedLengthPercentageOrAuto::LengthPercentage(
                        ComputedLengthPercentage::from_ch(3.0),
                    ),
                    ..ComputedBoxValues::initial().margin
                },
                ..ComputedBoxValues::initial()
            },
            ..style.clone()
        }));

        builder.resolve_style_font_metric_lengths(&mut style);

        let first_line = style.first_line_style.as_ref().unwrap();
        let ComputedLineHeight::Length(line_height) = &first_line.line_height_value else {
            panic!("expected first-line length line-height");
        };
        assert!(!line_height.requires_ch_advance());
        assert!(line_height.length_points() > 0.0);

        let first_letter = style.first_letter_style.as_ref().unwrap();
        let ComputedLengthPercentageOrAuto::LengthPercentage(margin_left) =
            &first_letter.box_values.margin.left
        else {
            panic!("expected first-letter length margin");
        };
        assert!(!margin_left.requires_ch_advance());
        assert!(margin_left.length_points() > 0.0);
    }

    #[test]
    fn page_background_positioning_uses_typed_paint_rects_for_each_box() {
        let declarations = Declarations::from_iter([
            ("border-top-width".to_string(), "7pt".to_string()),
            ("border-right-width".to_string(), "11pt".to_string()),
            ("border-bottom-width".to_string(), "13pt".to_string()),
            ("border-left-width".to_string(), "17pt".to_string()),
            ("border-top-style".to_string(), "solid".to_string()),
            ("border-right-style".to_string(), "solid".to_string()),
            ("border-bottom-style".to_string(), "solid".to_string()),
            ("border-left-style".to_string(), "solid".to_string()),
            ("padding-top".to_string(), "2pt".to_string()),
            ("padding-right".to_string(), "3pt".to_string()),
            ("padding-bottom".to_string(), "5pt".to_string()),
            ("padding-left".to_string(), "7pt".to_string()),
        ]);
        let margins = PageMargins::from_points(11.0, 13.0, 17.0, 19.0);
        let size = PageSize::from_points(200.0, 180.0);

        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Border,
                layout_pt(0.0),
            ),
            paint_space_rect(19.0, 17.0, 168.0, 152.0),
        );
        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Padding,
                layout_pt(0.0),
            ),
            paint_space_rect(36.0, 30.0, 140.0, 132.0),
        );
        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Content,
                layout_pt(0.0),
            ),
            paint_space_rect(43.0, 35.0, 130.0, 125.0),
        );
    }

    #[test]
    fn first_named_page_establishes_the_viewport_once() {
        let options = RenderOptions::default();
        let stylesheets = vec![css::parse_stylesheet(&Css::from_string(
            "@page { size: 300pt 400pt } @page chapter { size: 200pt 200pt }",
        ))];
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.current_page_name = Some("chapter".to_string());
        builder.rebuild_empty_current_page_context();
        let initial = builder.initial_viewport_context;

        builder.current_page_has_flow_content = true;
        builder.push_page();
        builder.current_page_name = None;
        builder.rebuild_empty_current_page_context();

        assert_eq!(builder.initial_viewport_context, initial);
        assert_eq!(initial.size, PageSize::from_points(200.0, 200.0));
        assert_eq!(
            builder.current_page_context.size,
            PageSize::from_points(300.0, 400.0)
        );
    }

    #[test]
    fn named_page_boundary_requires_in_flow_predecessor() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.current_page_name = Some("float-page".to_string());

        // A preceding float has paint and pagination occupancy, but it is
        // out of normal flow and cannot form a class-A boundary for the next
        // page-name group.
        builder.current_page_has_flow_content = true;
        let scope = builder.enter_page_name_scope_for_value(Some("article"));
        assert_eq!(builder.pages.len(), 0);
        assert_eq!(builder.current_page_name.as_deref(), Some("article"));
        builder.exit_page_name_scope(
            scope.map(|previous_page_name| PageNameScope::Inline { previous_page_name }),
        );

        builder.mark_current_page_flow_content();
        builder.enter_page_name_scope_for_value(Some("appendix"));
        assert_eq!(builder.pages.len(), 1);
        assert_eq!(builder.current_page_name.as_deref(), Some("appendix"));
    }

    #[test]
    fn viewport_units_use_the_immutable_initial_context_after_a_named_transition() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial = PageContext {
            size: PageSize::from_points(200.0, 200.0),
            ..PageContext::from_options(&options)
        };
        let destination = PageContext {
            size: PageSize::from_points(300.0, 400.0),
            ..PageContext::from_options(&options)
        };
        builder.initial_viewport_context = initial;
        builder.current_page_context = destination;
        let style = ComputedStyle {
            box_values: ComputedBoxValues {
                width: ComputedLengthPercentageOrAuto::LengthPercentage(
                    ComputedLengthPercentage::from_vw(100.0),
                ),
                height: css::PhysicalHeight::from_computed(
                    ComputedLengthPercentageOrAuto::LengthPercentage(
                        ComputedLengthPercentage::from_vh(100.0),
                    ),
                ),
                ..ComputedBoxValues::initial()
            },
            ..ComputedStyle::initial()
        };

        let resolved = builder.style_with_current_viewport_lengths(&style);
        let ComputedLengthPercentageOrAuto::LengthPercentage(width) = &resolved.box_values.width
        else {
            panic!("expected viewport-resolved width");
        };
        let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
            resolved.box_values.height.value()
        else {
            panic!("expected viewport-resolved height");
        };
        assert_eq!(width.length_points(), initial.area_width());
        assert_eq!(height.length_points(), initial.area_height());
    }

    #[test]
    fn layout_snapshot_restores_speculative_state_without_rewinding_pass_state() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial_cursor = builder.cursor_y;
        let snapshot = builder.snapshot();

        builder.cursor_y += 20.0;
        builder.quote_depth = 3;
        builder.page_anchors.insert("speculative-anchor".into(), 0);
        builder.positioned_paint_transaction_depth = 1;
        builder
            .float_contexts
            .push(FloatContext { shapes: Vec::new() });

        // Reservations select the outer footnote pass. They deliberately
        // survive a local speculative replay rather than being restored from
        // its checkpoint.
        builder.footnote_reservations.insert(0, 12.0);
        builder.restore(snapshot);

        assert_eq!(builder.cursor_y, initial_cursor);
        assert_eq!(builder.quote_depth, 0);
        assert!(builder.page_anchors.is_empty());
        assert_eq!(builder.positioned_paint_transaction_depth, 0);
        assert_eq!(builder.float_contexts.len(), 1);
        assert_eq!(builder.footnote_reservations.get(&0), Some(&12.0));
    }

    #[test]
    fn float_replay_clearance_scope_inherits_isolates_and_survives_snapshot_restore() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let boundary = FloatReplayClearanceBoundary::new(PageTopBlockPosition::new(42.0));

        builder.with_float_replay_clearance_scope(Some(boundary), |builder| {
            assert_eq!(
                builder.current_float_replay_clearance_boundary(),
                Some(boundary)
            );

            let snapshot = builder.snapshot();
            builder.with_float_replay_clearance_scope(None, |builder| {
                // An independent BFC gets a lexical reset rather than its
                // ancestor's clearance edge.
                assert_eq!(builder.current_float_replay_clearance_boundary(), None);
            });
            builder.restore(snapshot);

            assert_eq!(
                builder.current_float_replay_clearance_boundary(),
                Some(boundary)
            );
        });

        assert_eq!(builder.current_float_replay_clearance_boundary(), None);
    }
}
