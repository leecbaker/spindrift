use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn new(config: LayoutBuilderConfig<'a>) -> Self {
        let mut page_margin_boxes = HashMap::new();
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
            for (name, declarations) in &stylesheet.page_margin_boxes {
                page_margin_boxes
                    .entry(name.clone())
                    .or_insert_with(Declarations::new)
                    .extend(declarations.clone());
            }
        }
        let page_context = PageContext::from_options(config.options);
        let mut builder = Self {
            options: config.options,
            stylesheets: config.stylesheets,
            base_url: config.base_url,
            root_url: config.root_url,
            resource_cache: config.resource_cache,
            pages: Vec::new(),
            page_names: Vec::new(),
            page_blanks: Vec::new(),
            page_name_scope_suppression: 0,
            page_name_element_scope_suppression: 0,
            page_named_strings: Vec::new(),
            page_running_elements: Vec::new(),
            page_anchors: HashMap::new(),
            page_anchor_text: HashMap::new(),
            document_canvas_background: None,
            root_canvas_background_defined: false,
            current_page: page_for_context(page_context),
            current_page_has_flow_content: false,
            last_block_layout_outcome: BlockLayoutOutcome::default(),
            current_page_name: None,
            current_page_context: page_context,
            cursor_y: page_context.top(),
            content_left: page_context.left(),
            content_right: page_context.right(),
            content_logical_inline_size_stack: Vec::new(),
            inline_static_position: None,
            text_box_line_trim_stack: Vec::new(),
            last_in_flow_line_baseline_y: None,
            block_static_position_y_offset: None,
            absolute_static_position: None,
            escaped_atom_positioning_depth: 0,
            escaped_atom_containing_block: None,
            containing_block_direction: Direction::Ltr,
            containing_block_writing_mode: WritingMode::HorizontalTb,
            fragment_top_offsets: Vec::new(),
            child_available_space_stack: Vec::new(),
            definite_block_size_stack: Vec::new(),
            truncate_page_start_margins: false,
            avoid_inside_retry_depth: 0,
            out_of_flow_prebreak_suppression_depth: 0,
            element_side_effect_suppression_depth: 0,
            containing_blocks: Vec::new(),
            list_stack: Vec::new(),
            counter_set: CounterSet::new(),
            quote_depth: 0,
            current_page_named_strings: HashMap::new(),
            current_page_running_elements: HashMap::new(),
            next_assignment_id: 0,
            assignment_capture_stack: Vec::new(),
            ancestors: Vec::new(),
            page_counter_initial_values: config.page_counter_initial_values,
            page_rules,
            page_progression_direction: config.page_progression_direction,
            page_declarations,
            page_margin_boxes,
            counter_styles,
            first_page_declarations,
            font_system: Box::new(config.font_system),
            bookmarks: Vec::new(),
            positioned_layers: Vec::new(),
            fixed_layers: Vec::new(),
            next_paint_source_order: 1,
            overflow_clips: Vec::new(),
            next_float_id: 1,
            float_contexts: vec![FloatContext { shapes: Vec::new() }],
            adjoining_float_origin_y: None,
            pending_float_fragments: Vec::new(),
            pending_float_side_effects: Vec::new(),
            applied_clearance_count: 0,
            preserve_scoped_paint_public_order: false,
            defer_next_block_decoration_promotion: false,
        };
        builder.rebuild_empty_current_page_context();
        builder
    }

    pub(in crate::layout) fn layout_page_box(
        &mut self,
        page_box: &box_tree::PageBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        for child in &page_box.children {
            self.layout_formatting_box(child, stylesheets);
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
    ) {
        for child in &mut page_box.children {
            self.resolve_font_metric_lengths_in_box(child);
        }
    }

    pub(in crate::layout) fn resolve_style_viewport_lengths(
        style: &mut ComputedStyle,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        style.resolve_viewport_lengths(viewport_width, viewport_height);
        if let Some(style) = &mut style.marker_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
        if let Some(style) = &mut style.before_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
        if let Some(style) = &mut style.after_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
    }

    pub(in crate::layout) fn style_with_current_viewport_lengths(
        &self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = style.clone();
        self.resolve_style_current_viewport_lengths(&mut style);
        style
    }

    pub(in crate::layout) fn style_with_current_used_lengths(
        &mut self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = self.style_with_current_viewport_lengths(style);
        self.resolve_style_font_metric_lengths(&mut style);
        style
    }

    pub(in crate::layout) fn resolve_style_current_viewport_lengths(
        &self,
        style: &mut ComputedStyle,
    ) {
        Self::resolve_style_viewport_lengths(
            style,
            self.page_area_width(),
            self.page_area_height(),
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
        let ch_advance = self.font_system.ch_advance(style);
        style.resolve_font_metric_lengths(ch_advance);
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
        let ch_advance = self.font_system.ch_advance(style);
        style.resolve_font_metric_lengths_preserving_box_block_sizes(ch_advance);
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
        let parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
        let mut style = style_for_layout_element_with_parent_ch_advance(
            element,
            signature.clone(),
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
        let signature = layout_element_signature(element, signature, parent);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
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
        let parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
        let mut style = css::style_for_element_with_signature_and_parent_ch_advance(
            signature.clone(),
            inline_style,
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
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
                self.layout_anonymous_block(&box_.style, &box_.children, stylesheets, None)
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
            box_tree::BoxSource::GeneratedPseudo(_) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
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
            box_tree::BoxSource::GeneratedPseudo(_) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                );
            }
        }
        self.ancestors.pop();
    }

    pub(in crate::layout) fn layout_generated_pseudo_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let counter_scope = self.begin_pseudo_counter_scope(style);
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
        let page_name_scope = self.enter_page_name_scope(style, child_boxes);
        if self.should_prebreak_avoid_inside(element, style, stylesheets, child_boxes) {
            self.push_page_if_nonempty();
        }
        let mut layout_style;
        let style = if !style.display.is_none() && style.break_before.is_forced() {
            // CSS Fragmentation places forced `break-before` before the
            // generated box. Counters, named strings, and running elements must
            // therefore observe the post-break page assignment rather than the
            // previous fragmentainer:
            // https://www.w3.org/TR/css-break-3/#break-between
            self.apply_forced_break(style.break_before);
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
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            if self.should_try_avoid_break_inside(style) {
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
                self.materialize_empty_named_page_scope(style, page_name_scope.as_ref());
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            self.layout_element_inner(
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
            self.materialize_empty_named_page_scope(style, page_name_scope.as_ref());
            self.exit_page_name_scope(page_name_scope);
            return;
        }
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        if let Some(counter_scope) = counter_scope {
            self.end_counter_scope(counter_scope);
        }
        self.materialize_empty_named_page_scope(style, page_name_scope.as_ref());
        self.exit_page_name_scope(page_name_scope);
    }

    /// Materializes an empty page box for an entered named-page scope.
    ///
    /// CSS Paged Media forms named page groups from elements with a `page`
    /// value at class A break boundaries. WPT `page-name-display-none-child`
    /// expects an otherwise empty page-owning block to still occupy its named
    /// page before the next page group starts:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn materialize_empty_named_page_scope(
        &mut self,
        style: &ComputedStyle,
        scope: Option<&PageNameScope>,
    ) {
        let Some(scope) = scope else {
            return;
        };
        if style.page_name_specified && !self.scope_produced_current_page_content(scope) {
            self.materialize_current_empty_page();
        }
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
            || style.running_element_name.is_some()
        {
            return None;
        }
        if !style.page_name_specified
            && child_boxes
                .map(formatting_boxes_page_values)
                .is_none_or(|(start, end)| start.is_none() && end.is_none())
        {
            return None;
        }
        let (start_page_name, end_page_name) =
            page_values_from_style_and_children(style, child_boxes.unwrap_or_default());
        self.enter_page_name_scope_for_value(start_page_name.as_deref());
        Some(self.page_name_scope_checkpoint(end_page_name))
    }

    pub(in crate::layout) fn page_name_scope_checkpoint(
        &self,
        end_page_name: Option<String>,
    ) -> PageNameScope {
        PageNameScope {
            end_page_name,
            start_page_count: self.pages.len(),
            start_page_has_content: self.current_page_has_content(),
        }
    }

    pub(in crate::layout) fn scope_produced_current_page_content(
        &self,
        scope: &PageNameScope,
    ) -> bool {
        self.pages.len() != scope.start_page_count
            || self.current_page_has_content() != scope.start_page_has_content
    }

    pub(in crate::layout) fn materialize_current_empty_page(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let next_context = self.resolved_page_context(self.pages.len() + 2, false);
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.current_page_has_flow_content = false;
        self.apply_page_context(
            next_context,
            FragmentOffsets {
                left: 0.0,
                right: 0.0,
                top: 0.0,
            },
        );
        self.truncate_page_start_margins = true;
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
        Some(self.page_name_scope_checkpoint(previous))
    }
}
