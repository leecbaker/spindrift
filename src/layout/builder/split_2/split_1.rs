use super::*;
use crate::units::LayoutSize;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn new(config: LayoutBuilderConfig<'a>) -> Self {
        let mut counter_styles = HashMap::new();
        let mut page_rules = Vec::new();
        let mut page_declarations = Declarations::new();
        let mut first_page_declarations = Declarations::new();
        for stylesheet in config.stylesheets {
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
                counter_styles.insert(counter_style.name.clone(), counter_style.clone());
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
            page_anchors: HashMap::new(),
            page_anchor_text: HashMap::new(),
            document_canvas_background: None,
            document_canvas_overflow: DocumentCanvasOverflowContext::default(),
            document_canvas_fragment_insets: Vec::new(),
            document_root_generates_box: true,
            current_page: page_for_context(page_context),
            current_page_has_flow_content: false,
            current_page_has_named_page_flow_content: false,
            last_block_layout_outcome: BlockLayoutOutcome::default(),
            current_page_name: None,
            current_page_context: page_context,
            initial_viewport_context: page_context,
            page_descriptor_viewport_size: page_context.size,
            fragmentainer_override: None,
            fragmentation_suppression_depth: 0,
            multicol_spanner_fragmentation_depth: 0,
            multicol_spanner_speculation_depth: 0,
            multicol_balance_probe_depth: 0,
            forced_break_containment_scopes: Vec::new(),
            cursor_y: page_context.top(),
            content_left: page_context.left(),
            content_right: page_context.right(),
            content_logical_inline_size_stack: Vec::new(),
            multicol_column_containing_blocks: Vec::new(),
            intrinsic_inline_percentage_basis_stack: Vec::new(),
            inline_static_position: None,
            text_box_line_trim_stack: Vec::new(),
            clamp_line_slot_captures: Vec::new(),
            positioned_inline_layout_suppression_depth: 0,
            last_in_flow_line_baseline_y: None,
            block_static_position_y_offset: None,
            absolute_static_position: None,
            grid_positioning_scopes: Vec::new(),
            escaped_atom_positioning_depth: 0,
            escaped_atom_containing_block: None,
            containing_block_direction: Direction::Ltr,
            containing_block_writing_mode: WritingMode::HorizontalTb,
            initial_containing_block_writing_mode: WritingMode::HorizontalTb,
            fragment_top_offsets: Vec::new(),
            child_available_space_stack: Vec::new(),
            normal_flow_relative_containing_blocks: Vec::new(),
            definite_block_size_stack: Vec::new(),
            replayed_flex_item_percentage_height_bases: Vec::new(),
            table_wrapper_block_size_overrides: Vec::new(),
            truncate_page_start_margins: false,
            avoid_inside_retry_depth: 0,
            out_of_flow_prebreak_suppression_depth: 0,
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
            font_system: Box::new(config.font_system),
            bookmarks: Vec::new(),
            positioned_layers: Vec::new(),
            fixed_layers: Vec::new(),
            pending_positioned_page_span_target: None,
            next_paint_source_order: 1,
            overflow_clips: Vec::new(),
            active_scroll_snap_scopes: Vec::new(),
            next_float_id: 1,
            float_contexts: vec![FloatContext { shapes: Vec::new() }],
            adjoining_float_origin_y: None,
            pending_paint_fragments: Vec::new(),
            pending_page_side_effects: Vec::new(),
            applied_clearance_count: 0,
            preserve_scoped_paint_public_order: false,
            defer_next_block_decoration_promotion: false,
        };
        builder.rebuild_empty_current_page_context();
        builder.initial_viewport_context = builder.current_page_context;
        builder
    }

    pub(in crate::layout) fn layout_page_box(
        &mut self,
        page_box: &box_tree::PageBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        self.prepare_counter_plan(&page_box.counter_events);
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
        self.document_canvas_overflow = DocumentCanvasOverflowContext::from_page_box(page_box);
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
                let (root_page_start, root_page_start_specified) =
                    formatting_box_page_value_sources(child).0;
                if root_page_start_specified {
                    self.enter_page_name_scope_for_value(root_page_start.as_deref());
                }
                // Select root/body background propagation before descendant
                // layout. When the root paints the canvas, the body remains
                // an ordinary box; discovering the root only after children
                // would otherwise suppress the body's own background.
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
                if self.iframe_viewport.is_none()
                    && let Some((_, _, style, _)) = child.element_parts()
                    && style.visibility == Visibility::Visible
                    && (style.background_color.is_some_and(Color::is_visible)
                        || style
                            .background_layers
                            .iter()
                            .any(|layer| layer.image.is_some()))
                {
                    self.document_canvas_background = Some(DocumentCanvasBackground {
                        style: canvas_background_style(style),
                        root_background_defined: true,
                        root_positioning_area: None,
                    });
                }
                self.definite_block_size_stack
                    .push(PercentageBasis::definite_from(
                        content_box_pt(self.page_area_height()),
                        BlockSizeBasisSource::InitialContainingBlock,
                    ));
            }
            self.layout_formatting_box(child, stylesheets);
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
        let mut root_font_metrics = None;
        for child in &mut page_box.children {
            self.resolve_deferred_font_metrics_in_box(
                child,
                document_root_parent.font_size,
                parent_ch_advance,
                &mut root_font_metrics,
            );
        }
    }

    fn resolve_deferred_font_metrics_in_style(
        &mut self,
        style: &mut ComputedStyle,
        parent_font_size: f32,
        parent_ch_advance: LayoutLength,
        root_font_metrics: &mut Option<css::RootFontMetricLengthBasis>,
    ) -> (f32, LayoutLength) {
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        style.resolve_deferred_font_size_with_viewport(
            css::FontRelativeLengthBasis::new(layout_pt(parent_font_size), parent_ch_advance),
            LayoutSize::new(
                self.current_page_context.area_width(),
                self.current_page_context.area_height(),
            ),
        );
        style
            .line_height_value
            .resolve_em_relative_lengths(layout_pt(style.font_size));
        let (line_height, multiplier, is_normal) =
            style.line_height_value.clone().projected(style.font_size);
        style.line_height = line_height;
        style.line_height_multiplier = multiplier;
        style.line_height_is_normal = is_normal;
        style.root_font_size = root_font_metrics
            .as_ref()
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
            style.requires_ch_advance() || pseudo_requires_parent_ch,
        );
        // The selected `ch` metric is the existing horizontal basis except
        // where vertical upright text supplies a distinct vertical advance.
        // Keep the fallback selected for the style intact: the font fallback
        // is part of CSS's `ch` definition when the selected face cannot
        // provide the relevant advance.
        let horizontal_ch_advance = ch_advance;
        let vertical_ch_advance = matches!(
            style.text_layout_policy(),
            css::TextLayoutPolicy::Vertical(css::TextOrientation::Upright)
        )
        .then_some(ch_advance)
        .unwrap_or(horizontal_ch_advance);
        style.resolve_font_metric_lengths_with_box_axes(
            ch_advance,
            horizontal_ch_advance,
            vertical_ch_advance,
        );
        // A selected-font metric lookup interns that font in the document.
        // Do not perform one for an otherwise metric-free style: an empty
        // block with the initial `normal` line-height must not retain a font.
        // The existing metric-dependency traversal covers every `ch`-based
        // term, and selected-font metric expressions share that used-value
        // resolution path.
        let requires_selected_font_metrics = style.requires_selected_font_metrics();
        let ic_advance = if requires_selected_font_metrics {
            self.font_system.ic_advance_for_style(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        };
        let horizontal_ic_advance = if requires_selected_font_metrics {
            self.font_system.horizontal_ic_advance_for_style(style)
        } else {
            ic_advance
        };
        let vertical_ic_advance = matches!(
            style.text_layout_policy(),
            css::TextLayoutPolicy::Vertical(_)
        )
        .then_some(ic_advance)
        .unwrap_or(horizontal_ic_advance);
        style.resolve_ic_relative_lengths_with_box_axes(
            ic_advance,
            horizontal_ic_advance,
            vertical_ic_advance,
        );
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        style.resolve_ex_relative_lengths(x_height);
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_cap_relative_lengths(cap_height);
        style.resolve_line_height_relative_lengths();
        let root_font_metrics =
            *root_font_metrics.get_or_insert_with(|| css::RootFontMetricLengthBasis {
                font_size: layout_pt(style.font_size),
                ch_advance,
                x_height: layout_pt(x_height),
                cap_height: layout_pt(cap_height),
                ic_advance,
                line_height: layout_pt(style.line_height),
            });
        style.root_font_size = root_font_metrics.font_size.points();
        style.resolve_root_font_metric_lengths(root_font_metrics);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        let font_size = style.font_size;
        if let Some(style) = &mut style.marker_style {
            self.resolve_deferred_font_metrics_in_style(
                style,
                font_size,
                ch_advance,
                &mut Some(root_font_metrics),
            );
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_deferred_font_metrics_in_style(
                style,
                font_size,
                ch_advance,
                &mut Some(root_font_metrics),
            );
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_deferred_font_metrics_in_style(
                style,
                font_size,
                ch_advance,
                &mut Some(root_font_metrics),
            );
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_deferred_font_metrics_in_style(
                style,
                font_size,
                ch_advance,
                &mut Some(root_font_metrics),
            );
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_deferred_font_metrics_in_style(
                style,
                font_size,
                ch_advance,
                &mut Some(root_font_metrics),
            );
        }
        // All font-, root-font-, viewport-, and selected-font-metric terms
        // above are still in ordinary CSS units. Apply zoom only once they
        // have their concrete used-length values, so inherited text styles
        // and percentage bases retain their CSS semantics.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        (font_size, ch_advance)
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
        let style = match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => &box_.style,
            box_tree::MutableFormattingBox::Inline(box_) => &box_.style,
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => &box_.style,
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => &box_.style,
            box_tree::MutableFormattingBox::AtomicInline(box_) => &box_.style,
            box_tree::MutableFormattingBox::Text(box_) => &box_.style,
            box_tree::MutableFormattingBox::Table(box_) => &box_.style,
            box_tree::MutableFormattingBox::Flex(box_) => &box_.style,
            box_tree::MutableFormattingBox::Replaced(box_) => &box_.style,
        };
        style
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

    fn resolve_deferred_font_metrics_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
        parent_font_size: f32,
        parent_ch_advance: LayoutLength,
        root_font_metrics: &mut Option<css::RootFontMetricLengthBasis>,
    ) {
        let mut recurse = |builder: &mut Self,
                           children: &mut Vec<box_tree::MutableFormattingBox<'_>>,
                           style: &mut ComputedStyle| {
            let (font_size, _ch_advance) = builder.resolve_deferred_font_metrics_in_style(
                style,
                parent_font_size,
                parent_ch_advance,
                root_font_metrics,
            );
            let ch_advance = builder.ch_advance_for_style(
                style,
                builder.children_require_parent_ch_advance(children, font_size),
            );
            for child in children {
                builder.resolve_deferred_font_metrics_in_box(
                    child,
                    font_size,
                    ch_advance,
                    root_font_metrics,
                );
            }
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                let (font_size, _ch_advance) = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_font_size,
                    parent_ch_advance,
                    root_font_metrics,
                );
                let child_requires_parent_ch = self
                    .children_require_parent_ch_advance(&box_.run_in_children, font_size)
                    || self.children_require_parent_ch_advance(&box_.children, font_size);
                let ch_advance = self.ch_advance_for_style(&box_.style, child_requires_parent_ch);
                for child in &mut box_.run_in_children {
                    self.resolve_deferred_font_metrics_in_box(
                        child,
                        font_size,
                        ch_advance,
                        root_font_metrics,
                    );
                }
                for child in &mut box_.children {
                    self.resolve_deferred_font_metrics_in_box(
                        child,
                        font_size,
                        ch_advance,
                        root_font_metrics,
                    );
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                let (font_size, _ch_advance) = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_font_size,
                    parent_ch_advance,
                    root_font_metrics,
                );
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.children, font_size);
                let ch_advance = self.ch_advance_for_style(&box_.style, child_requires_parent_ch);
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_deferred_font_metrics_in_table_fragment(
                        fragment,
                        font_size,
                        ch_advance,
                        root_font_metrics,
                    );
                }
                for child in &mut box_.children {
                    self.resolve_deferred_font_metrics_in_box(
                        child,
                        font_size,
                        ch_advance,
                        root_font_metrics,
                    );
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_font_size,
                    parent_ch_advance,
                    root_font_metrics,
                );
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                let (font_size, _ch_advance) = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_font_size,
                    parent_ch_advance,
                    root_font_metrics,
                );
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.children, font_size);
                let ch_advance = self.ch_advance_for_style(&box_.style, child_requires_parent_ch);
                self.resolve_deferred_font_metrics_in_table_fragment(
                    &mut box_.fragment,
                    font_size,
                    ch_advance,
                    root_font_metrics,
                );
                for child in &mut box_.children {
                    self.resolve_deferred_font_metrics_in_box(
                        child,
                        font_size,
                        ch_advance,
                        root_font_metrics,
                    );
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
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
        parent_font_size: f32,
        parent_ch_advance: LayoutLength,
        root_font_metrics: &mut Option<css::RootFontMetricLengthBasis>,
    ) {
        for row in &mut fragment.rows {
            let mut row_parent_font_size = parent_font_size;
            let mut row_parent_ch_advance = parent_ch_advance;
            for group in &mut row.row_groups {
                if let Some(style) = &mut group.style {
                    (row_parent_font_size, row_parent_ch_advance) = self
                        .resolve_deferred_font_metrics_in_style(
                            style,
                            row_parent_font_size,
                            row_parent_ch_advance,
                            root_font_metrics,
                        );
                }
            }
            let (row_font_size, row_ch_advance) = row
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(
                        style,
                        row_parent_font_size,
                        row_parent_ch_advance,
                        root_font_metrics,
                    )
                })
                .unwrap_or((row_parent_font_size, row_parent_ch_advance));
            for cell in &mut row.cells {
                let (cell_font_size, cell_ch_advance) = cell
                    .style
                    .as_deref_mut()
                    .map(|style| {
                        self.resolve_deferred_font_metrics_in_style(
                            style,
                            row_font_size,
                            row_ch_advance,
                            root_font_metrics,
                        )
                    })
                    .unwrap_or((row_font_size, row_ch_advance));
                for child in &mut cell.children {
                    self.resolve_deferred_font_metrics_in_box(
                        child,
                        cell_font_size,
                        cell_ch_advance,
                        root_font_metrics,
                    );
                }
            }
        }
        for caption in &mut fragment.captions {
            let (font_size, ch_advance) = caption
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(
                        style,
                        parent_font_size,
                        parent_ch_advance,
                        root_font_metrics,
                    )
                })
                .unwrap_or((parent_font_size, parent_ch_advance));
            for child in &mut caption.children {
                self.resolve_deferred_font_metrics_in_box(
                    child,
                    font_size,
                    ch_advance,
                    root_font_metrics,
                );
            }
        }
        for column in &mut fragment.columns {
            let (group_font_size, group_ch_advance) = column
                .group
                .as_mut()
                .and_then(|group| group.style.as_deref_mut())
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(
                        style,
                        parent_font_size,
                        parent_ch_advance,
                        root_font_metrics,
                    )
                })
                .unwrap_or((parent_font_size, parent_ch_advance));
            if let Some(style) = &mut column.style {
                self.resolve_deferred_font_metrics_in_style(
                    style,
                    group_font_size,
                    group_ch_advance,
                    root_font_metrics,
                );
            }
        }
    }

    pub(in crate::layout) fn resolve_style_viewport_lengths(
        style: &mut ComputedStyle,
        viewport: LayoutSize,
    ) {
        style.resolve_viewport_lengths_for_viewport(viewport);
        if let Some(style) = &mut style.marker_style {
            Self::resolve_style_viewport_lengths(style, viewport);
        }
        if let Some(style) = &mut style.before_style {
            Self::resolve_style_viewport_lengths(style, viewport);
        }
        if let Some(style) = &mut style.after_style {
            Self::resolve_style_viewport_lengths(style, viewport);
        }
    }

    pub(in crate::layout) fn style_with_current_viewport_lengths(
        &self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = style.clone();
        self.resolve_style_current_viewport_lengths(&mut style);
        style.apply_effective_zoom();
        style
    }

    pub(in crate::layout) fn style_with_current_used_lengths(
        &mut self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = style.clone();
        self.resolve_style_current_viewport_lengths(&mut style);
        self.resolve_style_font_metric_lengths(&mut style);
        style.apply_effective_zoom();
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
        // In paged media the page area is the viewport used to resolve
        // viewport-relative lengths. Resolve at layout time so a box that is
        // pre-broken into a differently sized destination page observes that
        // destination page's viewport rather than the previous fragment.
        // <https://www.w3.org/TR/css-page-3/#page-model>
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        Self::resolve_style_viewport_lengths(
            style,
            LayoutSize::new(
                self.current_page_context.area_width(),
                self.current_page_context.area_height(),
            ),
        );
    }

    pub(in crate::layout) fn resolve_font_metric_lengths_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
    ) {
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.run_in_children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
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
                self.resolve_style_font_metric_lengths(&mut box_.style);
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_font_metric_lengths_in_table_fragment(fragment);
                }
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                self.resolve_font_metric_lengths_in_table_fragment(&mut box_.fragment);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
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
        stylesheets: &[Stylesheet],
        parent_style: &ComputedStyle,
        ancestors: &[ElementSignature],
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
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
        stylesheets: &[Stylesheet],
        parent_style: &ComputedStyle,
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
        let ancestors = self.ancestors.clone();
        self.build_frozen_child_boxes_with_font_metrics(
            element,
            stylesheets,
            parent_style,
            &ancestors,
        )
    }

    pub(in crate::layout) fn resolve_style_font_metric_lengths(
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
        style.resolve_font_metric_lengths(ch_advance);
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
        style.resolve_ic_relative_lengths(ic_advance);
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        style.resolve_ex_relative_lengths(x_height);
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_cap_relative_lengths(cap_height);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
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
    }

    pub(in crate::layout) fn style_for_layout_element_with_parent_font_metrics(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let mut style = style_for_layout_element_with_parent_ch_advance(
            element,
            signature.clone(),
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
            style = style_for_layout_element_with_parent_ch_advance(
                element,
                signature.clone(),
                stylesheets,
                parent,
                ancestors,
                parent_ch_advance,
            );
        }
        let signature = layout_element_signature(element, signature, parent);
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
        style
    }

    pub(in crate::layout) fn style_for_signature_with_parent_font_metrics(
        &mut self,
        signature: ElementSignature,
        inline_style: Option<&str>,
        stylesheets: &[Stylesheet],
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
        style
    }

    pub(in crate::layout) fn layout_formatting_box(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        match formatting_box {
            box_tree::FormattingBox::Block(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.source,
                &box_.run_in_children,
                &box_.children,
            ),
            box_tree::FormattingBox::Inline(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.source,
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                self.layout_anonymous_block(&box_.style, &box_.children, stylesheets, None);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                self.layout_inline_split_block_context(box_, stylesheets)
            }
            box_tree::FormattingBox::AtomicInline(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.source,
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Table(box_) => {
                self.layout_table_box(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    box_.signature.clone(),
                    &box_.source,
                    &box_.children,
                    &box_.fragment,
                );
            }
            box_tree::FormattingBox::Flex(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.source,
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Replaced(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.source,
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Text(box_) => {
                let text = normalized_text_for_style(&box_.text, &box_.style);
                if !text.is_empty() {
                    self.layout_text_block(&text, &box_.style, 0.0, 0.0, None);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.layout_element_with_child_boxes_and_run_ins(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    Some(children),
                );
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    match pseudo.kind {
                        box_tree::GeneratedPseudoKind::Before => {
                            box_tree::CounterEventSource::Before
                        }
                        box_tree::GeneratedPseudoKind::After => box_tree::CounterEventSource::After,
                    },
                    stylesheets,
                    run_in_children,
                    Some(children),
                    None,
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
        stylesheets: &[Stylesheet],
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.layout_element_with_child_boxes_run_ins_and_table_fragment(
                    element,
                    style,
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                );
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    match pseudo.kind {
                        box_tree::GeneratedPseudoKind::Before => {
                            box_tree::CounterEventSource::Before
                        }
                        box_tree::GeneratedPseudoKind::After => box_tree::CounterEventSource::After,
                    },
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
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
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let counter_scope = self.begin_pseudo_counter_scope(element, source, style);
        self.element_side_effect_suppression_depth += 1;
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        self.element_side_effect_suppression_depth -= 1;
        self.end_counter_scope(counter_scope);
    }

    pub(in crate::layout) fn layout_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) {
        self.layout_element_with_child_boxes(element, style, stylesheets, None);
    }

    pub(in crate::layout) fn layout_element_with_child_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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

    /// Lay out a replayed flex/grid item while its outer item context owns the
    /// principal box's paint effects. Descendant effect contexts are unchanged.
    /// CSS Grid and Flexbox paint items as stacking units after resolving their
    /// independent formatting contexts:
    /// <https://www.w3.org/TR/css-grid-1/#z-order> and
    /// <https://www.w3.org/TR/css-flexbox-1/#painting>.
    pub(in crate::layout) fn layout_element_with_child_boxes_without_principal_effect_context(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            None,
            false,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_and_run_ins(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
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
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        capture_principal_effect_context: bool,
    ) {
        self.push_page_value_scope(style);
        let page_name_scope = self.enter_page_name_scope(style, child_boxes);
        let fragmentainer_kind = FragmentainerKind::Page;
        if self.should_prebreak_avoid_inside(
            element,
            style,
            stylesheets,
            child_boxes,
            fragmentainer_kind,
        ) {
            self.push_page_if_nonempty();
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
        );
        if let Some(counter_scope) = counter_scope {
            self.end_counter_scope(counter_scope);
        }
        self.pop_page_value_scope();
        self.exit_page_name_scope(page_name_scope);
    }

    pub(in crate::layout) fn enter_page_name_scope(
        &mut self,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 || self.page_name_element_scope_suppression > 0 {
            return None;
        }
        if style.display.is_none()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || style.float != Float::None
            || style.running_element_name.is_some()
        {
            return None;
        }
        let page_value_sources =
            page_value_sources_from_style_and_children(style, child_boxes.unwrap_or_default());
        if !style.page_name_specified
            && page_value_sources.0.0.is_none()
            && page_value_sources.1.0.is_none()
        {
            return None;
        }
        self.enter_page_name_scope_for_value(page_value_sources.0.0.as_deref());
        Some(self.page_name_scope_checkpoint(page_value_sources.1.0))
    }

    pub(in crate::layout) fn page_name_scope_checkpoint(
        &self,
        end_page_name: Option<String>,
    ) -> PageNameScope {
        PageNameScope { end_page_name }
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
        if self.page_name_scope_suppression > 0 {
            return;
        }
        if self.enter_page_name_scope_for_value(page_name).is_some() {
            // A class-A boundary itself establishes the destination page
            // group, even when the selected normal-flow box has no used
            // geometry (for example, an empty block containing only
            // `display:none` content). A subsequent distinct sibling must
            // therefore materialize this otherwise empty page group.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            self.current_page_has_named_page_flow_content = true;
        }
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
        Some(self.page_name_scope_checkpoint(previous))
    }
}
