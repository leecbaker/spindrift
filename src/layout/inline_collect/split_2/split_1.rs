use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Run a speculative measurement without materializing positioned
    /// descendants.
    ///
    /// Absolutely positioned boxes have no in-flow intrinsic contribution.
    /// Their static-position geometry is only available during the committed
    /// formatting pass, so an intrinsic probe must not leave a provisional
    /// page-relative paint layer behind.
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning>
    pub(in crate::layout) fn with_positioned_layout_suppressed<R>(
        &mut self,
        measure: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.positioned_inline_layout_suppression_depth += 1;
        let result = measure(self);
        self.positioned_inline_layout_suppression_depth -= 1;
        result
    }

    /// Scope the percentage basis visible while collecting an intrinsic inline
    /// contribution. The available line width can remain a layout constraint
    /// even when it is cyclic and therefore unusable for percentage sizing.
    /// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
    pub(in crate::layout) fn with_intrinsic_inline_percentage_basis<R>(
        &mut self,
        basis: IntrinsicInlinePercentageBasis,
        collect: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.intrinsic_inline_percentage_basis_stack.push(basis);
        let result = collect(self);
        self.intrinsic_inline_percentage_basis_stack.pop();
        result
    }

    pub(in crate::layout) fn inline_visual_offset_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> InlineVisualOffset {
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            return InlineVisualOffset::zero();
        }
        let style = self.style_with_current_used_lengths(style);
        InlineVisualOffset::from_relative_offset(self.normal_flow_relative_position_offset(&style))
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
        // Size containment measures the principal box as if it had no
        // content. It has no effect on non-atomic inline boxes.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if style.contain.size
            && (!style.display.is_inline_level() || style.display.is_atomic_inline())
        {
            return inline_layout::InlineIntrinsicMeasurement::default();
        }
        self.with_positioned_layout_suppressed(|layout| {
            let items =
                layout.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
            layout.intrinsic_inline_measurement_for_items(items, style, available_width)
        })
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        self.with_positioned_layout_suppressed(|layout| {
            let items = layout.intrinsic_inline_items_for_boxes(children, style, stylesheets);
            layout.intrinsic_inline_measurement_for_items(items, style, available_width)
        })
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_boxes_with_marker(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        marker: &ListMarker,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        self.with_positioned_layout_suppressed(|layout| {
            let mut items = Vec::new();
            layout.push_inside_marker_items(marker, style, None, &mut items);
            layout.collect_intrinsic_inline_box_items(
                children,
                stylesheets,
                None,
                IntrinsicInlineCollectionContext {
                    baseline_shift: 0.0,
                    visual_offset: InlineVisualOffset::zero(),
                    block_style: style,
                    propagated_decoration: style.text_decoration.clone(),
                },
                &mut items,
            );
            layout.intrinsic_inline_measurement_for_items(items, style, available_width)
        })
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
        // An inline-level list-item has an inside marker.  The marker is part
        // of its shrink-to-fit contribution just as it is part of the line
        // sequence used to paint the atomic inline box.
        // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
        if style.display.is_list_item()
            && style.display.is_inline_level()
            && let Some(marker) =
                self.marker_for_list_item(element, style, self.containing_block_direction)
            && marker.participates_in_first_line()
        {
            self.push_inside_marker_items(&marker, style, None, &mut items);
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
                    style.text_decoration.clone(),
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
                        propagated_decoration: style.text_decoration.clone(),
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
                propagated_decoration: style.text_decoration.clone(),
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
        self.form_text_combine_upright_atoms(&mut items);
        insert_text_autospace_items(&mut self.font_system, &mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            stylesheets: &[],
            initial_first_formatted_line: true,
            available_width: available_width.max(0.0),
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
                        is_phantom: false,
                        is_first_formatted_line: line_index == 0,
                        is_last_line_in_paragraph: true,
                        is_forced_empty: true,
                        starts_after_preserved_segment_break: false,
                        clear_after: Clear::None,
                        block_before: 0.0,
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
        let mut contribution =
            graph.intrinsic_contribution(&mut self.font_system, context.block_style);
        // The max-content contribution includes the first formatted line's
        // `text-indent`. Its percentage component is cyclic in this query and
        // therefore resolves against zero, while any absolute component still
        // changes that line's unwrapped inline extent. The min-content
        // contribution instead measures the longest unbreakable segment: an
        // indent does not make that segment wider.
        // <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes> and
        // <https://www.w3.org/TR/css-text-3/#text-indent-property>.
        let intrinsic_indent = used_line_indent_for_formatted_line(
            paragraph_start_line_index == 0,
            starts_after_forced_break,
            context.hanging_indent,
            context.block_style,
            0.0,
        );
        contribution.max_content = LogicalInlineContentSize::new(content_box_pt(
            (contribution.max_content.points() + intrinsic_indent).max(0.0),
        ));
        let selected_lines = self.select_inline_lines_from_graph(
            &graph,
            context,
            paragraph_start_line_index,
            starts_after_forced_break,
        );
        let lines = selected_lines.fragments;
        let next_line_count = selected_lines.next_line_index;
        output.sequence.has_flow_side_effects |= selected_lines.has_float_side_effects;
        output.contribution.include_max(contribution);
        // Floats do not supply an in-flow line fragment, but an explicit
        // forced break after a float-only range still contributes one empty
        // line to intrinsic block-size. Keep intrinsic measurement aligned
        // with the collected paint-time line sequence.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>
        if force_empty_line && lines.is_empty() {
            output
                .sequence
                .records
                .push(inline_layout::InlineLineRecord {
                    paragraph_index,
                    block_line_index: next_line_count,
                    paragraph_line_index: 0,
                    fragment: None,
                    is_phantom: false,
                    is_first_formatted_line: next_line_count == 0,
                    is_last_line_in_paragraph: true,
                    is_forced_empty: true,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width: 0.0,
                    used_indent: used_line_indent(
                        next_line_count,
                        starts_after_forced_break,
                        context.hanging_indent,
                        context.block_style,
                        context.available_width,
                    ),
                    available_width: context.available_width,
                    line_height: context.block_style.line_height,
                });
            output
                .paragraphs
                .push(inline_layout::InlineMeasuredParagraph {
                    graph,
                    contribution,
                });
            paragraph.clear();
            return true;
        }
        let paragraph_last_hanging_width = lines
            .last()
            .map(|line| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line.items,
                    context.block_style,
                )
            })
            .map(SemanticLengthExt::points)
            .unwrap_or(0.0);
        let line_count = lines.len();
        let mut next_record_line_index = paragraph_start_line_index;
        for (offset, line) in lines.into_iter().enumerate() {
            while next_record_line_index < line.line_index {
                output
                    .sequence
                    .records
                    .push(inline_layout::InlineLineRecord {
                        paragraph_index,
                        block_line_index: next_record_line_index,
                        paragraph_line_index: output.sequence.records.len()
                            - paragraph_start_line_index,
                        fragment: None,
                        is_phantom: false,
                        is_first_formatted_line: next_record_line_index == 0,
                        is_last_line_in_paragraph: false,
                        // A float-excluded line has no selected source range, but
                        // it still consumes block-size before the next float band.
                        is_forced_empty: true,
                        starts_after_preserved_segment_break: false,
                        clear_after: Clear::None,
                        block_before: 0.0,
                        block_start_trim: 0.0,
                        block_end_trim: 0.0,
                        paragraph_last_hanging_width,
                        used_indent: 0.0,
                        available_width: context.available_width,
                        line_height: context.block_style.line_height,
                    });
                next_record_line_index += 1;
            }
            let line_index = line.line_index;
            let is_phantom = inline_layout::inline_line_fragment_is_phantom(&line);
            output
                .sequence
                .records
                .push(inline_layout::InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: output.sequence.records.len()
                        - paragraph_start_line_index,
                    is_phantom,
                    is_first_formatted_line: line_index == 0,
                    is_last_line_in_paragraph: offset + 1 == line_count,
                    is_forced_empty: false,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width,
                    used_indent: line.indent,
                    available_width: line.available_width,
                    line_height: line
                        .metrics
                        .height
                        .max(context.block_style.line_height)
                        .max(
                            line.items()
                                .iter()
                                .map(|item| {
                                    inline_line_item_logical_block_size(
                                        &item.item,
                                        context.block_style,
                                    )
                                })
                                .fold(0.0_f32, f32::max),
                        ),
                    fragment: Some(line.fragment),
                });
            next_record_line_index = line_index + 1;
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
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        padding: (f32, f32),
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
        content_height: Option<f32>,
    ) -> bool {
        let multicol_style = self.multicol_used_style(style);
        let style = &multicol_style;
        if marker.is_some() {
            return false;
        }
        // The principal box of a nested multicol can be measured in a
        // temporary fragmentainer wider than the active outer column. Its
        // anonymous columns nevertheless use that outer column's definite
        // content-box inline size as both their width and percentage basis.
        // <https://www.w3.org/TR/css-multicol-1/#column-box>
        let containing_column = self.multicol_column_containing_blocks.last().copied();
        let containing_inline_size = containing_column
            .map(|containing_block| containing_block.inline_size)
            .unwrap_or_else(|| {
                LogicalInlineContentSize::new(content_box_pt(
                    self.current_content_logical_inline_size(),
                ))
            });
        let available_width = containing_inline_size.points().max(1.0);
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        if let Some(containing_column) = containing_column {
            self.content_left = containing_column.content_left;
            self.content_right = containing_column.content_left + available_width;
        }
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return false;
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        // Atomic inline sizes are resolved while inline items are collected.
        // In multicol their percentage containing block is the anonymous
        // column box, not the multicol principal box. Scope both the physical
        // legacy width and the logical percentage basis to that column.
        // <https://www.w3.org/TR/css-multicol-1/#column-box>.
        let column_set_content_right = self.content_right;
        self.content_right = self.content_left + column_width;
        self.content_logical_inline_size_stack.push(column_width);
        self.multicol_column_containing_blocks
            .push(MulticolColumnContainingBlock {
                inline_size: LogicalInlineContentSize::new(content_box_pt(column_width)),
                content_left: self.content_left,
            });
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
        if let Some(child_boxes) = child_boxes {
            self.collect_inline_box_items(
                child_boxes,
                stylesheets,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                style,
                style.text_decoration.clone(),
                &mut items,
            );
        } else {
            self.collect_element_content_or_inline_items(
                element,
                style,
                stylesheets,
                link_target.clone(),
                InlinePlacement::zero(),
                &mut items,
            );
        }
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
        self.content_logical_inline_size_stack.pop();
        self.multicol_column_containing_blocks.pop();
        self.content_right = column_set_content_right;
        let result = self
            .try_layout_multicol_inline_items(
                items,
                style,
                available_width,
                padding,
                content_height,
            )
            .is_ok();
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        result
    }

    pub(in crate::layout) fn try_layout_multicol_inline_items(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_width: f32,
        padding: (f32, f32),
        content_height: Option<f32>,
    ) -> Result<(), Vec<InlineItem>> {
        let multicol_style = self.multicol_used_style(style);
        let style = &multicol_style;
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return Err(items);
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let (padding_left, padding_right) = padding;
        let available_column_width = (column_width - padding_left - padding_right).max(1.0);
        let mut sequence_style = style.as_computed().clone();
        sequence_style.box_decoration_break = css::BoxDecorationBreak::Clone;
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            &sequence_style,
            available_column_width,
            padding_left,
            0.0,
        );
        let auto_fill_max_height = (content_height.is_none()
            && style.column_fill == css::ColumnFill::Auto)
            .then(|| {
                used_max_height(style, PercentageBasis::definite(layout_pt(available_width)))
                    .map(SemanticLengthExt::points)
            })
            .flatten();
        let repeated_block_end_decoration =
            if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                style.padding.bottom + used_border_widths(style).bottom
            } else {
                0.0
            };
        let remaining_parent_height =
            (self.cursor_y - self.page_bottom() - repeated_block_end_decoration)
                .max(css::CSS_PX_TO_PT);
        let balanced_height = sequence.balanced_multicolumn_height(column_count, style);
        let sequential_auto_height = sequence.total_height().max(style.line_height);
        let natural_column_height =
            content_height
                .or(auto_fill_max_height)
                .unwrap_or_else(|| match style.column_fill {
                    css::ColumnFill::Auto => sequential_auto_height,
                    css::ColumnFill::Balance | css::ColumnFill::BalanceAll => balanced_height,
                });
        let fragmented_by_parent = self.active_fragmentainer_kind() == FragmentainerKind::Column
            && natural_column_height > remaining_parent_height + 0.01;
        let definite_fragment_height = content_height.map(|height| {
            if fragmented_by_parent {
                height.min(remaining_parent_height)
            } else {
                height
            }
        });
        let unconstrained_column_height = match style.column_fill {
            css::ColumnFill::Auto => definite_fragment_height
                .or(auto_fill_max_height)
                .unwrap_or(sequential_auto_height),
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll => definite_fragment_height
                .map(|limit| balanced_height.min(limit))
                .unwrap_or(balanced_height),
        };
        // An auto-block-size nested multicol whose natural balanced row is
        // taller than the active outer column must fragment that row at the
        // outer fragmentainer boundary. The final row is rebalanced separately
        // by the painter and may shrink below this limit.
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>.
        let column_height = if fragmented_by_parent {
            unconstrained_column_height.min(remaining_parent_height)
        } else {
            unconstrained_column_height
        }
        .max(style.line_height.min(remaining_parent_height));
        let used_column_set_height = if let Some(height) = definite_fragment_height {
            height
        } else if let Some(max_height) = auto_fill_max_height {
            sequence
                .total_height()
                .min(max_height)
                .max(style.line_height)
        } else {
            column_height
        };
        self.paint_inline_line_sequence_multicolumn(
            &sequence,
            style,
            inline_layout::MulticolumnInlinePaintGeometry {
                column_count,
                column_gap: gap,
                column_width,
                column_height,
                used_column_set_height,
                wrap_column_rows: fragmented_by_parent,
                shrink_final_row: content_height.is_none(),
            },
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
            && marker.paints_outside()
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
            && marker.participates_in_first_line()
            && !marker.follows_content_in_first_line()
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
        if let Some(marker) = marker
            && marker.follows_content_in_first_line()
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
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
        // A DOM fallback can be selected from descendant features while all
        // of its source children are owned by frozen block formatting boxes.
        // Do not turn that empty collection into a phantom line box before
        // the block children are laid out.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        if items.is_empty() {
            return;
        }
        let _ = self.layout_inline_items(
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
            && marker.paints_outside()
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
            && marker.participates_in_first_line()
            && !marker.follows_content_in_first_line()
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        let run_in_item_start = items.len();
        self.collect_inline_box_items(
            run_in_children,
            stylesheets,
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration.clone(),
            &mut items,
        );
        if let Some(marker) = marker
            && marker.follows_content_in_first_line()
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        mark_inline_text_items_as_run_in(&mut items[run_in_item_start..]);
        if style.content.is_generated() {
            self.push_element_content_items_from_boxes(
                element,
                style,
                box_tree::CounterEventSource::Principal,
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                style,
                style.text_decoration.clone(),
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
                style.text_decoration.clone(),
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
                    if element_suppresses_direct_text_children(element) {
                        continue;
                    }
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
                    if is_html_select_item_element(child_element)
                        && !has_html_select_context(element, &self.ancestors)
                    {
                        continue;
                    }
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
                        .with_propagated_lines(style.text_decoration.clone());
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
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope = self.begin_counter_scope(child_element, &child_style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        child_element,
                        &child_style,
                        &[],
                        None,
                        stylesheets,
                        placement.baseline_shift,
                        child_placement.visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(mut atom) = atom {
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
                box_tree::CounterEventSource::Principal,
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
                            propagated_decoration: propagated_decoration.clone(),
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
                box_tree::CounterEventSource::Principal,
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
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_inline_box_edge_item(
        &mut self,
        style: &ComputedStyle,
        edge: InlineBoxEdge,
        positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        // Inline atom advance is a line-coordinate input.
        let width = inline_box_edge_width(style, edge).points();
        // Retain zero-advance edges as lexical scope markers. CSS Text gives
        // a visual tracking boundary to the innermost inline ancestor shared
        // by its two typographic units; eliding an undecorated `span` loses
        // that ancestry even though it has no box geometry. Positioned
        // inlines additionally use the same marker for their containing
        // block, so one durable representation serves both concerns.
        // <https://www.w3.org/TR/css-text-3/#letter-spacing> and
        // <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
        let (_, border, padding) = inline_box_edge_components(style, edge);
        let edge_fragment = InlineBoxEdgeFragment {
            logical_edge: match edge {
                InlineBoxEdge::Start => InlineLogicalEdge::Start,
                InlineBoxEdge::End => InlineLogicalEdge::End,
            },
            physical_side: inline_box_edge_physical_side(style, edge),
            positioning_containing_block_id,
            advance: width,
            paint_extent: (border + padding).max(0.0),
        };
        let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        output.push(InlineItem::Atom(Box::new(
            InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment)),
                style.clone(),
                None,
                InlineSize::new(width, style.line_height),
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
        let counter_snapshot = (!options.push_page_scope).then(|| self.counter_set.clone());
        let counter_scope = self.begin_counter_scope(element, style);
        let inline_box_start = output.len();
        // Inline box edges consume used margins, borders, and padding. Resolve
        // selected-font metric units before projecting those edges so `ch`,
        // `ex`, and related values do not silently become their stale
        // length-only cache value.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
        // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
        let mut edge_style = self.style_with_current_used_lengths(style);
        let edge_percentage_basis = if options.push_page_scope {
            self.current_content_logical_inline_size().max(0.0)
        } else {
            0.0
        };
        apply_used_box_metrics(
            &mut edge_style,
            PercentageBasis::definite(layout_pt(edge_percentage_basis)),
        );
        let positioning_containing_block_source =
            inline_scope_establishes_positioning_containing_block(&edge_style).then(|| {
                InlinePositioningContainingBlockSource {
                    id: InlinePositioningContainingBlockId(inline_box_start),
                    style: edge_style.clone(),
                }
            });
        if options.fragment_edges.owns_start {
            self.push_inline_box_edge_item(
                &edge_style,
                InlineBoxEdge::Start,
                positioning_containing_block_source
                    .as_ref()
                    .map(|source| source.id),
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
            && (style.list_style_position == ListStylePosition::Inside
                || style.display.is_inline_level())
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
            edge_style,
            positioning_containing_block_source,
            pushed_page_scope,
            mark_hanging_edges: options.mark_hanging_edges,
            fragment_edges: options.fragment_edges,
            counter_scope,
            counter_snapshot,
        }
    }

    pub(in crate::layout) fn end_inline_element_scope(
        &mut self,
        state: InlineElementScopeState,
        _style: &ComputedStyle,
        output: &mut Vec<InlineItem>,
    ) {
        let InlineElementScopeState {
            inline_box_start,
            link_target,
            baseline_shift,
            visual_offset,
            edge_style,
            positioning_containing_block_source,
            pushed_page_scope,
            mark_hanging_edges,
            fragment_edges,
            counter_scope,
            counter_snapshot,
        } = state;
        self.push_bidi_scope_end(
            &edge_style,
            link_target,
            baseline_shift,
            visual_offset,
            output,
        );
        if pushed_page_scope {
            output.push(InlineItem::PageScopeEnd);
        }
        if fragment_edges.owns_end {
            self.push_inline_box_edge_item(
                &edge_style,
                InlineBoxEdge::End,
                positioning_containing_block_source
                    .as_ref()
                    .map(|source| source.id),
                baseline_shift,
                visual_offset,
                None,
                output,
            );
        }
        if mark_hanging_edges {
            mark_inline_box_hanging_edges(output, inline_box_start, &edge_style, fragment_edges);
        }
        mark_inline_box_ancestor_decorations(
            output,
            inline_box_start,
            &edge_style,
            positioning_containing_block_source
                .as_ref()
                .map(|source| source.id),
        );
        self.end_counter_scope(counter_scope);
        if let Some(counter_snapshot) = counter_snapshot {
            self.counter_set = counter_snapshot;
        }
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

fn mark_inline_text_items_as_run_in(items: &mut [InlineItem]) {
    for item in items {
        if let InlineItem::Word(word) = item
            && word.source != InlineTextSource::Marker
        {
            word.source = InlineTextSource::RunIn;
        }
    }
}
