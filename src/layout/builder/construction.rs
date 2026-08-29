use std::collections::HashSet;

use super::*;
use crate::layout::assets::PendingPositionedFragmentation;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn new(config: LayoutBuilderConfig<'a>) -> Self {
        let mut counter_styles = HashMap::new();
        let mut page_rules = Vec::new();
        let mut page_declarations = Declarations::new();
        let mut first_page_declarations = Declarations::new();
        for stylesheet in config.stylesheets.iter() {
            for rule in &stylesheet.rules {
                if rule.selector_text.trim() == ":root" {
                    page_declarations.extend(
                        (&rule.declarations)
                            .into_iter()
                            .filter_map(|(name, value)| {
                                name.starts_with("--")
                                    .then_some((name.clone(), value.clone()))
                            })
                            .collect(),
                    );
                }
            }
            first_page_declarations.extend(stylesheet.first_page_declarations.clone());
            page_rules.extend(stylesheet.page_rules.clone());
            for counter_style in &stylesheet.counter_styles {
                // The six non-overridable predefined styles always use their
                // UA definitions. Other predefined styles may be overridden
                // by later author stylesheets, including the HTML `type`
                // presentational hints that name lower-roman and friends.
                // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
                let non_overridable = matches!(
                    counter_style.name.as_str(),
                    "decimal"
                        | "disc"
                        | "square"
                        | "circle"
                        | "disclosure-open"
                        | "disclosure-closed"
                );
                if !non_overridable || !counter_styles.contains_key(&counter_style.name) {
                    counter_styles.insert(counter_style.name.clone(), counter_style.clone());
                }
            }
        }
        let page_context = PageContext::from_options(config.options);
        let mut builder = Self {
            options: config.options,
            stylesheets: config.stylesheets,
            base_url: config.base_url,
            root_url: config.root_url,
            resource_cache: config.resource_cache,
            iframe_documents: config.iframe_documents,
            iframe_viewport: config.iframe_viewport,
            pages: Vec::new(),
            page_names: Vec::new(),
            page_blanks: Vec::new(),
            page_name_scope_suppression: 0,
            page_name_element_scope_suppression: 0,
            page_value_scope_stack: Vec::new(),
            page_named_strings: Vec::new(),
            page_running_elements: Vec::new(),
            suppressed_named_strings_before: HashMap::new(),
            suppressed_named_strings_after: HashMap::new(),
            page_anchors: HashMap::new(),
            page_anchor_source_positions: HashMap::new(),
            page_anchor_text: HashMap::new(),
            page_anchor_counters: HashMap::new(),
            target_references: config.target_references,
            has_normal_flow_target_references: false,
            document_canvas_background: None,
            document_canvas_scroll_translation: PaintTranslation::identity(),
            document_canvas_root_positioning_area: None,
            document_canvas_overflow: DocumentCanvasResolution::default(),
            document_canvas_fragment_insets: Vec::new(),
            document_root_generates_box: true,
            current_page: page_for_context(page_context),
            current_page_has_flow_content: false,
            current_page_has_named_page_flow_content: false,
            current_page_selected_name: None,
            last_block_layout_outcome: BlockLayoutOutcome::default(),
            last_principal_transform_box: None,
            preserve_3d_context_depth: 0,
            current_page_name: None,
            current_page_context: page_context,
            initial_viewport_context: page_context,
            page_descriptor_viewport_size: page_context.size,
            fragmentainer_override: None,
            footnote_bodies: HashMap::new(),
            ruby_formatting_descendants: HashMap::new(),
            dom_page_boundary_summaries: HashMap::new(),
            speculative_table_height_estimates: HashMap::new(),
            speculative_table_height_plans: HashMap::new(),
            footnote_measurements: Vec::new(),
            rendered_footnote_measurements: Vec::new(),
            measured_footnotes: HashSet::new(),
            committed_inline_floats: HashMap::new(),
            footnote_reservations: HashMap::new(),
            footnote_call_minimum_page_indices: HashMap::new(),
            footnote_layout_mode: FootnoteLayoutMode::Measure,
            footnote_measurement_depth: 0,
            rendered_footnotes: std::collections::HashSet::new(),
            pending_page_footnotes: Vec::new(),
            fragmentation_suppression_depth: 0,
            multicol_spanner_fragmentation_depth: 0,
            multicol_spanner_speculation_depth: 0,
            multicol_balance_probe_depth: 0,
            speculative_auto_float_margin_box_heights: HashMap::new(),
            active_auto_float_measurements: Vec::new(),
            active_auto_float_measurement_fallbacks: Vec::new(),
            inherited_adjoining_start_margins: Vec::new(),
            float_replay_clearance_scopes: Vec::new(),
            cursor_y: page_context.top(),
            content_left: page_context.left(),
            content_right: page_context.right(),
            table_cell_content_coordinate_contexts: Vec::new(),
            principal_inline_end_inset: 0.0,
            principal_body_block_end_inset: layout_pt(0.0),
            root_principal_flow_context: RootPrincipalFlowContext::default(),
            root_pseudo_block_projection: None,
            direct_block_layout_constraint: None,
            inline_split_float_exclusion_query_offset: RelativeOffset::zero(),
            content_logical_inline_size_stack: Vec::new(),
            container_unit_contexts: Vec::new(),
            multicol_column_containing_blocks: Vec::new(),
            intrinsic_inline_percentage_basis_stack: Vec::new(),
            inline_static_position: None,
            text_box_line_trim_stack: Vec::new(),
            clamp_line_slot_captures: Vec::new(),
            positioned_inline_layout_suppression_depth: 0,
            last_in_flow_line_baseline_y: None,
            pending_outside_marker_anchors: PendingOutsideMarkerAnchors::default(),
            block_static_position_y_offset: None,
            absolute_static_position: None,
            grid_positioning_scopes: Vec::new(),
            pending_subgrid_contexts: Vec::new(),
            escaped_atom_positioning_depth: 0,
            escaped_atom_containing_block: None,
            escaped_atom_positioning_context: None,
            containing_block_direction: Direction::Ltr,
            containing_block_writing_mode: WritingMode::HorizontalTb,
            initial_containing_block_writing_mode: WritingMode::HorizontalTb,
            principal_flow: DocumentPrincipalFlow {
                writing_mode: WritingMode::HorizontalTb,
                direction: Direction::Ltr,
                text_orientation: TextOrientation::Mixed,
                source: PrincipalFlowSource::Root,
            },
            fragmentainer_transition_recorders: Vec::new(),
            fragment_top_offsets: Vec::new(),
            child_available_space_stack: Vec::new(),
            normal_flow_relative_containing_blocks: Vec::new(),
            static_position_containing_blocks: Vec::new(),
            block_percentage_context_stack: BlockPercentageContextStack::default(),
            replayed_flex_item_percentage_height_bases: Vec::new(),
            table_wrapper_block_size_overrides: Vec::new(),
            positioned_table_sizing: Vec::new(),
            truncate_page_start_margins: false,
            avoid_inside_retry_depth: 0,
            out_of_flow_prebreak_suppression_depth: 0,
            layout_pass_kind: LayoutPassKind::Normal,
            execution_purpose: LayoutExecutionPurpose::Committed,
            element_side_effect_suppression_depth: 0,
            containing_blocks: Vec::new(),
            fixed_containing_blocks: Vec::new(),
            multicol_text_box_trim_end_child_indices: None,
            counter_set: CounterSet::new(),
            counter_plan: CounterPlan::default(),
            quote_depth: 0,
            current_page_named_strings: HashMap::new(),
            current_page_running_elements: HashMap::new(),
            next_assignment_id: 0,
            assignment_capture_stack: Vec::new(),
            ancestors: Vec::new(),
            page_counter_initial_values: config.page_counter_initial_values,
            page_rules,
            page_progression_direction: config.page_progression_direction,
            page_margin_inherited_style:
                crate::layout::page_margin::page_context_style_from_options(config.options),
            page_declarations,
            counter_styles,
            first_page_declarations,
            root_metric_state: RootMetricState::Bootstrapping,
            root_metrics_require_selected_font: false,
            font_system: Box::new(config.font_system),
            autospace_items_scratch: Vec::new(),
            bookmarks: Vec::new(),
            positioned_layers: Vec::new(),
            committed_positioned_paint_identities: HashSet::new(),
            positioned_paint_transaction_depth: 0,
            positioned_scratch_page_limit: None,
            positioned_scratch_page_origin: None,
            fixed_layers: Vec::new(),
            deferred_multicol_positioned_children: Vec::new(),
            multicol_positioned_containing_block_spans: Vec::new(),
            next_multicol_positioned_containing_block_span_id: 1,
            active_multicol_positioned_containing_block_spans: Vec::new(),
            multicol_positioned_replay_capture_depth: 0,
            absolute_positioned_page_span_target: None,
            pending_positioned_fragmentation: PendingPositionedFragmentation::default(),
            next_paint_source_order: 1,
            overflow_clips: Vec::new(),
            active_scroll_snap_scopes: Vec::new(),
            next_float_id: 1,
            float_contexts: vec![FloatContext { shapes: Vec::new() }],
            float_fragment_parent_inline_spans: Vec::new(),
            adjoining_float_origin_y: None,
            pending_paint_fragments: Vec::new(),
            pending_page_side_effects: Vec::new(),
            float_paint_capture_depth: 0,
            preserve_scoped_paint_public_order: false,
            defer_next_block_decoration_promotion: false,
        };
        builder.rebuild_empty_current_page_context();
        builder.initial_viewport_context = builder.current_page_context;
        builder
    }

    pub(in crate::layout) fn layout_page_box(
        &mut self,
        page_box: &box_tree::PageBox<'a>,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.prepare_counter_plan(&page_box.counter_events);
        self.install_footnotes(page_box);
        match self.footnote_layout_mode {
            FootnoteLayoutMode::Measure => self.measured_footnotes.clear(),
            FootnoteLayoutMode::Render => {
                self.rendered_footnotes.clear();
                self.rendered_footnote_measurements.clear();
            }
        }
        self.suppressed_named_strings_before.clear();
        self.suppressed_named_strings_after.clear();
        for event in &page_box.suppressed_named_string_events {
            let target = event.target;
            let destination = match target {
                box_tree::SuppressedNamedStringEventTarget::BeforeElement(element) => &mut self
                    .suppressed_named_strings_before
                    .entry(element)
                    .or_default(),
                box_tree::SuppressedNamedStringEventTarget::AfterElement(element) => &mut self
                    .suppressed_named_strings_after
                    .entry(element)
                    .or_default(),
            };
            destination.push(event.clone());
        }
        self.page_counter_initial_values = page_box
            .counter_events
            .iter()
            .find(|event| event.source == box_tree::CounterEventSource::Principal)
            .and_then(|event| {
                self.counter_plan
                    .values_at_origin
                    .get(&CounterOriginKey::new(
                        event.element,
                        box_tree::CounterEventSource::Principal,
                    ))
            })
            .map(|stacks| {
                stacks
                    .iter()
                    .filter_map(|(name, values)| {
                        values.last().cloned().map(|value| (name.clone(), value))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for child in &page_box.children {
            // The document root has the initial containing block as its
            // percentage-height containing block. That definite page-area
            // basis applies while resolving the root itself; normal block
            // layout then derives the basis for its descendants from the
            // root's used height.
            // <https://www.w3.org/TR/CSS2/visudet.html#root-height>
            let is_document_root = child
                .element_parts()
                .is_some_and(|(element, _, _, _)| element.tag.eq_ignore_ascii_case("html"));
            if is_document_root {
                // The root's percentage-height basis belongs to the first
                // page fragment that receives its normal-flow descendants.
                // A descendant can select that first page through `page`
                // before the root has laid out its own box; choosing the
                // destination context here prevents `html { height: 100% }`
                // from retaining the provisional default page area's height
                // and then fragmenting across several named pages.
                // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                // <https://www.w3.org/TR/CSS2/visudet.html#root-height>
                let root_page_start =
                    resolved_formatting_box_page_boundary_values(child, None).start;
                if let Some(root_page_start) = root_page_start {
                    self.enter_page_name_scope_for_value(Some(&root_page_start));
                }
                // Select root/body background propagation before descendant
                // layout. When the root paints the canvas, the body remains
                // an ordinary box; discovering the root only after children
                // would otherwise suppress the body's own background.
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
                if let Some((_, _, style, _)) = child.element_parts()
                    && style.visibility == Visibility::Visible
                    && (style
                        .background
                        .background_color
                        .visible_color(style.color)
                        .is_some()
                        || style.background.background_image.is_image()
                        || style
                            .background
                            .background_layers
                            .iter()
                            .any(|layer| layer.image.is_image()))
                {
                    self.document_canvas_background = Some(DocumentCanvasBackground {
                        style: canvas_background_style(style),
                        source: DocumentCanvasBackgroundSource::Root,
                    });
                }
                self.block_percentage_context_stack.push_context(
                    DescendantBlockPercentageContext::definite(
                        content_box_pt(self.page_area_height()),
                        BlockSizeBasisSource::InitialContainingBlock,
                    ),
                );
            }
            // The page box is the root formatting context rather than a CSS
            // block container, so its children do not pass through the usual
            // block-child traversal that dispatches floats. A floated
            // document root still participates in the initial containing
            // block's float formatting context.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            if is_document_root
                && let Some((element, _, style, children)) = child.element_parts()
                && matches!(style.position, Position::Absolute | Position::Fixed)
            {
                let root_layout_style = self.principal_flow.root_layout_style(style);
                // The page-box root bypasses ordinary block-child traversal,
                // but its own positioning scheme is still resolved against
                // the initial containing block. Preserve the hypothetical
                // normal-flow source while dispatching it so auto insets use
                // the root's static position, including signed margins.
                // <https://drafts.csswg.org/css-position-3/#absolute-positioning>
                // and <https://drafts.csswg.org/css-position-3/#fixed-positioning>
                self.layout_positioned_block_with_static_source(
                    element,
                    &root_layout_style,
                    stylesheets,
                    Some(children),
                    None,
                );
                // Ordinary root flow flushes any positioned descendants at
                // its block-layout boundary.  A positioned root has no such
                // in-flow boundary, so commit its principal layer here.
                self.flush_positioned_layers();
            } else if is_document_root
                && let Some((element, signature, style, children)) = child.element_parts()
                && style.float != Float::None
            {
                let root_layout_style = self.principal_flow.root_layout_style(style);
                let mut float_run = self.float_run_state();
                self.layout_floating_child(
                    element,
                    signature.clone(),
                    &root_layout_style,
                    Some(children),
                    None,
                    stylesheets,
                    FloatPlacementAxes::new(
                        self.initial_containing_block_writing_mode,
                        root_layout_style.used_direction(),
                    ),
                    &mut float_run,
                );
            } else if is_document_root
                && let Some((element, signature, style, children)) = child.element_parts()
            {
                let root_layout_style = self.principal_flow.root_layout_style(style);
                self.layout_element_box(
                    element,
                    &root_layout_style,
                    stylesheets,
                    signature.clone(),
                    &box_tree::BoxSource::Principal,
                    &[],
                    children,
                );
            } else {
                self.layout_formatting_box(child, stylesheets);
            }
            if is_document_root {
                self.block_percentage_context_stack.pop();
            }
        }
    }
}
