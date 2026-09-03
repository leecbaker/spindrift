use super::generated_content::{
    annotate_line_break_element_breaks_with_clear, generated_content_originating_clear,
};
use super::*;
use crate::layout::block::{AtomicPrincipalFlow, classify_atomic_principal_flow};

impl<'a> LayoutBuilder<'a> {
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        // Size containment measures the principal box as if it had no
        // content. It has no effect on non-atomic inline boxes.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if used_property_containment(element, style).size {
            return inline_layout::InlineIntrinsicMeasurement::default();
        }
        self.with_positioned_layout_suppressed(|layout| {
            let mut items =
                layout.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
            // A block-level list item's principal inline flow is measured
            // here for intrinsic sizing, but its marker is deliberately not
            // part of `intrinsic_inline_items_for_element`: that generic
            // collector also serves vertical and atomic-inline paths which
            // already inject their marker elsewhere. In horizontal writing,
            // an inside marker contributes to the list item's min/max
            // content sizes just as it does to its first formatted line.
            //
            // The marker is inserted only into the list item's own measured
            // stream. Recursive block sizing consumes that contribution for
            // floats and ancestors without making the marker inline content
            // of a parent block.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            if style.writing_mode == WritingMode::HorizontalTb
                && style.display.is_list_item()
                && style.display.is_block_level()
                && let Some(marker) =
                    layout.marker_for_list_item(element, style, layout.containing_block_direction)
                && marker.participates_in_first_line()
            {
                let mut marker_items = Vec::new();
                layout.push_inside_marker_items(&marker, style, None, &mut marker_items);
                marker_items.append(&mut items);
                items = marker_items;
            }
            layout.intrinsic_inline_measurement_for_items(items, style, available_width)
        })
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
                    propagated_decoration_layers: style
                        .text_decoration_origins
                        .effective_layers_vec(),
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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Vec<InlineItem> {
        let mut items = Vec::new();
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        // An inline-level list-item has an inside marker. The marker is part
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
            if style.content.is_generated() && child_boxes.is_empty() {
                // A generated pseudo can have no frozen formatting children
                // even though its `content` still supplies literal generated
                // text.  Measure that content from the originating style so
                // an inside marker's separator remains followed by the
                // pseudo's generated run instead of being treated as a
                // trailing collapsible space.  This matters for a
                // shrink-to-fit generated `display: list-item` float.
                // <https://drafts.csswg.org/css-content-3/#content-property>
                self.push_intrinsic_element_content_items_from_dom(
                    element,
                    style,
                    stylesheets,
                    None,
                    InlinePlacement::zero(),
                    &mut items,
                );
            } else if style.content.is_generated() {
                self.push_intrinsic_element_content_items_from_boxes(
                    element,
                    style,
                    child_boxes,
                    stylesheets,
                    None,
                    0.0,
                    InlineVisualOffset::zero(),
                    style.text_decoration_origins.effective_layers_vec(),
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
                        propagated_decoration_layers: style
                            .text_decoration_origins
                            .effective_layers_vec(),
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
        stylesheets: &Stylesheets<'_>,
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
                propagated_decoration_layers: style.text_decoration_origins.effective_layers_vec(),
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
        #[cfg(feature = "layout-profile")]
        let _profile_scope = crate::layout::layout_profile::inline_intrinsic_measurement_scope();
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
        insert_text_autospace_items(
            &mut self.font_system,
            &mut self.autospace_items_scratch,
            &mut items,
        );
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            line_clamp: used_line_clamp_for_style(block_style),
            clamp_continuation: css::ClampContinuation::None,
            stylesheets: &css::EMPTY_STYLESHEETS,
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
                    let force_empty_line = true;
                    self.flush_intrinsic_inline_measurement_paragraph(
                        &mut paragraph,
                        context,
                        force_empty_line,
                        inline_layout::InlineLineTermination::ForcedBreak,
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
                        inline_layout::InlineLineTermination::CollectionBoundary,
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
            inline_layout::InlineLineTermination::BlockEnd,
            starts_after_forced_break,
            &mut output,
        );
        let records = std::mem::take(&mut output.sequence.records);
        let (records, fragment_text_box_trim) = self.with_text_box_line_trim_applied(
            records,
            block_style,
            self.current_text_box_line_trim(),
        );
        output.sequence.records = records;
        output.sequence.fragment_text_box_trim = fragment_text_box_trim;
        let mut preceding_line_direction = None;
        output
            .sequence
            .resolve_bidi_base_directions(block_style, &mut preceding_line_direction);
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        terminal_termination: inline_layout::InlineLineTermination,
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
                        kind: inline_layout::InlineLineKind::ForcedEmpty,
                        is_first_formatted_line: line_index == 0,
                        is_last_line_in_paragraph: true,
                        termination: terminal_termination,
                        used_bidi_base_direction: None,
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
                        text_fit_used_style: None,
                        decoration_origin_fragments: Default::default(),
                    });
                return true;
            }
            return false;
        }
        let paragraph_index = output.paragraphs.len();
        let paragraph_start_line_index = output.sequence.records.len();
        let graph = self.build_inline_opportunity_graph(paragraph.iter(), context.block_style);
        // Intrinsic contributions must measure the same pseudo-generated
        // first-letter fragment as final line layout. In particular, an
        // auto-sized absolutely positioned inline box uses these values for
        // shrink-to-fit width before its final paint-time line is selected.
        // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo> and
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
        let graph = if context.initial_first_formatted_line && paragraph_start_line_index == 0 {
            self.graph_with_first_letter_pseudo(&graph, context.block_style)
        } else {
            graph
        };
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
                    kind: inline_layout::InlineLineKind::ForcedEmpty,
                    is_first_formatted_line: next_line_count == 0,
                    is_last_line_in_paragraph: true,
                    termination: terminal_termination,
                    used_bidi_base_direction: None,
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
                    text_fit_used_style: None,
                    decoration_origin_fragments: Default::default(),
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
                        kind: inline_layout::InlineLineKind::ForcedEmpty,
                        is_first_formatted_line: next_record_line_index == 0,
                        is_last_line_in_paragraph: false,
                        termination: inline_layout::InlineLineTermination::SoftWrap,
                        // A float-excluded line has no selected source range, but
                        // it still consumes block-size before the next float band.
                        used_bidi_base_direction: None,
                        starts_after_preserved_segment_break: false,
                        clear_after: Clear::None,
                        block_before: 0.0,
                        block_start_trim: 0.0,
                        block_end_trim: 0.0,
                        paragraph_last_hanging_width,
                        used_indent: 0.0,
                        available_width: context.available_width,
                        line_height: context.block_style.line_height,
                        text_fit_used_style: None,
                        decoration_origin_fragments: Default::default(),
                    });
                next_record_line_index += 1;
            }
            let line_index = line.line_index;
            let preserve_empty_line = force_empty_line && offset + 1 == line_count;
            let kind = inline_layout::InlineLineKind::for_fragment(&line, preserve_empty_line);
            output
                .sequence
                .records
                .push(inline_layout::InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: output.sequence.records.len()
                        - paragraph_start_line_index,
                    kind,
                    is_first_formatted_line: line_index == 0 && !kind.is_phantom(),
                    is_last_line_in_paragraph: offset + 1 == line_count,
                    termination: if offset + 1 == line_count {
                        terminal_termination
                    } else {
                        inline_layout::InlineLineTermination::SoftWrap
                    },
                    used_bidi_base_direction: None,
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
                    text_fit_used_style: None,
                    decoration_origin_fragments: Default::default(),
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
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn collect_intrinsic_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
                        placement.baseline_shift(),
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
                    let child_signature = ElementSignature::from_sibling_snapshot(
                        element_index,
                        sibling_tags.clone(),
                    )
                    .expect("source child must have a cached sibling signature");
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature.clone(),
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
                    // Intrinsic collection recursively resolves descendant
                    // styles as well, so it must observe the same source-DOM
                    // selector ancestry as final inline layout.
                    // <https://drafts.csswg.org/selectors-4/#child-combinators>
                    self.with_ancestor_signature(child_signature, |layout| {
                        let link = child_element
                            .attrs
                            .get("href")
                            .cloned()
                            .or_else(|| inherited_link.clone());
                        let child_placement = placement
                            .with_added_baseline_placement(
                                layout.vertical_align_baseline_shift_for_inline_style(
                                    &child_style,
                                    style,
                                ),
                            )
                            .with_added_visual_offset(
                                layout.inline_visual_offset_for_style(&child_style),
                            );
                        let counter_snapshot = layout.counter_set.clone();
                        let counter_scope = layout.begin_counter_scope(child_element, &child_style);
                        let atom = layout.intrinsic_inline_atom_for_element(
                            child_element,
                            &child_style,
                            &[],
                            None,
                            stylesheets,
                            placement.baseline_shift(),
                            child_placement.visual_offset,
                            link.clone(),
                        );
                        layout.end_counter_scope(counter_scope);
                        layout.counter_set = counter_snapshot;
                        if let Some(atom) = atom {
                            let atom = layout.finish_inline_atom_for_parent(atom, style);
                            output.push(InlineItem::Atom(Box::new(atom)));
                            return;
                        }
                        let scope = layout.begin_inline_element_scope(
                            child_element,
                            &child_style,
                            link.clone(),
                            child_placement,
                            InlineElementScopeOptions::DOM_INTRINSIC.with_preserved_empty_metrics(
                                empty_inline_scope_has_distinct_metrics(style, &child_style),
                            ),
                            output,
                        );
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.before_style.as_deref(),
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Rollback,
                            output,
                        );
                        layout.collect_intrinsic_element_content_or_inline_items(
                            child_element,
                            &child_style,
                            stylesheets,
                            link.clone(),
                            child_placement,
                            output,
                        );
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.after_style.as_deref(),
                            link,
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Rollback,
                            output,
                        );
                        layout.end_inline_element_scope(scope, &child_style, output);
                    });
                }
            }
        }
    }

    pub(in crate::layout) fn collect_intrinsic_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
                placement.baseline_shift(),
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
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
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
                            propagated_decoration_layers: propagated_decoration_layers.clone(),
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
}

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn intrinsic_inline_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        // Atomic inline dimensions participate directly in their containing
        // line's intrinsic contribution. Resolve viewport and font-relative
        // components before that contribution is captured; otherwise a
        // vertical `height: 20vh` is reduced to its line strut and cannot
        // negotiate against the orthogonal flow's available inline size.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flow>
        let used_style = self.style_with_current_used_lengths(style);
        let style = &used_style;
        let intrinsic_metrics = intrinsic_box_metrics(style);
        let available_width = (self.content_right
            - self.content_left
            - intrinsic_metrics.margin.left.points()
            - intrinsic_metrics.margin.right.points())
        .max(0.0);
        let inline_percentage_basis = self
            .intrinsic_inline_percentage_basis_stack
            .last()
            .cloned()
            .unwrap_or_else(|| {
                PercentageBasis::definite_from(
                    content_box_pt(available_width),
                    IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                )
            });
        // A source-less HTML image has zero natural dimensions. During an
        // intrinsic probe a percentage width is cyclic, so it cannot use the
        // probe's available line span to manufacture a table-column
        // contribution. Final layout revisits the element with the committed
        // containing-block width.
        // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
        // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
        if element.tag == "img"
            && crate::dom::selected_img_source(element).is_none()
            && !inline_percentage_basis.is_definite()
        {
            return None;
        }
        let replaced_sizing = ReplacedBoxSizingContext {
            available_width: content_box_pt(available_width),
            inline_percentage_basis,
            block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                self.block_percentage_context_stack
                    .current_percentage_basis(),
            ),
        };
        if let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        {
            let image = used_generated_image_value(
                image.as_image()?,
                style,
                available_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )?;
            let border_box_width = image.border_box_size.width;
            let border_box_height = image.border_box_size.height;
            let content = image.into_inline_atom_content();
            return Some(
                InlineAtom::new(
                    content,
                    style.clone(),
                    None,
                    InlineSize::new(
                        border_box_width
                            + intrinsic_metrics.margin.left.points()
                            + intrinsic_metrics.margin.right.points(),
                        border_box_height
                            + intrinsic_metrics.margin.top.points()
                            + intrinsic_metrics.margin.bottom.points(),
                    ),
                    border_box_height,
                    baseline_shift,
                    link_target,
                    self.generated_alt_text(element, style),
                )
                .with_visual_offset(visual_offset),
            );
        }
        let (width, height, baseline_offset) = match resolve_replaced_element(
            element,
            style,
            replaced_sizing,
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            Some(ResolvedReplacedElement::Canvas(canvas)) => {
                let border_box_width = canvas.border_box_size.width;
                let border_box_height = canvas.border_box_size.height;
                (
                    border_box_width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    border_box_height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    border_box_height,
                )
            }
            Some(ResolvedReplacedElement::Image(image)) => {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                (
                    border_box_width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    border_box_height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    border_box_height,
                )
            }
            Some(ResolvedReplacedElement::Svg(svg)) => {
                let width = svg.border_box_size.width;
                let height = svg.border_box_size.height;
                (
                    width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    height,
                )
            }
            None if style.display.is_table() => {
                // An inline table is an atomic inline with table-specific
                // sizing, painting, and baseline export.  A generic
                // intrinsic atom loses the first-row baseline and replaces
                // the captured table with an empty SVG placeholder.
                // <https://www.w3.org/TR/CSS22/tables.html#table-display>
                return self
                    .inline_table_atom_for_element(
                        element,
                        style,
                        children,
                        table_fragment?,
                        stylesheets,
                        baseline_shift,
                        link_target,
                    )
                    .map(|atom| atom.with_visual_offset(visual_offset));
            }
            None if style.display.is_flex() && style.display.is_inline_level() => {
                let box_metrics = intrinsic_box_metrics(style);
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let contributions = self.estimate_flex_intrinsic_widths(
                    element,
                    style,
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(available_width)),
                    Some(children),
                );
                let content_width = crate::layout::intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_width),
                    non_content_pt(horizontal_extras),
                    contributions.min_content.content_box_length(),
                    contributions.max_content.content_box_length(),
                    crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
                .points();
                (
                    constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                        + horizontal_extras
                        + box_metrics.margin.left.points()
                        + box_metrics.margin.right.points(),
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_grid() && style.display.is_inline_level() => {
                return Some(self.intrinsic_inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                ));
            }
            None if style.display.is_atomic_inline() => {
                let box_metrics = intrinsic_box_metrics(style);
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let vertical_extras = box_metrics.vertical_non_content_length().points();
                let containing_block_height = self
                    .block_percentage_context_stack
                    .current_percentage_basis();
                let definite_content_height = used_content_box_height_or_auto_with_basis(
                    style,
                    containing_block_height,
                    non_content_pt(vertical_extras),
                )
                .map(|height| {
                    constrain_content_height(
                        style,
                        height,
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                });
                self.block_percentage_context_stack.push_context(
                    DescendantBlockPercentageContext::formatting_context(
                        definite_content_height.map(content_box_pt),
                        BlockSizeBasisSource::InlineBlock,
                    ),
                );
                let principal_flow = if children.is_empty() {
                    AtomicPrincipalFlow::InlineSequence
                } else {
                    let has_block_children = has_non_inline_formatting_box(children);
                    classify_atomic_principal_flow(
                        false,
                        false,
                        !has_block_children && formatting_box_has_inline_content(children),
                    )
                };
                let contribution = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    self.intrinsic_inline_measurement_for_text(&text, style, available_width)
                        .contribution
                } else if matches!(principal_flow, AtomicPrincipalFlow::BlockChildren) {
                    let (min_content, max_content) = self.block_intrinsic_content_inline_sizes(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                        available_width,
                    );
                    inline_layout::InlineIntrinsicContribution {
                        min_content,
                        max_content,
                    }
                } else {
                    self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                    )
                };
                self.block_percentage_context_stack.pop();
                let content_width = if style.writing_mode.has_vertical_lines()
                    && matches!(principal_flow, AtomicPrincipalFlow::BlockChildren)
                    && style.box_values.height.is_auto()
                {
                    // The atomic parent negotiates this formatting context's
                    // logical inline size. In vertical writing that is the
                    // physical height, so resolve shrink-to-fit against the
                    // line's available inline measure rather than through
                    // the physical `width` property adapter.
                    crate::layout::intrinsic::shrink_to_fit_width(
                        contribution.min_content.content_box_length(),
                        contribution.max_content.content_box_length(),
                        content_box_pt(available_width),
                    )
                    .points()
                } else {
                    crate::layout::intrinsic::content_box_width_from_intrinsic(
                        style,
                        layout_pt(available_width),
                        non_content_pt(horizontal_extras),
                        contribution.min_content.content_box_length(),
                        contribution.max_content.content_box_length(),
                        crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                    )
                    .points()
                };
                let mut content_width = if style.box_values.width.is_auto() {
                    content_width.max(style.font_size)
                } else {
                    content_width
                };
                let block_flow_geometry = (style.writing_mode.has_vertical_lines()
                    && matches!(principal_flow, AtomicPrincipalFlow::BlockChildren))
                .then(|| {
                    // The shrink-to-fit result selected from this atomic
                    // context's intrinsic contribution is its logical inline
                    // measure in vertical writing. Ordinary block stretch
                    // sizing would replace this indefinite atomic measure
                    // with the initial containing block's physical height.
                    let logical_inline_size = LogicalInlineContentSize::new(content_box_pt(
                        definite_content_height.unwrap_or(content_width),
                    ));
                    let logical_block_size = LogicalBlockContentSize::new(content_box_pt(
                        self.estimate_block_child_intrinsic_logical_block_size(
                            element,
                            style,
                            stylesheets,
                            Some(children),
                            available_width,
                            Some(logical_inline_size),
                        ),
                    ));
                    (logical_inline_size, logical_block_size)
                });
                let (measured_content_height, measured_physical_height, line_baseline_offset) =
                    if let Some((logical_inline_size, logical_block_size)) = block_flow_geometry {
                        content_width = logical_block_size.points();
                        (
                            logical_block_size.points(),
                            logical_inline_size.points(),
                            self.estimate_block_child_intrinsic_last_baseline(
                                element,
                                style,
                                stylesheets,
                                Some(children),
                                available_width,
                                logical_inline_size,
                            )
                            .map(SemanticLengthExt::points),
                        )
                    } else if children.is_empty() {
                        let text = inline_text_for_style(element, style);
                        let measurement =
                            self.intrinsic_inline_measurement_for_text(&text, style, content_width);
                        (
                            measurement.height().max(style.line_height),
                            measurement.physical_height(style),
                            self.inline_box_sequence_baseline_offset(
                                &measurement.sequence,
                                style,
                                intrinsic_metrics.border.to_css_edges(),
                            ),
                        )
                    } else {
                        self.block_percentage_context_stack.push_context(
                            DescendantBlockPercentageContext::formatting_context(
                                definite_content_height.map(content_box_pt),
                                BlockSizeBasisSource::InlineBlock,
                            ),
                        );
                        let measurement = self.intrinsic_inline_measurement_for_element(
                            element,
                            style,
                            stylesheets,
                            Some(children),
                            content_width,
                        );
                        (
                            measurement.height().max(style.line_height),
                            measurement.physical_height(style),
                            self.inline_box_sequence_baseline_offset(
                                &measurement.sequence,
                                style,
                                intrinsic_metrics.border.to_css_edges(),
                            ),
                        )
                    };
                if !children.is_empty() {
                    self.block_percentage_context_stack.pop();
                }
                let vertical_writing_mode = style.writing_mode.has_vertical_lines();
                let has_intrinsic_inline_content =
                    !children.is_empty() || !inline_text_for_style(element, style).is_empty();
                if vertical_writing_mode
                    && style.box_values.width.is_auto()
                    && has_intrinsic_inline_content
                {
                    // A vertical inline-block's physical width is its logical
                    // block contribution, i.e. the stacked line-box extent.
                    // Its physical height is instead the used logical inline
                    // extent below. Keeping those projections distinct makes
                    // intrinsic atom sizing agree with final inline paint.
                    // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                    content_width = measured_content_height;
                }
                let content_height = if vertical_writing_mode {
                    definite_content_height.unwrap_or(measured_physical_height)
                } else {
                    definite_content_height.unwrap_or_else(|| {
                        constrain_content_height(
                            style,
                            content_box_pt(measured_content_height),
                            PercentageBasis::definite(layout_pt(available_width)),
                        )
                        .points()
                    })
                };
                let border_box_height = content_height + vertical_extras;
                let baseline_offset = Self::inline_block_baseline_offset(
                    style,
                    used_property_containment(element, style).layout,
                    if vertical_writing_mode {
                        content_width + horizontal_extras
                    } else {
                        border_box_height
                    },
                    line_baseline_offset,
                );
                (
                    constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                        + horizontal_extras
                        + box_metrics.margin.left.points()
                        + box_metrics.margin.right.points(),
                    border_box_height
                        + box_metrics.margin.top.points()
                        + box_metrics.margin.bottom.points(),
                    baseline_offset,
                )
            }
            None => return None,
        };
        Some(
            InlineAtom::new(
                InlineAtomContent::Svg { asset: None },
                style.clone(),
                None,
                InlineSize::new(width, height),
                baseline_offset,
                baseline_shift,
                link_target,
                None,
            )
            .with_visual_offset(visual_offset),
        )
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn collect_intrinsic_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        context: IntrinsicInlineCollectionContext<'_>,
        output: &mut Vec<InlineItem>,
    ) {
        for child in children {
            if let Some((_, _, style, _)) = child.element_parts()
                && (matches!(style.position, Position::Absolute | Position::Fixed)
                    || style.float != Float::None)
            {
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                let decoration_layers = propagated_decoration_layers_for_child(
                    &context.propagated_decoration_layers,
                    &box_.core.style,
                );
                self.collect_intrinsic_inline_box_items(
                    &box_.core.children,
                    stylesheets,
                    inherited_link.clone(),
                    context
                        .clone()
                        .with_block_style(&box_.core.style)
                        .with_propagated_decoration_layers(decoration_layers),
                    output,
                );
                if formatting_box_has_inline_content(&box_.core.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &context.propagated_decoration_layers,
                        &text_style,
                    );
                    apply_propagated_decoration_layers(&mut text_style, &decoration_layers);
                    self.push_inline_words(
                        &box_.text,
                        &text_style,
                        inherited_link.clone(),
                        context.baseline_shift,
                        context.visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &context.propagated_decoration_layers,
                        &inline_style,
                    );
                    apply_propagated_decoration_layers(&mut inline_style, &decoration_layers);
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement =
                        InlinePlacement::new(context.baseline_shift, context.visual_offset)
                            .with_added_baseline_placement(
                                self.vertical_align_baseline_shift_for_inline_style(
                                    &inline_style,
                                    context.block_style,
                                ),
                            )
                            .with_added_visual_offset(
                                self.inline_visual_offset_for_style(&inline_style),
                            );
                    let scope = self.begin_inline_element_scope(
                        box_.core.element,
                        &inline_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::BOX_INTRINSIC
                            .with_fragment_edges(box_.fragment_edges)
                            .with_preserved_empty_metrics(empty_inline_scope_has_distinct_metrics(
                                context.block_style,
                                &inline_style,
                            )),
                        output,
                    );
                    let ruby_positioning_source = (inline_style.display.is_ruby()
                        || inline_style.display.is_ruby_internal())
                    .then(|| {
                        scope
                            .positioning_containing_block_source()
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                    })
                    .flatten();
                    let inlinified_ruby_children =
                        inline_style.display.is_ruby_internal().then(|| {
                            crate::layout::ruby::inlinified_direct_children(&box_.core.children)
                        });
                    let inline_children = inlinified_ruby_children
                        .as_deref()
                        .unwrap_or(&box_.core.children);
                    if inline_style.content.is_generated() {
                        let start_len = output.len();
                        self.push_intrinsic_element_content_items_from_boxes(
                            box_.core.element,
                            &inline_style.clone(),
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            decoration_layers.clone(),
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.core.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.core.element,
                            clear,
                            output,
                            start_len,
                        );
                    } else {
                        self.collect_intrinsic_inline_box_items(
                            inline_children,
                            stylesheets,
                            link.clone(),
                            context
                                .clone()
                                .with_baseline_shift(child_placement.baseline_shift())
                                .with_visual_offset(child_placement.visual_offset)
                                .with_block_style(&inline_style.clone())
                                .with_propagated_decoration_layers(decoration_layers),
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                    // Intrinsic inline collection is also used to construct
                    // the retained item stream for inline formatting
                    // contexts. Ruby's generated empty counterparts can
                    // therefore hide an explicitly inset positioned child
                    // from the ordinary in-flow traversal. Replay it from
                    // the ruby role's completed inline scope, whose paired
                    // start/end edges define the containing block.
                    if let Some(source) = ruby_positioning_source.as_ref() {
                        self.layout_undeferred_ruby_positioned_descendants(
                            &box_.core.children,
                            stylesheets,
                            context.block_style,
                            source,
                            &[],
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(atom) = atom {
                        let atom = self.finish_inline_atom_for_parent(atom, context.block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::Table(box_)
                    if box_.core.style.display.is_inline_level() =>
                {
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        Some(&box_.fragment),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(atom) = atom {
                        let atom = self.finish_inline_atom_for_parent(atom, context.block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_intrinsic_inline_box_items(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        context
                            .clone()
                            .with_block_style(&box_.style)
                            .with_propagated_decoration_layers(
                                propagated_decoration_layers_for_child(
                                    &context.propagated_decoration_layers,
                                    &box_.style,
                                ),
                            ),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
                box_tree::FormattingBox::Table(_) => {}
            }
        }
    }
}
