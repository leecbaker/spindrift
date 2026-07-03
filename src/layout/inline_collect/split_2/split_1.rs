use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_visual_offset_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> InlineVisualOffset {
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            return InlineVisualOffset::zero();
        }
        let style = self.style_with_current_used_lengths(style);
        InlineVisualOffset::from_relative_offset(relative_position_offset(
            &style,
            self.current_containing_block(),
        ))
    }

    pub(in crate::layout) fn intrinsic_inline_contribution_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> inline_layout::InlineIntrinsicContribution {
        self.intrinsic_inline_measurement_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
            f32::MAX,
        )
        .contribution
    }

    pub(in crate::layout) fn intrinsic_inline_contribution_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> inline_layout::InlineIntrinsicContribution {
        self.intrinsic_inline_measurement_for_boxes(children, style, stylesheets, f32::MAX)
            .contribution
    }

    /// Measure inline content through durable graph-selected fragments.
    ///
    /// CSS Sizing derives intrinsic inline contributions from CSS Text break
    /// opportunities, and CSS Flexbox consumes the same content as selected
    /// line fragments for hypothetical cross sizes:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>,
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout) fn intrinsic_inline_measurement_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let items = self.intrinsic_inline_items_for_boxes(children, style, stylesheets);
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    pub(in crate::layout) fn intrinsic_inline_items_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Vec<InlineItem> {
        let mut items = Vec::new();
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if child_boxes.is_none() {
            self.push_generated_pseudo_items(
                element,
                style,
                style.before_style.as_deref(),
                None,
                0.0,
                InlineVisualOffset::zero(),
                GeneratedPseudoCounterMode::Rollback,
                &mut items,
            );
        }
        if let Some(child_boxes) = child_boxes {
            if style.content.is_generated() {
                self.push_intrinsic_element_content_items_from_boxes(
                    element,
                    style,
                    child_boxes,
                    stylesheets,
                    None,
                    0.0,
                    InlineVisualOffset::zero(),
                    style.text_decoration,
                    &mut items,
                );
            } else {
                self.collect_intrinsic_inline_box_items(
                    child_boxes,
                    stylesheets,
                    None,
                    IntrinsicInlineCollectionContext {
                        baseline_shift: 0.0,
                        visual_offset: InlineVisualOffset::zero(),
                        block_style: style,
                        propagated_decoration: style.text_decoration,
                    },
                    &mut items,
                );
            }
        } else {
            self.collect_intrinsic_element_content_or_inline_items(
                element,
                style,
                stylesheets,
                None,
                InlinePlacement::zero(),
                &mut items,
            );
        }
        if child_boxes.is_none() {
            self.push_generated_pseudo_items(
                element,
                style,
                style.after_style.as_deref(),
                None,
                0.0,
                InlineVisualOffset::zero(),
                GeneratedPseudoCounterMode::Rollback,
                &mut items,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        items
    }

    pub(in crate::layout) fn intrinsic_inline_items_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Vec<InlineItem> {
        let mut items = Vec::new();
        self.collect_intrinsic_inline_box_items(
            children,
            stylesheets,
            None,
            IntrinsicInlineCollectionContext {
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                block_style: style,
                propagated_decoration: style.text_decoration,
            },
            &mut items,
        );
        items
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_items(
        &mut self,
        items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(block_style);
        self.with_text_box_line_trim_scope(text_box_line_trim, |layout| {
            layout.intrinsic_inline_measurement_for_items_in_trim_scope(
                items,
                block_style,
                available_width,
            )
        })
    }

    fn intrinsic_inline_measurement_for_items_in_trim_scope(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        normalize_inline_whitespace_items(&mut items);
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            stylesheets: &[],
            available_width: available_width.max(1.0),
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        };
        let mut output = inline_layout::InlineIntrinsicMeasurement::default();
        output.sequence.available_width = context.available_width;
        let mut paragraph = Vec::new();
        let mut starts_after_forced_break = false;
        for item in items {
            match inline_item_boundary_role(&item) {
                InlineBoundaryRole::ForcedBreak => {
                    self.flush_intrinsic_inline_measurement_paragraph(
                        &mut paragraph,
                        context,
                        true,
                        starts_after_forced_break,
                        &mut output,
                    );
                    starts_after_forced_break = true;
                }
                role if role == InlineBoundaryRole::Float || role.is_page_scope() => {
                    let flushed = self.flush_intrinsic_inline_measurement_paragraph(
                        &mut paragraph,
                        context,
                        false,
                        starts_after_forced_break,
                        &mut output,
                    );
                    if flushed {
                        starts_after_forced_break = false;
                    }
                }
                _ => paragraph.push(item),
            }
        }
        self.flush_intrinsic_inline_measurement_paragraph(
            &mut paragraph,
            context,
            false,
            starts_after_forced_break,
            &mut output,
        );
        let records = std::mem::take(&mut output.sequence.records);
        let (records, fragment_text_box_trim) =
            self.with_text_box_line_trim_applied(records, block_style);
        output.sequence.records = records;
        output.sequence.fragment_text_box_trim = fragment_text_box_trim;
        output
    }

    /// Estimate inline block-size from graph-selected line fragments.
    ///
    /// CSS Flexbox computes a flex item's hypothetical cross size by laying
    /// the item out as an in-flow block, while CSS Inline and CSS Text define
    /// the line boxes and break opportunities used by that block layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout) fn intrinsic_inline_block_metrics_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
    ) -> (f32, usize) {
        let measurement = self.intrinsic_inline_measurement_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
            available_width,
        );
        (measurement.height(), measurement.line_count())
    }

    pub(in crate::layout) fn intrinsic_inline_block_metrics_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> (f32, usize) {
        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_width,
        );
        (measurement.height(), measurement.line_count())
    }

    pub(in crate::layout) fn flush_intrinsic_inline_measurement_paragraph(
        &mut self,
        paragraph: &mut Vec<InlineItem>,
        context: InlineParagraphContext<'_>,
        force_empty_line: bool,
        starts_after_forced_break: bool,
        output: &mut inline_layout::InlineIntrinsicMeasurement,
    ) -> bool {
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line {
                let line_index = output.sequence.records.len();
                output
                    .sequence
                    .records
                    .push(inline_layout::InlineLineRecord {
                        paragraph_index: output.paragraphs.len(),
                        block_line_index: line_index,
                        paragraph_line_index: 0,
                        fragment: None,
                        is_first_formatted_line: line_index == 0,
                        is_last_line_in_paragraph: true,
                        is_forced_empty: true,
                        clear_after: Clear::None,
                        block_start_trim: 0.0,
                        block_end_trim: 0.0,
                        paragraph_last_hanging_width: 0.0,
                        used_indent: used_line_indent(
                            line_index,
                            starts_after_forced_break,
                            context.hanging_indent,
                            context.block_style,
                            context.available_width,
                        ),
                        available_width: context.available_width,
                        line_height: context.block_style.line_height,
                    });
                return true;
            }
            return false;
        }
        let paragraph_index = output.paragraphs.len();
        let paragraph_start_line_index = output.sequence.records.len();
        let graph = self.build_inline_opportunity_graph(paragraph.iter(), context.block_style);
        let contribution = graph.intrinsic_contribution(&mut self.font_system, context.block_style);
        let (lines, next_line_count) = self.select_inline_lines_from_graph(
            &graph,
            context,
            paragraph_start_line_index,
            starts_after_forced_break,
        );
        output.contribution.min_content = output
            .contribution
            .min_content
            .max(contribution.min_content);
        output.contribution.max_content = output
            .contribution
            .max_content
            .max(contribution.max_content);
        let paragraph_last_hanging_width = lines
            .last()
            .map(|line| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line.items,
                    context.block_style,
                )
            })
            .unwrap_or(0.0);
        let line_count = lines.len();
        for (offset, line) in lines.into_iter().enumerate() {
            let line_index = paragraph_start_line_index + offset;
            output
                .sequence
                .records
                .push(inline_layout::InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: offset,
                    is_first_formatted_line: line_index == 0,
                    is_last_line_in_paragraph: offset + 1 == line_count,
                    is_forced_empty: false,
                    clear_after: Clear::None,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width,
                    used_indent: line.indent,
                    available_width: line.available_width,
                    line_height: line.metrics.height.max(context.block_style.line_height),
                    fragment: Some(line),
                });
        }
        debug_assert_eq!(output.sequence.records.len(), next_line_count);
        output
            .paragraphs
            .push(inline_layout::InlineMeasuredParagraph {
                graph,
                contribution,
            });
        paragraph.clear();
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_multicol_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        padding: (f32, f32),
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
        content_height: Option<f32>,
    ) -> bool {
        if marker.is_some() {
            return false;
        }
        let available_width = self.current_content_logical_inline_size().max(1.0);
        let gap = used_multicol_column_gap(style.column_gap, available_width, style.font_size);
        if used_multicol_column_count(style, available_width, gap)
            .filter(|count| *count > 1)
            .is_none()
        {
            return false;
        }
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(
                style,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        self.push_generated_pseudo_items(
            element,
            style,
            style.before_style.as_deref(),
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        self.collect_element_content_or_inline_items(
            element,
            style,
            stylesheets,
            link_target.clone(),
            InlinePlacement::zero(),
            &mut items,
        );
        self.push_generated_pseudo_items(
            element,
            style,
            style.after_style.as_deref(),
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(
                style,
                link_target,
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        self.try_layout_multicol_inline_items(
            items,
            style,
            available_width,
            padding,
            content_height,
        )
        .is_ok()
    }

    pub(in crate::layout) fn try_layout_multicol_inline_items(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_width: f32,
        padding: (f32, f32),
        content_height: Option<f32>,
    ) -> Result<(), Vec<InlineItem>> {
        let gap = used_multicol_column_gap(style.column_gap, available_width, style.font_size);
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return Err(items);
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let (padding_left, padding_right) = padding;
        let available_column_width = (column_width - padding_left - padding_right).max(1.0);
        let mut sequence_style = style.clone();
        sequence_style.box_decoration_break = css::BoxDecorationBreak::Clone;
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            &sequence_style,
            available_column_width,
            padding_left,
            0.0,
        );
        let column_height = content_height
            .unwrap_or_else(|| sequence.balanced_multicolumn_height(column_count, style))
            .max(style.line_height);
        self.paint_inline_line_sequence_multicolumn(
            &sequence,
            style,
            column_count,
            gap,
            column_width,
            column_height,
        );
        Ok(())
    }

    pub(in crate::layout) fn layout_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        padding: (f32, f32),
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let (padding_left, padding_right) = padding;
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left + padding_left,
                self.content_right - padding_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(
                style,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        self.push_generated_pseudo_items(
            element,
            style,
            style.before_style.as_deref(),
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        self.collect_element_content_or_inline_items(
            element,
            style,
            stylesheets,
            link_target.clone(),
            InlinePlacement::zero(),
            &mut items,
        );
        self.push_generated_pseudo_items(
            element,
            style,
            style.after_style.as_deref(),
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(
                style,
                link_target,
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        self.layout_inline_items(
            items,
            style,
            available_width,
            padding_left,
            0.0,
            stylesheets,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_run_in_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let available_width = self.current_content_logical_inline_size().max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(
                style,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        self.collect_inline_box_items(
            run_in_children,
            stylesheets,
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration,
            &mut items,
        );
        if style.content.is_generated() {
            self.push_element_content_items_from_boxes(
                element,
                style,
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                style,
                style.text_decoration,
                &mut items,
            );
        } else {
            self.collect_inline_box_items(
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                style,
                style.text_decoration,
                &mut items,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(
                style,
                link_target,
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
        }
        if !items.is_empty() {
            self.layout_inline_items(items, style, available_width, 0.0, 0.0, stylesheets);
        }
    }

    pub(in crate::layout) fn collect_intrinsic_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for child in &element.children {
            match &child.kind {
                NodeKind::Text(text) => {
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        placement.baseline_shift,
                        placement.visual_offset,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    let child_signature = ElementSignature::with_sibling_list(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature,
                        stylesheets,
                        Some(style),
                    );
                    if child_style.float != Float::None
                        || matches!(child_style.position, Position::Absolute | Position::Fixed)
                        || child_style.display.is_none()
                        || child_style.display.is_block_level()
                    {
                        continue;
                    }
                    child_style.text_decoration = child_style
                        .text_decoration
                        .with_propagated_lines(style.text_decoration);
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement = placement
                        .with_added_baseline_shift(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &child_style,
                                style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&child_style),
                        );
                    if let Some(mut atom) = self.intrinsic_inline_atom_for_element(
                        child_element,
                        &child_style,
                        &[],
                        None,
                        stylesheets,
                        placement.baseline_shift,
                        child_placement.visual_offset,
                        link.clone(),
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                        continue;
                    }
                    let scope = self.begin_inline_element_scope(
                        child_element,
                        &child_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::DOM_INTRINSIC,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.before_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Rollback,
                        output,
                    );
                    self.collect_intrinsic_element_content_or_inline_items(
                        child_element,
                        &child_style,
                        stylesheets,
                        link.clone(),
                        child_placement,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.after_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Rollback,
                        output,
                    );
                    self.end_inline_element_scope(scope, &child_style, output);
                }
            }
        }
    }

    pub(in crate::layout) fn collect_intrinsic_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        if style.content.is_generated() {
            self.push_intrinsic_element_content_items_from_dom(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        } else {
            self.collect_intrinsic_inline_items(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        }
    }

    pub(in crate::layout) fn push_intrinsic_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_intrinsic_inline_items(
                        element,
                        style,
                        stylesheets,
                        inherited_link.clone(),
                        placement,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                placement.baseline_shift,
                placement.visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_intrinsic_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_intrinsic_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        IntrinsicInlineCollectionContext {
                            baseline_shift,
                            visual_offset,
                            block_style: style,
                            propagated_decoration,
                        },
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    /// Push a regular inline box edge into the inline formatting stream.
    ///
    /// CSS Inline treats inline-level margin, border, and padding as part of
    /// the inline box fragments. Keeping the start/end edges as explicit atomic
    /// items lets line fitting account for positive, negative, and zero-net
    /// inline margins while preserving the nonnegative border/padding paint:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting> and
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>.
    pub(in crate::layout) fn push_inline_box_edge_item(
        &mut self,
        style: &ComputedStyle,
        edge: InlineBoxEdge,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        let width = inline_box_edge_width(style, edge);
        if !inline_box_edge_has_nonzero_component(style, edge) {
            return;
        }
        let (_, border, padding) = inline_box_edge_components(style, edge);
        let edge_fragment = InlineBoxEdgeFragment {
            logical_edge: match edge {
                InlineBoxEdge::Start => InlineLogicalEdge::Start,
                InlineBoxEdge::End => InlineLogicalEdge::End,
            },
            physical_side: inline_box_edge_physical_side(style, edge),
            advance: width,
            paint_extent: (border + padding).max(0.0),
        };
        let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        output.push(InlineItem::Atom(Box::new(
            InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment)),
                style.clone(),
                None,
                width,
                style.line_height,
                baseline_offset,
                baseline_shift,
                link_target,
                None,
            )
            .with_visual_offset(visual_offset),
        )));
    }

    pub(in crate::layout) fn begin_inline_element_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        link_target: Option<String>,
        placement: InlinePlacement,
        options: InlineElementScopeOptions,
        output: &mut Vec<InlineItem>,
    ) -> InlineElementScopeState {
        let counter_scope = self.begin_counter_scope(element, style);
        let inline_box_start = output.len();
        if options.fragment_edges.owns_start {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::Start,
                placement.baseline_shift,
                placement.visual_offset,
                None,
                output,
            );
        }
        let pushed_page_scope = options.push_page_scope && style.page_name_specified;
        if pushed_page_scope {
            output.push(InlineItem::PageScopeStart(style.page_name.clone()));
        }
        self.push_bidi_scope_start(
            style,
            link_target.clone(),
            placement.baseline_shift,
            placement.visual_offset,
            output,
        );
        if options.push_inside_marker
            && style.display.is_list_item()
            && style.list_style_position == ListStylePosition::Inside
            && let Some(marker) =
                self.marker_for_list_item(element, style, self.containing_block_direction)
        {
            self.push_inside_marker_items(&marker, style, link_target.clone(), output);
        }
        InlineElementScopeState {
            inline_box_start,
            link_target,
            baseline_shift: placement.baseline_shift,
            visual_offset: placement.visual_offset,
            pushed_page_scope,
            mark_hanging_edges: options.mark_hanging_edges,
            fragment_edges: options.fragment_edges,
            counter_scope,
        }
    }

    pub(in crate::layout) fn end_inline_element_scope(
        &mut self,
        state: InlineElementScopeState,
        style: &ComputedStyle,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_end(
            style,
            state.link_target,
            state.baseline_shift,
            state.visual_offset,
            output,
        );
        if state.pushed_page_scope {
            output.push(InlineItem::PageScopeEnd);
        }
        if state.fragment_edges.owns_end {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::End,
                state.baseline_shift,
                state.visual_offset,
                None,
                output,
            );
        }
        if state.mark_hanging_edges {
            mark_inline_box_hanging_edges(
                output,
                state.inline_box_start,
                style,
                state.fragment_edges,
            );
        }
        mark_inline_box_ancestor_decorations(output, state.inline_box_start, style);
        self.end_counter_scope(state.counter_scope);
    }

    pub(in crate::layout) fn collect_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        if style.content.is_generated() {
            self.push_element_content_items_from_dom(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        } else {
            self.collect_inline_items(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        }
    }
}
