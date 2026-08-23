use std::collections::HashSet;

use super::*;
use crate::layout::assets::PendingPositionedFragmentation;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use crate::units::LayoutSize;

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
            definite_block_size_stack: Vec::new(),
            replayed_flex_item_percentage_height_bases: Vec::new(),
            table_wrapper_block_size_overrides: Vec::new(),
            positioned_table_sizing: Vec::new(),
            truncate_page_start_margins: false,
            avoid_inside_retry_depth: 0,
            out_of_flow_prebreak_suppression_depth: 0,
            layout_pass_kind: LayoutPassKind::Normal,
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
                self.definite_block_size_stack
                    .push(PercentageBasis::definite_from(
                        content_box_pt(self.page_area_height()),
                        BlockSizeBasisSource::InitialContainingBlock,
                    ));
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
                self.definite_block_size_stack.pop();
            }
        }
    }

    pub(in crate::layout) fn next_paint_source_order(&mut self) -> usize {
        let source_order = self.next_paint_source_order;
        self.next_paint_source_order += 1;
        source_order
    }

    /// Resolves font-metric-relative computed lengths in a formatting tree.
    ///
    /// CSS Values defines `ch` from the used font's "0" glyph advance. The
    /// box tree is built before fonts are resolved, so layout performs this
    /// used-value projection after `FontSystem` is available and before any
    /// formatting context consumes sizes:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
    /// <https://www.w3.org/TR/css-cascade-5/#used>.
    pub(in crate::layout) fn resolve_font_metric_lengths_in_page_box(
        &mut self,
        page_box: &mut box_tree::MutablePageBox<'_>,
        _parent_style: &ComputedStyle,
    ) {
        // The document root has no element parent. Its font-relative
        // `font-size` terms must therefore use the CSS initial values, not
        // Quire's outer rendering defaults. This mirrors the root cascade
        // base and keeps `rem` in a root `font-size` anchored to the initial
        // font size:
        // <https://www.w3.org/TR/css-cascade-5/#root-element> and
        // <https://www.w3.org/TR/css-values-4/#em>.
        let document_root_parent = ComputedStyle::initial();
        let parent_ch_advance = self.ch_advance_for_style(
            &document_root_parent,
            page_box.children.iter().any(|child| {
                self.box_requires_parent_ch_advance(child, document_root_parent.font_size)
            }),
        );
        self.root_metrics_require_selected_font = page_box
            .children
            .iter()
            .any(Self::box_requires_root_font_metrics);
        let mut root_metrics = self.root_metric_state;
        let document_root_parent_metrics = css::FontRelativeLengthBasis::new(
            layout_pt(document_root_parent.font_size),
            parent_ch_advance,
        );
        for child in &mut page_box.children {
            self.resolve_deferred_font_metrics_in_box(
                child,
                document_root_parent_metrics,
                &mut root_metrics,
            );
        }
        self.root_metric_state = root_metrics;
    }

    fn resolve_deferred_font_metrics_in_style(
        &mut self,
        style: &mut ComputedStyle,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) -> css::FontRelativeLengthBasis {
        let establishes_root_metrics = matches!(*root_metrics, RootMetricState::Bootstrapping);
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            parent_metrics,
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            root_metrics.font_size_basis(),
        );
        style
            .line_height_value
            .resolve_em_relative_lengths(layout_pt(style.font_size));
        let (line_height, _, _) = style.line_height_value.clone().projected(style.font_size);
        style.line_height = line_height;
        style.root_font_size = root_metrics
            .font_size_basis()
            .map_or(style.font_size, |basis| basis.font_size.points());
        style.finalize_computed_font_relative_lengths();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            (establishes_root_metrics && self.root_metrics_require_selected_font)
                || style.requires_ch_advance()
                || pseudo_requires_parent_ch,
        );
        // A selected-font metric lookup interns that font in the document.
        // Do not perform one for an otherwise metric-free style: an empty
        // block with the initial `normal` line-height must not retain a font.
        // The existing metric-dependency traversal covers every `ch`-based
        // term, and selected-font metric expressions share that used-value
        // resolution path.
        let requires_selected_font_metrics = (establishes_root_metrics
            && self.root_metrics_require_selected_font)
            || style.requires_selected_font_metrics();
        let ic_advance = if requires_selected_font_metrics {
            self.font_system.ic_advance_for_style(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        };
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_selected_font_metric_lengths(css::SelectedFontMetricLengthBasis::new(
            ch_advance,
            ic_advance,
            layout_pt(x_height),
            layout_pt(cap_height),
        ));
        if let RootMetricState::Resolved(root_metrics) = self.root_metric_state {
            style.root_font_size = root_metrics.basis().font_size.points();
            style.resolve_root_font_metric_lengths(root_metrics.basis());
        }
        style.resolve_line_height_relative_lengths();
        if establishes_root_metrics {
            root_metrics.establish(ResolvedRootFontMetrics::measured_for_document_root(
                css::RootFontMetricLengthBasis {
                    font_size: layout_pt(style.font_size),
                    ch_advance,
                    x_height: layout_pt(x_height),
                    cap_height: layout_pt(cap_height),
                    ic_advance,
                    line_height: layout_pt(style.line_height),
                },
            ));
        }
        let root_font_metrics = root_metrics.resolved().basis();
        style.root_font_size = root_font_metrics.font_size.points();
        style.resolve_root_font_metric_lengths(root_font_metrics);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        let font_metrics =
            css::FontRelativeLengthBasis::new(layout_pt(style.font_size), ch_advance)
                .with_selected_font_metrics(layout_pt(x_height), layout_pt(cap_height), ic_advance)
                .with_line_height(layout_pt(style.line_height));
        if let Some(style) = &mut style.marker_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        // All font-, root-font-, viewport-, and selected-font-metric terms
        // above are still in ordinary CSS units. Apply zoom only once they
        // have their concrete used-length values, so inherited text styles
        // and percentage bases retain their CSS semantics.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        font_metrics
    }

    pub(in crate::layout) fn ch_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        required: bool,
    ) -> LayoutLength {
        if required {
            self.font_system.ch_advance(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        }
    }

    fn box_requires_parent_ch_advance(
        &self,
        formatting_box: &box_tree::MutableFormattingBox<'_>,
        parent_font_size: f32,
    ) -> bool {
        formatting_box
            .style()
            .deferred_font_size
            .requires_parent_ch_advance(parent_font_size)
    }

    fn children_require_parent_ch_advance(
        &self,
        children: &[box_tree::MutableFormattingBox<'_>],
        parent_font_size: f32,
    ) -> bool {
        children
            .iter()
            .any(|child| self.box_requires_parent_ch_advance(child, parent_font_size))
    }

    fn children_require_parent_selected_font_metrics(
        &self,
        children: &[box_tree::MutableFormattingBox<'_>],
    ) -> bool {
        children
            .iter()
            .any(Self::box_requires_parent_selected_font_metrics)
    }

    fn box_requires_parent_selected_font_metrics(
        formatting_box: &box_tree::MutableFormattingBox<'_>,
    ) -> bool {
        let children_require = |children: &[box_tree::MutableFormattingBox<'_>]| {
            children
                .iter()
                .any(Self::box_requires_parent_selected_font_metrics)
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.run_in_children)
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                box_.style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.children)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Text(box_) => box_
                .style
                .deferred_font_size
                .requires_parent_selected_font_metrics(),
            box_tree::MutableFormattingBox::Table(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
        }
    }

    fn selected_font_metric_basis_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> css::FontRelativeLengthBasis {
        let ch_advance = self.font_system.ch_advance(style);
        let x_height = self.font_system.used_x_height_for_style(style);
        let cap_height = self.font_system.used_cap_height_for_style(style);
        let ic_advance = self.font_system.ic_advance_for_style(style);
        css::FontRelativeLengthBasis::new(layout_pt(style.font_size), ch_advance)
            .with_selected_font_metrics(x_height, cap_height, ic_advance)
            .with_line_height(layout_pt(style.line_height))
    }

    /// Finds root-relative selected-font units before resolving the root
    /// style, so a metric-free document does not intern a font merely to
    /// create a fallback snapshot.
    fn box_requires_root_font_metrics(formatting_box: &box_tree::MutableFormattingBox<'_>) -> bool {
        let children_require = |children: &[box_tree::MutableFormattingBox<'_>]| {
            children.iter().any(Self::box_requires_root_font_metrics)
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.run_in_children)
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                box_.style.requires_root_font_metrics() || children_require(&box_.children)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
                    || box_
                        .table_fragment
                        .as_ref()
                        .is_some_and(Self::table_fragment_requires_root_font_metrics)
            }
            box_tree::MutableFormattingBox::Text(box_) => box_.style.requires_root_font_metrics(),
            box_tree::MutableFormattingBox::Table(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
                    || Self::table_fragment_requires_root_font_metrics(&box_.fragment)
            }
        }
    }

    fn table_fragment_requires_root_font_metrics(
        fragment: &box_tree::MutableTableFragment<'_>,
    ) -> bool {
        fragment.rows.iter().any(|row| {
            row.row_groups
                .iter()
                .filter_map(|group| group.style.as_deref())
                .any(ComputedStyle::requires_root_font_metrics)
                || row
                    .style
                    .as_deref()
                    .is_some_and(ComputedStyle::requires_root_font_metrics)
                || row.cells.iter().any(|cell| {
                    cell.style
                        .as_deref()
                        .is_some_and(ComputedStyle::requires_root_font_metrics)
                        || cell
                            .children
                            .iter()
                            .any(Self::box_requires_root_font_metrics)
                })
        }) || fragment.captions.iter().any(|caption| {
            caption
                .style
                .as_deref()
                .is_some_and(ComputedStyle::requires_root_font_metrics)
                || caption
                    .children
                    .iter()
                    .any(Self::box_requires_root_font_metrics)
        }) || fragment.columns.iter().any(|column| {
            column
                .group
                .as_ref()
                .and_then(|group| group.style.as_deref())
                .is_some_and(ComputedStyle::requires_root_font_metrics)
                || column
                    .style
                    .as_deref()
                    .is_some_and(ComputedStyle::requires_root_font_metrics)
        })
    }

    fn resolve_deferred_font_metrics_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) {
        let mut recurse = |builder: &mut Self,
                           children: &mut Vec<box_tree::MutableFormattingBox<'_>>,
                           style: &mut ComputedStyle| {
            let font_metrics =
                builder.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics);
            let font_size = font_metrics.font_size().points();
            let child_requires_selected_metrics =
                builder.children_require_parent_selected_font_metrics(children);
            let ch_advance = builder.ch_advance_for_style(
                style,
                child_requires_selected_metrics
                    || builder.children_require_parent_ch_advance(children, font_size),
            );
            let child_metrics = if child_requires_selected_metrics {
                builder.selected_font_metric_basis_for_style(style)
            } else {
                font_metrics.with_ch_advance(ch_advance)
            };
            for child in children {
                builder.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
            }
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch = self
                    .children_require_parent_ch_advance(&box_.run_in_children, font_size)
                    || self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics = self
                    .children_require_parent_selected_font_metrics(&box_.run_in_children)
                    || self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                for child in &mut box_.run_in_children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics =
                    self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_deferred_font_metrics_in_table_fragment(
                        fragment,
                        child_metrics,
                        root_metrics,
                    );
                }
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_metrics,
                    root_metrics,
                );
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics =
                    self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                self.resolve_deferred_font_metrics_in_table_fragment(
                    &mut box_.fragment,
                    child_metrics,
                    root_metrics,
                );
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
        }
    }

    /// Resolve a table fragment in table-tree inheritance order.
    ///
    /// A row group is the parent of its rows, and a row is the parent of its
    /// cells, including the anonymous wrappers generated by the table fixup
    /// algorithm. Resolving an anonymous row before its row group would make
    /// `font-size: inherit` use the table's font instead of the row group's.
    /// <https://drafts.csswg.org/css-tables/#fixup-algorithm>
    fn resolve_deferred_font_metrics_in_table_fragment(
        &mut self,
        fragment: &mut box_tree::MutableTableFragment<'_>,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) {
        for row in &mut fragment.rows {
            let mut row_parent_metrics = parent_metrics;
            for group in &mut row.row_groups {
                if let Some(style) = &mut group.style {
                    row_parent_metrics = self.resolve_deferred_font_metrics_in_style(
                        style,
                        row_parent_metrics,
                        root_metrics,
                    );
                }
            }
            let row_metrics = row
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(
                        style,
                        row_parent_metrics,
                        root_metrics,
                    )
                })
                .unwrap_or(row_parent_metrics);
            for cell in &mut row.cells {
                let cell_metrics = cell
                    .style
                    .as_deref_mut()
                    .map(|style| {
                        self.resolve_deferred_font_metrics_in_style(
                            style,
                            row_metrics,
                            root_metrics,
                        )
                    })
                    .unwrap_or(row_metrics);
                for child in &mut cell.children {
                    self.resolve_deferred_font_metrics_in_box(child, cell_metrics, root_metrics);
                }
            }
        }
        for caption in &mut fragment.captions {
            let caption_metrics = caption
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics)
                })
                .unwrap_or(parent_metrics);
            for child in &mut caption.children {
                self.resolve_deferred_font_metrics_in_box(child, caption_metrics, root_metrics);
            }
        }
        for column in &mut fragment.columns {
            let group_metrics = column
                .group
                .as_mut()
                .and_then(|group| group.style.as_deref_mut())
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics)
                })
                .unwrap_or(parent_metrics);
            if let Some(style) = &mut column.style {
                self.resolve_deferred_font_metrics_in_style(style, group_metrics, root_metrics);
            }
        }
    }

    pub(in crate::layout) fn resolve_style_viewport_lengths(
        style: &mut ComputedStyle,
        viewport: LayoutSize,
        container_physical: LayoutSize,
    ) {
        style.resolve_viewport_lengths_for_viewport_and_container(viewport, container_physical);
        if let Some(style) = &mut style.marker_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
        if let Some(style) = &mut style.before_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
        if let Some(style) = &mut style.after_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
    }

    pub(in crate::layout) fn style_with_current_viewport_lengths(
        &self,
        style: &impl css::CascadedStyleSource,
    ) -> css::ZoomedLayoutStyle {
        let mut style = css::LayoutStyle::from_computed(style);
        self.resolve_style_current_viewport_lengths(&mut style);
        style.into_zoomed()
    }

    pub(in crate::layout) fn style_with_current_used_lengths(
        &mut self,
        style: &impl css::CascadedStyleSource,
    ) -> css::ZoomedLayoutStyle {
        let mut style = css::LayoutStyle::from_computed(style);
        self.resolve_style_current_viewport_lengths(&mut style);
        // A frozen box tree can retain an `em`/`rem` expression until it is
        // replayed for intrinsic sizing or an isolated formatting context.
        // These units are computed from the element's resolved font sizes,
        // independently of any containing-block percentage basis.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        style.finalize_computed_font_relative_lengths();
        self.resolve_style_font_metric_lengths(&mut style);
        let mut style = style.into_zoomed();
        // Viewport and font-relative units can turn an authored box edge into
        // a fixed computed length after cascading. Keep the legacy used-edge
        // cache synchronized so its fixed-edge fast path does not retain the
        // pre-resolution value.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-cascade-5/#computed>
        synchronize_resolved_fixed_box_edge_cache(&mut style);
        style
    }

    pub(in crate::layout) fn resolve_style_current_viewport_lengths(
        &self,
        style: &mut ComputedStyle,
    ) {
        // Document viewport-relative lengths resolve against the immutable
        // initial containing block. An embedded document instead has the
        // iframe's finite browsing-context viewport, even though its static
        // layout surface is deliberately made tall to avoid fragmentation.
        // A destination page may otherwise have a different used page area
        // through a named or spread `@page` rule, but that changes layout
        // geometry rather than the document viewport.
        // <https://www.w3.org/TR/css-page-3/#page-model>
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        let viewport = self
            .iframe_viewport
            .map(|context| context.viewport.layout_size())
            .unwrap_or_else(|| {
                LayoutSize::new(
                    self.initial_viewport_context.area_width(),
                    self.initial_viewport_context.area_height(),
                )
            });
        Self::resolve_style_viewport_lengths(
            style,
            viewport,
            self.current_container_unit_physical(viewport),
        );
    }

    /// Select the nearest eligible query container independently for the
    /// physical width and height axes. The CSS unit resolver maps those
    /// physical values to `cqi`/`cqb` using the consuming style's writing
    /// mode.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    fn current_container_unit_physical(&self, fallback: LayoutSize) -> LayoutSize {
        let mut width = None;
        let mut height = None;
        for context in self.container_unit_contexts.iter().rev().copied() {
            if width.is_none() && context.supplies_physical_width() {
                width = Some(context.physical_width.points());
            }
            if height.is_none() && context.supplies_physical_height() {
                height = Some(context.physical_height.points());
            }
            if width.is_some() && height.is_some() {
                break;
            }
        }
        LayoutSize::new(
            width.unwrap_or(fallback.width),
            height.unwrap_or(fallback.height),
        )
    }

    /// Enter one layout-time query-container scope after its used content box
    /// has been resolved. `container-type: normal` deliberately adds no
    /// record, keeping unrelated descendants out of the selection walk.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    pub(in crate::layout) fn push_container_unit_context(
        &mut self,
        style: &ComputedStyle,
        physical_width: PhysicalContentWidth,
        physical_height: PhysicalContentHeight,
    ) -> bool {
        if matches!(style.container_type, ContainerType::Normal) {
            return false;
        }
        self.container_unit_contexts.push(ContainerUnitContext {
            physical_width,
            physical_height,
            writing_mode: style.writing_mode,
            container_type: style.container_type,
        });
        true
    }

    pub(in crate::layout) fn pop_container_unit_context(&mut self, active: bool) {
        if active {
            self.container_unit_contexts
                .pop()
                .expect("container unit scopes must be lexically balanced");
        }
    }

    pub(in crate::layout) fn resolve_font_metric_lengths_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
    ) {
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.run_in_children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_font_metric_lengths_in_table_fragment(fragment);
                }
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                self.resolve_font_metric_lengths_in_table_fragment(&mut box_.fragment);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
        }
    }

    pub(in crate::layout) fn resolve_font_metric_lengths_in_table_fragment(
        &mut self,
        fragment: &mut box_tree::MutableTableFragment<'_>,
    ) {
        for row in &mut fragment.rows {
            if let Some(style) = &mut row.style {
                self.resolve_style_font_metric_lengths(style);
            }
            for group in &mut row.row_groups {
                if let Some(style) = &mut group.style {
                    self.resolve_style_font_metric_lengths(style);
                }
            }
            for cell in &mut row.cells {
                if let Some(style) = &mut cell.style {
                    self.resolve_table_cell_style_font_metric_lengths(style);
                }
                for child in &mut cell.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
        }
        for caption in &mut fragment.captions {
            if let Some(style) = &mut caption.style {
                self.resolve_style_font_metric_lengths(style);
            }
            for child in &mut caption.children {
                self.resolve_font_metric_lengths_in_box(child);
            }
        }
        for column in &mut fragment.columns {
            if let Some(style) = &mut column.style {
                self.resolve_style_font_metric_lengths(style);
            }
            if let Some(group) = &mut column.group
                && let Some(style) = &mut group.style
            {
                self.resolve_style_font_metric_lengths(style);
            }
        }
    }

    pub(in crate::layout) fn build_frozen_child_boxes_with_font_metrics<'b>(
        &mut self,
        element: &'b Element,
        stylesheets: &Stylesheets<'_>,
        parent_style: &impl css::CascadedStyleSource,
        ancestors: &[ElementSignature],
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
        let parent_style = css::CascadedStyleSource::cascaded_style(parent_style);
        let mut child_boxes = box_tree::build_child_boxes_with_font_metrics(
            element,
            stylesheets,
            parent_style,
            ancestors,
            &mut self.font_system,
        );
        for child in &mut child_boxes {
            self.resolve_font_metric_lengths_in_box(child);
        }
        box_tree::freeze_child_boxes(child_boxes)
    }

    pub(in crate::layout) fn build_frozen_child_boxes_with_current_ancestors<'b>(
        &mut self,
        element: &'b Element,
        stylesheets: &Stylesheets<'_>,
        parent_style: &impl css::CascadedStyleSource,
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
        let parent_style = css::CascadedStyleSource::cascaded_style(parent_style);
        let mut child_boxes = {
            let ancestors = &self.ancestors;
            let font_system = &mut self.font_system;
            box_tree::build_child_boxes_with_font_metrics(
                element,
                stylesheets,
                parent_style,
                ancestors,
                font_system,
            )
        };
        for child in &mut child_boxes {
            self.resolve_font_metric_lengths_in_box(child);
        }
        box_tree::freeze_child_boxes(child_boxes)
    }

    pub(in crate::layout) fn resolve_style_font_metric_lengths(
        &mut self,
        style: &mut ComputedStyle,
    ) {
        self.resolve_deferred_root_font_metric_font_size(style);
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            style.requires_ch_advance() || pseudo_requires_parent_ch,
        );
        // Styles created during layout (notably positioned descendants) pass
        // through this late resolution path. Resolve every selected-font box
        // metric here, not only `ch`, so their used sizes agree with styles
        // prepared during the structural font-metric pass.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        let requires_selected_font_metrics = style.requires_selected_font_metrics();
        let ic_advance = if requires_selected_font_metrics {
            self.font_system.ic_advance_for_style(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        };
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_selected_font_metric_lengths(css::SelectedFontMetricLengthBasis::new(
            ch_advance,
            ic_advance,
            layout_pt(x_height),
            layout_pt(cap_height),
        ));
        if let RootMetricState::Resolved(root_metrics) = self.root_metric_state {
            style.root_font_size = root_metrics.basis().font_size.points();
            style.resolve_root_font_metric_lengths(root_metrics.basis());
        }
        style.resolve_line_height_relative_lengths();
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        if let Some(style) = &mut style.marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
    }

    /// Correct a lazily built descendant's provisional `font-size` once the
    /// document root has established its used font-size and selected-font
    /// metric snapshot.
    ///
    /// CSS cascade intentionally retains a deferred font size while a box is
    /// being built. A child constructed after the structural prepass has not
    /// passed through that prepass, so root-relative terms must consume the
    /// typed snapshot retained by the builder instead of their provisional
    /// parent-sized fallback.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    fn resolve_deferred_root_font_metric_font_size(&mut self, style: &mut ComputedStyle) {
        if !style.deferred_font_size.requires_root_font_metrics()
            && !style.deferred_font_size.requires_document_root_font_size()
        {
            return;
        }
        let RootMetricState::Resolved(root_metrics) = self.root_metric_state else {
            // The structural traversal has not yet reached the document root's
            // used font-size boundary. The bootstrap path intentionally uses
            // the CSS initial root fallback; the normal recursive pass will
            // revisit this descendant after establishing the snapshot.
            return;
        };
        let provisional_parent_font_size = style.font_size;
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            css::FontRelativeLengthBasis::new(
                layout_pt(provisional_parent_font_size),
                css::fallback_ch_advance_for_style(style),
            ),
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            Some(root_metrics.basis()),
        );
    }

    /// Resolves lazily cascaded `font-size` values that depend on the parent
    /// selected font. Structural traversal normally performs this work, but
    /// positioned and replayed boxes are constructed after that traversal.
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>
    fn resolve_deferred_parent_font_metric_font_size(
        &mut self,
        style: &mut ComputedStyle,
        parent_style: &ComputedStyle,
    ) {
        if !style
            .deferred_font_size
            .requires_parent_selected_font_metrics()
        {
            return;
        }
        let parent_metrics = self.selected_font_metric_basis_for_style(parent_style);
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            parent_metrics,
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            self.root_metric_state.font_size_basis(),
        );
    }

    pub(in crate::layout) fn resolve_table_cell_style_font_metric_lengths(
        &mut self,
        style: &mut ComputedStyle,
    ) {
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            style.requires_ch_advance() || pseudo_requires_parent_ch,
        );
        style.resolve_font_metric_lengths_preserving_box_block_sizes(ch_advance);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        if let Some(style) = &mut style.marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
    }

    pub(in crate::layout) fn style_for_layout_element_with_parent_font_metrics(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
    ) -> ComputedStyle {
        let ancestors = self.ancestors.clone();
        self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            element,
            signature,
            stylesheets,
            parent,
            &ancestors,
        )
    }

    pub(in crate::layout) fn style_for_layout_element_with_parent_font_metrics_and_ancestors(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let signature = layout_element_signature(element, signature, parent);
        let inline_style = element.attrs.get("style").map(String::as_str);
        let mut style = style_for_layout_signature_with_parent_ch_advance(
            signature.clone(),
            inline_style,
            stylesheets,
            parent,
            ancestors,
            Some(parent_ch_advance),
        );
        if style
            .deferred_font_size
            .requires_parent_ch_advance(inheritance_source.font_size)
        {
            parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
            style = style_for_layout_signature_with_parent_ch_advance(
                signature.clone(),
                inline_style,
                stylesheets,
                parent,
                ancestors,
                Some(parent_ch_advance),
            );
        }
        let pseudo_parent_ch_advance = css::fallback_ch_advance_for_style(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        if style.pseudo_styles_require_parent_ch_advance() {
            let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
            css::apply_pseudo_rules_with_parent_ch_advance(
                &mut style,
                &signature,
                stylesheets,
                ancestors,
                pseudo_parent_ch_advance,
            );
        }
        self.resolve_deferred_parent_font_metric_font_size(&mut style, &inheritance_source);
        self.resolve_deferred_root_font_metric_font_size(&mut style);
        self.resolve_style_font_metric_lengths(&mut style);
        // Computed styles deliberately do not inherit text-decoration
        // longhands.  At this layout boundary, materialize the decorating
        // ancestors as used-style paint layers instead.  Keeping this after
        // pseudo and font-metric resolution preserves the decorating box's
        // resolved paint parameters while allowing descendant text to retain
        // its own computed style.
        //
        // CSS Text Decoration Level 4 § 2.1, Line Decoration: text
        // decorations propagate through in-flow descendants, rather than
        // behaving as inherited CSS properties.
        if let Some(parent_style) = parent {
            style =
                TextDecorationPropagationContext::from_style(parent_style).used_child_style(&style);
        }
        style
    }

    pub(in crate::layout) fn style_for_signature_with_parent_font_metrics(
        &mut self,
        signature: ElementSignature,
        inline_style: Option<&str>,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let mut style = css::style_for_element_with_signature_and_parent_ch_advance(
            signature.clone(),
            inline_style,
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        if style
            .deferred_font_size
            .requires_parent_ch_advance(inheritance_source.font_size)
        {
            parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
            style = css::style_for_element_with_signature_and_parent_ch_advance(
                signature.clone(),
                inline_style,
                stylesheets,
                parent,
                ancestors,
                parent_ch_advance,
            );
        }
        let pseudo_parent_ch_advance = css::fallback_ch_advance_for_style(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        if style.pseudo_styles_require_parent_ch_advance() {
            let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
            css::apply_pseudo_rules_with_parent_ch_advance(
                &mut style,
                &signature,
                stylesheets,
                ancestors,
                pseudo_parent_ch_advance,
            );
        }
        self.resolve_deferred_parent_font_metric_font_size(&mut style, &inheritance_source);
        self.resolve_deferred_root_font_metric_font_size(&mut style);
        self.resolve_style_font_metric_lengths(&mut style);
        style
    }

    pub(in crate::layout) fn layout_formatting_box(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_formatting_box_with_parent_decoration(formatting_box, stylesheets, None);
    }

    /// Lay out a frozen formatting box with the decoration origins propagated
    /// by its in-flow parent.
    ///
    /// Frozen box trees retain computed styles, so normal CSS inheritance
    /// cannot carry line-decoration provenance across this boundary. Resolve
    /// the layout-only propagation context here before dispatching the box's
    /// formatting algorithm.
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    pub(in crate::layout) fn layout_formatting_box_with_parent_decoration(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        parent_style: Option<&ComputedStyle>,
    ) {
        let decoration_context = parent_style
            .map(TextDecorationPropagationContext::from_style)
            .unwrap_or_default();
        match formatting_box {
            box_tree::FormattingBox::Block(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                // The document box can recur through an anonymous root-flow
                // wrapper before it reaches its own descendants. Keep the
                // stored style computed, but apply the principal-flow axes at
                // every formatting entry for that one principal box.
                let layout_style = box_
                    .core
                    .element
                    .tag
                    .eq_ignore_ascii_case("html")
                    .then_some(())
                    .filter(|_| matches!(&box_.core.source, box_tree::BoxSource::Principal))
                    .map(|_| self.principal_flow.root_layout_style(&used_style));
                self.layout_element_box(
                    box_.core.element,
                    layout_style.as_ref().unwrap_or(&used_style),
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &box_.run_in_children,
                    &box_.core.children,
                );
            }
            box_tree::FormattingBox::Inline(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                let used_style = decoration_context.used_child_style(&box_.style);
                self.layout_anonymous_block(&used_style, &box_.children, stylesheets, None);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .layout_inline_split_block_context_with_parent_decoration(
                    box_,
                    stylesheets,
                    parent_style,
                ),
            box_tree::FormattingBox::AtomicInline(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Table(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_table_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &box_.core.children,
                    &box_.fragment,
                );
            }
            box_tree::FormattingBox::Flex(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Replaced(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Text(box_) => {
                let used_style = decoration_context.used_child_style(&box_.style);
                let text = normalized_text_for_style(&box_.text, &used_style);
                if !text.is_empty() {
                    self.layout_text_block(&text, &used_style, 0.0, 0.0, None);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.capture_suppressed_named_strings_before(element.id);
                self.layout_element_with_child_boxes_and_run_ins(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    Some(children),
                );
                self.capture_suppressed_named_strings_after(element.id);
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    pseudo.kind.counter_event_source(),
                    stylesheets,
                    run_in_children,
                    Some(children),
                    None,
                    PrincipalBoxPaintMode::RootPaints,
                );
            }
        }
        self.ancestors.pop();
    }

    /// Lays out a table formatting box through the generic element entry path.
    ///
    /// CSS Paged Media applies the `page` property to normal-flow boxes before
    /// their page context is generated, and CSS Tables uses a table wrapper/grid
    /// fragment for layout. This preserves the prebuilt durable table fragment
    /// while still applying named-page, counter, running-element, and
    /// break-inside entry behavior:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_table_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.capture_suppressed_named_strings_before(element.id);
                self.layout_element_with_child_boxes_run_ins_and_table_fragment(
                    element,
                    style,
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                );
                self.capture_suppressed_named_strings_after(element.id);
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    pseudo.kind.counter_event_source(),
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                    PrincipalBoxPaintMode::RootPaints,
                );
            }
        }
        self.ancestors.pop();
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_generated_pseudo_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        source: box_tree::CounterEventSource,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        let counter_scope = self.begin_pseudo_counter_scope(element, source, style);
        self.element_side_effect_suppression_depth += 1;
        // A sole image in a tree-abiding ::before/::after box is anonymous
        // replaced content inside that pseudo's decorated box. Keep the
        // pseudo's authored dimensions for its own background/border while
        // the image payload retains its zoomed natural size. Principal
        // `content: <image>` remains a replacement of the element itself.
        // <https://www.w3.org/TR/css-content-3/#content-property>
        let mut pseudo_content_style;
        let style = if matches!(
            style.content,
            css::Content::Replacement {
                image: css::GeneratedContentPart::Image { .. },
                ..
            }
        ) {
            pseudo_content_style = style.clone();
            pseudo_content_style.object_fit = css::ObjectFit::None;
            pseudo_content_style.object_position = css::BackgroundPosition::INITIAL;
            &pseudo_content_style
        } else {
            style
        };
        let consuming_root_canvas =
            !style.display.is_block_level() && self.begin_root_inline_canvas_continuation(element);
        let previous_root_pseudo_block_projection = self.root_pseudo_block_projection;
        let root_before_principal_track_start = (element.tag.eq_ignore_ascii_case("html")
            && source == box_tree::CounterEventSource::Before
            && style.writing_mode == WritingMode::HorizontalTb
            && self.principal_flow.writing_mode == WritingMode::VerticalLr)
            .then_some(self.content_left);
        if element.tag.eq_ignore_ascii_case("html") {
            self.root_pseudo_block_projection =
                match (style.writing_mode, self.principal_flow.writing_mode) {
                    // A root pseudo retains its horizontal computed style, but
                    // a propagated vertical-lr body establishes the initial
                    // containing block's used principal flow. Project this one
                    // direct root child through that flow so the ordinary child
                    // traversal can advance the horizontal block track before
                    // entering the body.
                    // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                    (WritingMode::HorizontalTb, WritingMode::VerticalLr)
                        if source == box_tree::CounterEventSource::Before =>
                    {
                        Some(RootPseudoBlockProjection {
                            element: element.id,
                            block_start: PhysicalSide::Left,
                            block_end_inset: layout_pt(0.0),
                        })
                    }
                    // The inverse projection retains the propagated body's
                    // physical block-end canvas inset while a vertical root
                    // pseudo participates in a horizontal principal flow.
                    (WritingMode::VerticalLr, WritingMode::HorizontalTb) => {
                        Some(RootPseudoBlockProjection {
                            element: element.id,
                            block_start: PhysicalSide::Left,
                            block_end_inset: self.principal_body_block_end_inset,
                        })
                    }
                    _ => None,
                };
        }
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            principal_box_paint_mode,
        );
        if element.tag.eq_ignore_ascii_case("html")
            && source == box_tree::CounterEventSource::Before
            && style.writing_mode == WritingMode::HorizontalTb
            && self.principal_flow.writing_mode == WritingMode::VerticalLr
        {
            // The generated root pseudo is laid out directly rather than as a
            // normal child traversal entry. It therefore must explicitly
            // consume its committed margin-box span from the propagated
            // body's horizontal track. The outcome span already includes the
            // projected logical block-end margin; adding a physical margin
            // here would count the horizontal pseudo's margin twice.
            // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
            let advance = self
                .last_block_layout_outcome
                .physical_border_box_inline_span
                .points();
            self.content_left = (root_before_principal_track_start
                .expect("the vertical principal-track start was captured")
                + advance)
                .min(self.content_right);
        }
        if consuming_root_canvas {
            self.finish_root_inline_canvas_continuation();
        }
        self.root_pseudo_block_projection = previous_root_pseudo_block_projection;
        self.element_side_effect_suppression_depth -= 1;
        self.end_counter_scope(counter_scope);
    }

    /// Resolves a propagated body's completed document canvas immediately
    /// before the next source-ordered root inline sequence is laid out.
    ///
    /// This is a layout transition, rather than a paint-fragment adjustment:
    /// the source page is already committed by the body traversal when this
    /// method is reached.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn begin_root_inline_canvas_continuation(
        &mut self,
        element: &Element,
    ) -> bool {
        if !element.tag.eq_ignore_ascii_case("html")
            || !self.principal_flow.has_propagated_body()
            || self
                .root_principal_flow_context
                .active_root_inline_canvas
                .is_some()
        {
            return false;
        }
        let axes = WritingModeAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        );
        if !axes.swaps_physical_axes() {
            return false;
        }
        let Some(continuation) = self.root_principal_flow_context.completed_canvas.take() else {
            return false;
        };
        debug_assert_eq!(continuation.source_page.get(), self.pages.len());
        let placement = continuation.resolve_root_inline_placement(
            axes,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        );
        match placement {
            RootInlineCanvasPlacement::RemainingTrack {
                block_track,
                inline_origin,
            } => {
                self.content_left = block_track.left_x();
                self.content_right = block_track.right_x();
                self.cursor_y = inline_origin.points();
            }
            RootInlineCanvasPlacement::NextPage { .. } => {
                // The preceding body has completed its page-owned canvas
                // before this root sequence begins. Mark that normal-flow
                // occupancy so page finalization cannot coalesce the source
                // page away, then establish the destination inline origin
                // before line construction starts.
                self.mark_current_page_flow_content();
                self.push_page();
                self.cursor_y = match inline_start_side(
                    self.principal_flow.writing_mode,
                    self.principal_flow.used_direction(),
                ) {
                    PhysicalSide::Top => self.page_top(),
                    // A bottom-origin principal flow reaches the following
                    // page at the body canvas's inline end. Its inset lies
                    // beyond the new fragmentainer's physical bottom, so the
                    // next root inline sequence is clipped there by ordinary
                    // line layout rather than replaying a translated paint
                    // fragment from the source page.
                    PhysicalSide::Bottom => {
                        self.page_bottom() - continuation.inline_end_inset.points()
                    }
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("a vertical principal flow has a vertical inline axis")
                    }
                };
            }
        }
        self.root_principal_flow_context.active_root_inline_canvas = Some(continuation);
        true
    }

    /// Completes the root inline sequence that consumed the propagated body
    /// continuation. The state remains live through line layout so nested
    /// paint and pagination paths cannot observe a partially consumed canvas.
    pub(in crate::layout) fn finish_root_inline_canvas_continuation(&mut self) {
        debug_assert!(
            self.root_principal_flow_context
                .active_root_inline_canvas
                .is_some()
        );
        self.root_principal_flow_context.active_root_inline_canvas = None;
    }

    pub(in crate::layout) fn layout_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_element_with_child_boxes(element, style, stylesheets, None);
    }

    pub(in crate::layout) fn layout_element_with_child_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_element_with_child_boxes_and_run_ins(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_and_run_ins(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            None,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_and_table_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_run_ins_and_table_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            PrincipalBoxPaintMode::RootPaints,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        capture_principal_effect_context: bool,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        // Most formatting contexts dispatch principal children directly to
        // this common boundary instead of through `layout_element_box`.
        // Consume any non-painting GCPM source event at that same source-order
        // point before a following child's break/page selection is applied.
        self.capture_suppressed_named_strings_before(element.id);
        self.push_page_value_scope(style);
        let page_name_scope = self.enter_page_name_scope(element, style, child_boxes);
        // The common element-dispatch boundary is also used while a
        // multicolumn container lays out its temporary column fragmentainers.
        // Break selection must therefore use the active fragmentation context
        // rather than assuming the outer paged-media page:
        // <https://www.w3.org/TR/css-break-3/#break-types>.
        let fragmentainer_kind = self.active_fragmentainer_kind();
        if self.should_prebreak_avoid_inside(
            element,
            style,
            stylesheets,
            child_boxes,
            fragmentainer_kind,
        ) {
            // Prebreaking before an avoid-kept subtree is a real box
            // fragmentation boundary. Preserve the same destination-local
            // containing-block geometry as the later speculative retry path;
            // raw `push_page` offsets otherwise retain the prior fragment's
            // root/body canvas translation for tables and other nested BFCs.
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
            let continuation = (fragmentainer_kind == FragmentainerKind::Page)
                .then(|| self.block_page_break_continuation_context());
            let source_page_count = self.pages.len();
            self.push_page_if_nonempty();
            if self.pages.len() != source_page_count
                && let Some(continuation) = continuation
            {
                self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
            }
        }
        let mut layout_style;
        let box_break_context = FragmentBreakContext::for_standalone_box(style);
        let style = if !style.display.is_none()
            && let Some(forced_break_before) =
                box_break_context.forced_break_before_in(fragmentainer_kind)
        {
            // CSS Fragmentation places forced `break-before` before the
            // generated box. Counters, named strings, and running elements must
            // therefore observe the post-break page assignment rather than the
            // previous fragmentainer:
            // https://www.w3.org/TR/css-break-3/#break-between
            self.apply_forced_break_in(fragmentainer_kind, forced_break_before);
            layout_style = style.clone();
            layout_style.break_before = PageBreak::Auto;
            &layout_style
        } else {
            style
        };
        let counter_scope =
            (!style.display.is_none()).then(|| self.begin_counter_scope(element, style));
        let source_page_index = self.pages.len();
        let source_paint_checkpoint = self.current_page.paint_checkpoint();
        let source_starts_page_fragment = !self.current_page_has_content();
        let source_content_left = self.content_left;
        let source_cursor_y = self.cursor_y;
        if !style.display.is_none() {
            let named_assignment_ids = self.capture_named_strings(element, style);
            if self.capture_running_element(element, style) {
                // `position: running()` removes the flex item before normal
                // element dispatch, so it must consume the replay item's
                // one-shot percentage basis here instead of leaving it armed
                // for the next sibling.
                // <https://www.w3.org/TR/css-gcpm-3/#running-elements>
                let _ = self.take_replayed_flex_item_percentage_height_basis();
                let placement = AssignmentPlacement {
                    page_index: source_page_index,
                    starts_page_fragment: source_starts_page_fragment,
                    border_box: Some(PaintClip::from_paint_rect(paint_space_rect(
                        source_content_left,
                        source_cursor_y,
                        0.0,
                        0.0,
                    ))),
                };
                self.update_named_assignment_placements(&named_assignment_ids, placement);
                if let Some(counter_scope) = counter_scope {
                    self.end_counter_scope(counter_scope);
                }
                self.capture_suppressed_named_strings_after(element.id);
                self.pop_page_value_scope();
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            if self.should_try_avoid_break_inside(style, fragmentainer_kind) {
                self.layout_avoiding_break_inside(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    child_boxes,
                    table_fragment,
                    principal_box_paint_mode,
                );
                let placement = self.final_source_assignment_placement(
                    style,
                    source_page_index,
                    source_paint_checkpoint,
                    source_starts_page_fragment,
                    source_content_left,
                    source_cursor_y,
                );
                self.update_named_assignment_placements(&named_assignment_ids, placement);
                if let Some(counter_scope) = counter_scope {
                    self.end_counter_scope(counter_scope);
                }
                self.capture_suppressed_named_strings_after(element.id);
                self.pop_page_value_scope();
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            self.layout_element_inner_with_principal_effect_context(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
                capture_principal_effect_context,
                principal_box_paint_mode,
            );
            let placement = self.final_source_assignment_placement(
                style,
                source_page_index,
                source_paint_checkpoint,
                source_starts_page_fragment,
                source_content_left,
                source_cursor_y,
            );
            self.update_named_assignment_placements(&named_assignment_ids, placement);
            if let Some(counter_scope) = counter_scope {
                self.end_counter_scope(counter_scope);
            }
            self.capture_suppressed_named_strings_after(element.id);
            self.pop_page_value_scope();
            self.exit_page_name_scope(page_name_scope);
            return;
        }
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            capture_principal_effect_context,
            principal_box_paint_mode,
        );
        if let Some(counter_scope) = counter_scope {
            self.end_counter_scope(counter_scope);
        }
        self.capture_suppressed_named_strings_after(element.id);
        self.pop_page_value_scope();
        self.exit_page_name_scope(page_name_scope);
    }

    pub(in crate::layout) fn enter_page_name_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 || self.page_name_element_scope_suppression > 0 {
            return None;
        }
        if style.display.is_none()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || style.float != Float::None
            || style.position.is_running()
        {
            return None;
        }
        let page_value_sources = page_value_sources_from_element_style_and_children(
            element,
            style,
            child_boxes.unwrap_or_default(),
        );
        let start_page_name = match &page_value_sources.start {
            PageBoundaryValue::Named(name) => Some(name.as_str()),
            PageBoundaryValue::Inapplicable
            | PageBoundaryValue::Inherited
            | PageBoundaryValue::Auto => None,
        };
        let end_page_name = match page_value_sources.end {
            PageBoundaryValue::Named(name) => Some(name),
            PageBoundaryValue::Inapplicable
            | PageBoundaryValue::Inherited
            | PageBoundaryValue::Auto => None,
        };
        if !style.page.is_specified() && start_page_name.is_none() && end_page_name.is_none() {
            return None;
        }
        // Element scopes establish lexical `page` used-value resolution, but
        // do not themselves materialize a page. The parent formatting
        // context owns the class-A boundary and compares this box's
        // propagated start value with its preceding sibling's end value.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        Some(PageNameScope::Element)
    }

    /// Switches named page groups at a class A sibling page-break boundary.
    ///
    /// CSS Paged Media defines `page` transitions at possible page-break
    /// points between block-level siblings, using the previous sibling's end
    /// page value and the next sibling's start page value:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
    pub(in crate::layout) fn switch_page_name_at_class_a_boundary(
        &mut self,
        page_name: Option<&str>,
    ) {
        if self.page_name_scope_suppression > 0 || self.fragmentation_suppression_depth > 0 {
            return;
        }
        // A class-A page boundary belongs to the active principal
        // fragmentation flow. An orthogonal nested block progresses along a
        // different physical axis and cannot materialize a page transition by
        // itself; its parent fragmentainer owns any eventual page break.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        if self.containing_block_writing_mode != self.principal_flow.writing_mode {
            return;
        }
        // A class-A boundary belongs to the participating boxes in that
        // principal fragmentation flow, rather than to a physical-Y cursor.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if self.principal_flow.writing_mode != WritingMode::HorizontalTb {
            // The root start value chooses the first page type but does not
            // itself cross a fragmentainer boundary.  Moving the vertical
            // block cursor here made the first named child disappear before
            // it could contribute any content.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            if !self.current_page_has_named_page_flow_content {
                self.enter_page_name_scope_for_value(page_name);
                return;
            }
            // The page fragmentainer remains physically top-to-bottom even
            // when the principal block axis is horizontal. Selecting a named
            // page therefore updates the active fragment's type without
            // manufacturing an unrelated horizontal page strip; subsequent
            // vertical-flow placement remains owned by that fragmentainer.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let Some(page_name) = page_name else {
                return;
            };
            self.current_page_name = Some(page_name.to_string());
            return;
        }
        // The structural page value on the preceding side may differ from
        // the succeeding one even when both surrounding lexical scopes
        // resolve to the same materialized page name.  For example, a first
        // child can end the parent's `a` group with `b`; the following
        // inherited child must start a *new* `a` page.  Comparing only the
        // destination with `current_page_name` loses that return boundary.
        // <https://drafts.csswg.org/css-page-3/#using-named-pages>
        if self.current_page_name.as_deref() == page_name
            && self.current_page_has_named_page_flow_content
        {
            self.push_page_for_page_name(page_name);
            return;
        }
        self.enter_page_name_scope_for_value(page_name);
    }

    /// Enters a page-name scope for inline-level content.
    ///
    /// CSS Paged Media applies the `page` property to boxes, including
    /// inline-level boxes. When a later inline box specifies a different page,
    /// the current line fragment must end and following content must be laid
    /// out in that page group:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn enter_inline_page_name_scope(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 {
            return None;
        }
        let previous = self.current_page_name.clone();
        self.enter_page_name_scope_for_value(page_name);
        Some(PageNameScope::Inline {
            previous_page_name: previous,
        })
    }
}
