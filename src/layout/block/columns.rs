use super::super::*;
use crate::css::Edges;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_simple_block_child_columns(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> bool {
        if is_definition_list_element(element) {
            return false;
        }
        let Some(child_boxes) = child_boxes else {
            return false;
        };
        if child_boxes.is_empty() || !simple_multicol_block_children_supported(child_boxes) {
            return false;
        }

        let available_width = (self.content_right - self.content_left).max(1.0);
        let gap = used_multicol_column_gap(style.column_gap, available_width, style.font_size);
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return false;
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let mut column_cursors = vec![previous_cursor_y; column_count];
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets =
            simple_column_text_box_trim_targets(child_boxes, column_count, text_box_line_trim);

        for (child_index, child_box) in child_boxes.iter().enumerate() {
            let column_index = child_index % column_count;
            self.content_left = previous_left + (column_width + gap) * column_index as f32;
            self.content_right = self.content_left + column_width;
            self.cursor_y = column_cursors[column_index];
            let child_text_box_line_trim =
                text_box_trim_targets.trim_for(column_index, child_index, text_box_line_trim);
            self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                layout.layout_simple_multicol_block_child(child_box, stylesheets);
            });
            column_cursors[column_index] = self.cursor_y;
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = column_cursors
            .into_iter()
            .fold(previous_cursor_y, |bottom, cursor| bottom.min(cursor));
        for primitive in multicol_gap_decoration_primitives(
            style,
            previous_left,
            previous_cursor_y,
            self.cursor_y,
            column_width,
            gap,
            column_count,
        ) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }
        true
    }

    fn layout_simple_multicol_block_child(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        match child_box {
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                self.layout_anonymous_block(&box_.style, &box_.children, stylesheets, None);
            }
            box_tree::FormattingBox::Block(box_) => {
                self.push_ancestor_signature(box_.signature.clone());
                self.layout_element_with_child_boxes_and_run_ins(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    &box_.run_in_children,
                    Some(&box_.children),
                );
                self.ancestors.pop();
            }
            box_tree::FormattingBox::InlineSplitBlockContext(context)
                if context.children.len() == 1 =>
            {
                let scope = self.begin_inline_split_block_paint_scope();
                self.layout_simple_multicol_block_child(&context.children[0], stylesheets);
                self.finish_inline_split_block_paint_scope(context, scope);
            }
            _ => {}
        }
    }

    pub(in crate::layout) fn layout_definition_list_columns(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> bool {
        if !is_definition_list_element(element) {
            return false;
        }

        let groups = child_boxes
            .map(definition_list_column_groups_from_boxes)
            .unwrap_or_else(|| {
                definition_list_column_groups_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
        if groups.is_empty() {
            return false;
        }

        let available_width = (self.content_right - self.content_left).max(1.0);
        let gap = used_multicol_column_gap(style.column_gap, available_width, style.font_size);
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return false;
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let mut column_cursors = vec![previous_cursor_y; column_count];
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets =
            definition_list_column_text_box_trim_targets(&groups, column_count, text_box_line_trim);

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            self.content_left = previous_left + (column_width + gap) * column_index as f32;
            self.content_right = self.content_left + column_width;
            self.cursor_y = column_cursors[column_index];

            for (item_index, item) in group.iter().enumerate() {
                let child_text_box_line_trim = text_box_trim_targets.trim_for(
                    column_index,
                    group_index,
                    item_index,
                    text_box_line_trim,
                );
                self.push_ancestor_signature(item.signature.clone());
                self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                    layout.layout_element_with_child_boxes(
                        item.element,
                        &item.style,
                        stylesheets,
                        item.children,
                    );
                });
                self.ancestors.pop();
            }

            column_cursors[column_index] = self.cursor_y;
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = column_cursors
            .into_iter()
            .fold(previous_cursor_y, |bottom, cursor| bottom.min(cursor));
        for primitive in multicol_gap_decoration_primitives(
            style,
            previous_left,
            previous_cursor_y,
            self.cursor_y,
            column_width,
            gap,
            column_count,
        ) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }
        true
    }

    pub(in crate::layout) fn layout_ordered_mixed_flow_children(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        can_collapse_start_margin: bool,
        can_collapse_end_margin: bool,
    ) -> Option<BlockEndMarginCollapse> {
        let sibling_tags = element_sibling_signature_list(element);
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets = self.ordered_mixed_text_box_trim_targets(
            element,
            style,
            stylesheets,
            &sibling_tags,
            text_box_line_trim,
        );
        let mut element_index = 0usize;
        let mut inline_run_index = 0usize;
        let mut inline_nodes = Vec::new();
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut pending_end_margin_collapse = None;
        let mut float_run = self.float_run_state();
        let mut first_formatted_line = FirstFormattedLineState::for_style(style);

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                inline_nodes.push(child.clone());
                continue;
            };

            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature.clone(),
                stylesheets,
                Some(style),
            );
            if child_style.float != Float::None {
                if self.layout_ordered_mixed_inline_fragment_block(
                    &inline_nodes,
                    style,
                    stylesheets,
                    &mut inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    first_formatted_line.applies_to_next_inline_run(),
                ) {
                    first_formatted_line.consume_next_formatted_line();
                    seen_flow_child = true;
                    previous_flow_bottom_margin = None;
                    self.flush_float_run(&mut float_run);
                }
                inline_nodes.clear();
                if self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    &child_style,
                    None,
                    None,
                    stylesheets,
                    &mut float_run,
                ) {
                    seen_flow_child = true;
                    previous_flow_bottom_margin = None;
                    continue;
                }
            }
            let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
                || is_document_canvas_element(element)
                || is_replaced_element(child_element);

            if !is_flow_child {
                inline_nodes.push(child.clone());
                continue;
            }

            if self.layout_ordered_mixed_inline_fragment_block(
                &inline_nodes,
                style,
                stylesheets,
                &mut inline_run_index,
                &text_box_trim_targets,
                text_box_line_trim,
                first_formatted_line.applies_to_next_inline_run(),
            ) {
                first_formatted_line.consume_next_formatted_line();
                seen_flow_child = true;
                previous_flow_bottom_margin = None;
                self.flush_float_run(&mut float_run);
            }
            inline_nodes.clear();

            let collapsible_block_child = is_collapsible_block_child(child_element, &child_style);
            let mut collapses_with_parent_end = false;
            if collapsible_block_child {
                if !seen_flow_child && can_collapse_start_margin {
                    child_style.margin.top =
                        collapsed_margin_delta(style.margin.top, child_style.margin.top);
                } else if let Some(previous_margin) = previous_flow_bottom_margin {
                    child_style.margin.top =
                        collapsed_margin_delta(previous_margin, child_style.margin.top);
                }

                collapses_with_parent_end = can_collapse_end_margin
                    && !has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
            }

            seen_flow_child = true;
            first_formatted_line.consume_next_formatted_line();

            self.flush_float_run(&mut float_run);
            self.push_ancestor_signature(child_signature);
            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            let child_text_box_line_trim = text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                text_box_line_trim,
            );
            self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                layout.layout_element(child_element, &child_style, stylesheets);
            });
            self.ancestors.pop();
            let child_consumed_bottom_margin = if child_uses_block_layout {
                self.last_block_layout_outcome.consumed_bottom_margin
            } else {
                child_style.margin.bottom
            };
            if collapses_with_parent_end {
                pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                    child_consumed_margin: child_consumed_bottom_margin,
                    collapsed_margin: collapse_margins(
                        child_consumed_bottom_margin,
                        style.margin.bottom,
                    ),
                });
            }
            previous_flow_bottom_margin =
                collapsible_block_child.then_some(child_consumed_bottom_margin);
        }

        if self.layout_ordered_mixed_inline_fragment_block(
            &inline_nodes,
            style,
            stylesheets,
            &mut inline_run_index,
            &text_box_trim_targets,
            text_box_line_trim,
            first_formatted_line.applies_to_next_inline_run(),
        ) {
            first_formatted_line.consume_next_formatted_line();
            previous_flow_bottom_margin = None;
            self.flush_float_run(&mut float_run);
        }
        self.flush_float_run(&mut float_run);

        let _ = previous_flow_bottom_margin;
        pending_end_margin_collapse
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_ordered_mixed_inline_fragment_block(
        &mut self,
        inline_nodes: &[Node],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inline_run_index: &mut usize,
        text_box_trim_targets: &OrderedMixedTextBoxTrimTargets,
        text_box_line_trim: TextBoxLineTrim,
        allow_typographic_first_line: bool,
    ) -> bool {
        let is_text_box_trim_candidate =
            ordered_mixed_inline_nodes_accept_text_box_trim(inline_nodes, style);
        let run_text_box_line_trim = if is_text_box_trim_candidate {
            text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::InlineRun(*inline_run_index),
                text_box_line_trim,
            )
        } else {
            TextBoxLineTrim::default()
        };
        let laid_out = self.with_text_box_line_trim_scope(run_text_box_line_trim, |layout| {
            layout.layout_inline_fragment_block_with_first_line_policy(
                inline_nodes,
                style,
                stylesheets,
                allow_typographic_first_line,
            )
        });
        if is_text_box_trim_candidate {
            *inline_run_index += 1;
        }
        laid_out
    }

    fn ordered_mixed_text_box_trim_targets(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        sibling_tags: &ElementSiblingSignatureList,
        trim: TextBoxLineTrim,
    ) -> OrderedMixedTextBoxTrimTargets {
        let mut targets = OrderedMixedTextBoxTrimTargets::default();
        if trim.is_empty() {
            return targets;
        }

        let mut element_index = 0usize;
        let mut inline_run_index = 0usize;
        let mut inline_nodes = Vec::new();
        let mut candidates = Vec::new();

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                inline_nodes.push(child.clone());
                continue;
            };

            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(style),
            );
            if child_style.float != Float::None {
                if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                    candidates.push(OrderedMixedTextBoxTrimCandidate {
                        target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                        accepts_block_start: true,
                        accepts_block_end: true,
                    });
                    inline_run_index += 1;
                }
                inline_nodes.clear();
                continue;
            }

            let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
                || is_document_canvas_element(element)
                || is_replaced_element(child_element);
            if !is_flow_child {
                inline_nodes.push(child.clone());
                continue;
            }

            if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                candidates.push(OrderedMixedTextBoxTrimCandidate {
                    target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                    accepts_block_start: true,
                    accepts_block_end: true,
                });
                inline_run_index += 1;
            }
            inline_nodes.clear();

            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                accepts_block_start: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    true,
                ),
                accepts_block_end: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    false,
                ),
            });
        }

        if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                accepts_block_start: true,
                accepts_block_end: true,
            });
        }

        if trim.trims_block_start {
            targets.block_start = candidates
                .first()
                .and_then(|candidate| candidate.accepts_block_start.then_some(candidate.target));
        }
        if trim.trims_block_end {
            targets.block_end = candidates
                .iter()
                .next_back()
                .and_then(|candidate| candidate.accepts_block_end.then_some(candidate.target));
        }
        targets
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderedMixedTextBoxTrimTarget {
    InlineRun(usize),
    FlowElement(usize),
}

#[derive(Debug, Clone, Copy)]
struct OrderedMixedTextBoxTrimCandidate {
    target: OrderedMixedTextBoxTrimTarget,
    accepts_block_start: bool,
    accepts_block_end: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct OrderedMixedTextBoxTrimTargets {
    block_start: Option<OrderedMixedTextBoxTrimTarget>,
    block_end: Option<OrderedMixedTextBoxTrimTarget>,
}

impl OrderedMixedTextBoxTrimTargets {
    fn trim_for(
        self,
        target: OrderedMixedTextBoxTrimTarget,
        source: TextBoxLineTrim,
    ) -> TextBoxLineTrim {
        let trims_block_start = self.block_start == Some(target);
        let trims_block_end = self.block_end == Some(target);
        TextBoxLineTrim {
            trims_block_start,
            trims_block_end,
            block_start: if trims_block_start {
                source.block_start
            } else {
                0.0
            },
            block_end: if trims_block_end {
                source.block_end
            } else {
                0.0
            },
        }
    }
}

fn ordered_mixed_inline_nodes_accept_text_box_trim(
    nodes: &[Node],
    containing_style: &ComputedStyle,
) -> bool {
    if nodes.is_empty() {
        return false;
    }
    let element = Element {
        tag: "span".to_string(),
        namespace_url: String::new(),
        document_syntax: dom::DocumentSyntax::Html,
        attrs: HashMap::new(),
        namespace_attrs: Vec::new(),
        children: nodes.to_vec(),
        is_target: false,
    };
    !inline_text_for_style(&element, containing_style).is_empty()
}

fn ordered_mixed_element_accepts_text_box_trim(
    element: &Element,
    style: &ComputedStyle,
    block_start: bool,
) -> bool {
    matches!(
        element_layout_kind(element, style),
        ElementLayoutKind::BlockFlow
    ) && definition_list_item_style_allows_text_box_trim(style, block_start)
}

#[derive(Debug, Clone)]
struct DefinitionListColumnTextBoxTrimTargets {
    block_start: Vec<Option<(usize, usize)>>,
    block_end: Vec<Option<(usize, usize)>>,
    block_start_blocked: Vec<bool>,
    block_end_blocked: Vec<bool>,
}

impl DefinitionListColumnTextBoxTrimTargets {
    fn empty(column_count: usize) -> Self {
        Self {
            block_start: vec![None; column_count],
            block_end: vec![None; column_count],
            block_start_blocked: vec![false; column_count],
            block_end_blocked: vec![false; column_count],
        }
    }

    fn trim_for(
        &self,
        column_index: usize,
        group_index: usize,
        item_index: usize,
        source: TextBoxLineTrim,
    ) -> TextBoxLineTrim {
        let trims_block_start = self.block_start.get(column_index).copied().flatten()
            == Some((group_index, item_index));
        let trims_block_end =
            self.block_end.get(column_index).copied().flatten() == Some((group_index, item_index));
        TextBoxLineTrim {
            trims_block_start,
            trims_block_end,
            block_start: if trims_block_start {
                source.block_start
            } else {
                0.0
            },
            block_end: if trims_block_end {
                source.block_end
            } else {
                0.0
            },
        }
    }
}

fn definition_list_column_text_box_trim_targets(
    groups: &[Vec<DefinitionListColumnItem<'_>>],
    column_count: usize,
    trim: TextBoxLineTrim,
) -> DefinitionListColumnTextBoxTrimTargets {
    let mut targets = DefinitionListColumnTextBoxTrimTargets::empty(column_count);
    if trim.is_empty() {
        return targets;
    }

    if trim.trims_block_start {
        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            if targets.block_start[column_index].is_some()
                || targets.block_start_blocked[column_index]
            {
                continue;
            }
            match definition_list_group_edge_text_box_trim_target(group, true, false) {
                Some((item_index, true)) => {
                    targets.block_start[column_index] = Some((group_index, item_index));
                }
                Some((_, false)) => targets.block_start_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    if trim.trims_block_end {
        for (group_index, group) in groups.iter().enumerate().rev() {
            let column_index = group_index % column_count;
            if targets.block_end[column_index].is_some() || targets.block_end_blocked[column_index]
            {
                continue;
            }
            match definition_list_group_edge_text_box_trim_target(group, false, true) {
                Some((item_index, true)) => {
                    targets.block_end[column_index] = Some((group_index, item_index));
                }
                Some((_, false)) => targets.block_end_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    targets
}

fn definition_list_group_edge_text_box_trim_target(
    group: &[DefinitionListColumnItem<'_>],
    block_start: bool,
    find_last: bool,
) -> Option<(usize, bool)> {
    if find_last {
        group.iter().enumerate().next_back().map(|(index, item)| {
            (
                index,
                definition_list_item_accepts_text_box_trim(item, block_start),
            )
        })
    } else {
        group.iter().enumerate().next().map(|(index, item)| {
            (
                index,
                definition_list_item_accepts_text_box_trim(item, block_start),
            )
        })
    }
}

fn definition_list_item_accepts_text_box_trim(
    item: &DefinitionListColumnItem<'_>,
    block_start: bool,
) -> bool {
    matches!(
        element_layout_kind(item.element, &item.style),
        ElementLayoutKind::BlockFlow
    ) && definition_list_item_style_allows_text_box_trim(&item.style, block_start)
}

fn definition_list_item_style_allows_text_box_trim(
    style: &ComputedStyle,
    block_start: bool,
) -> bool {
    let side = if block_start {
        block_start_side(style.writing_mode)
    } else {
        block_end_side(style.writing_mode)
    };
    definition_list_item_physical_edge_value(style.padding, side) <= 0.0
        && definition_list_item_physical_edge_value(used_border_widths(style), side) <= 0.0
}

fn definition_list_item_physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}

fn simple_multicol_block_children_supported(child_boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    child_boxes
        .iter()
        .all(simple_multicol_block_child_supported)
}

fn simple_multicol_block_child_supported(child_box: &box_tree::FormattingBox<'_>) -> bool {
    if !formatting_box_is_in_normal_flow(child_box)
        || formatting_box_is_zero_height_page_boundary(child_box)
    {
        return false;
    }
    match child_box {
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            simple_multicol_style_supported(&box_.style)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(context)
            if context.children.len() == 1 =>
        {
            simple_multicol_block_child_supported(&context.children[0])
        }
        box_tree::FormattingBox::Block(box_) => {
            matches!(
                element_layout_kind(box_.element, &box_.style),
                ElementLayoutKind::BlockFlow
            ) && simple_multicol_style_supported(&box_.style)
        }
        box_tree::FormattingBox::Inline(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Text(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    }
}

fn simple_multicol_style_supported(style: &ComputedStyle) -> bool {
    style.float == Float::None
        && matches!(style.position, Position::Static)
        && style.margin.top == 0.0
        && style.margin.right == 0.0
        && style.margin.bottom == 0.0
        && style.margin.left == 0.0
        && !style.break_before.is_forced()
        && !style.break_after.is_forced()
        && !style.break_inside_avoid
}

#[derive(Debug, Clone)]
struct SimpleColumnTextBoxTrimTargets {
    block_start: Vec<Option<usize>>,
    block_end: Vec<Option<usize>>,
    block_start_blocked: Vec<bool>,
    block_end_blocked: Vec<bool>,
}

impl SimpleColumnTextBoxTrimTargets {
    fn empty(column_count: usize) -> Self {
        Self {
            block_start: vec![None; column_count],
            block_end: vec![None; column_count],
            block_start_blocked: vec![false; column_count],
            block_end_blocked: vec![false; column_count],
        }
    }

    fn trim_for(
        &self,
        column_index: usize,
        child_index: usize,
        source: TextBoxLineTrim,
    ) -> TextBoxLineTrim {
        let trims_block_start =
            self.block_start.get(column_index).copied().flatten() == Some(child_index);
        let trims_block_end =
            self.block_end.get(column_index).copied().flatten() == Some(child_index);
        TextBoxLineTrim {
            trims_block_start,
            trims_block_end,
            block_start: if trims_block_start {
                source.block_start
            } else {
                0.0
            },
            block_end: if trims_block_end {
                source.block_end
            } else {
                0.0
            },
        }
    }
}

fn simple_column_text_box_trim_targets(
    child_boxes: &[box_tree::FormattingBox<'_>],
    column_count: usize,
    trim: TextBoxLineTrim,
) -> SimpleColumnTextBoxTrimTargets {
    let mut targets = SimpleColumnTextBoxTrimTargets::empty(column_count);
    if trim.is_empty() {
        return targets;
    }

    if trim.trims_block_start {
        for (child_index, child_box) in child_boxes.iter().enumerate() {
            let column_index = child_index % column_count;
            if targets.block_start[column_index].is_some()
                || targets.block_start_blocked[column_index]
            {
                continue;
            }
            match simple_multicol_child_edge_text_box_trim_target(child_box, true) {
                Some(true) => targets.block_start[column_index] = Some(child_index),
                Some(false) => targets.block_start_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    if trim.trims_block_end {
        for (child_index, child_box) in child_boxes.iter().enumerate().rev() {
            let column_index = child_index % column_count;
            if targets.block_end[column_index].is_some() || targets.block_end_blocked[column_index]
            {
                continue;
            }
            match simple_multicol_child_edge_text_box_trim_target(child_box, false) {
                Some(true) => targets.block_end[column_index] = Some(child_index),
                Some(false) => targets.block_end_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    targets
}

fn simple_multicol_child_edge_text_box_trim_target(
    child_box: &box_tree::FormattingBox<'_>,
    block_start: bool,
) -> Option<bool> {
    if !formatting_box_is_in_normal_flow(child_box)
        || formatting_box_is_zero_height_page_boundary(child_box)
    {
        return None;
    }
    match child_box {
        box_tree::FormattingBox::AnonymousBlock(box_) => Some(
            simple_multicol_style_allows_text_box_trim(&box_.style, block_start),
        ),
        box_tree::FormattingBox::InlineSplitBlockContext(context)
            if context.children.len() == 1 =>
        {
            simple_multicol_child_edge_text_box_trim_target(&context.children[0], block_start)
        }
        box_tree::FormattingBox::Block(box_) => Some(
            matches!(
                element_layout_kind(box_.element, &box_.style),
                ElementLayoutKind::BlockFlow
            ) && simple_multicol_style_allows_text_box_trim(&box_.style, block_start),
        ),
        box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => Some(false),
        box_tree::FormattingBox::Inline(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Text(_) => None,
    }
}

fn simple_multicol_style_allows_text_box_trim(style: &ComputedStyle, block_start: bool) -> bool {
    let side = if block_start {
        block_start_side(style.writing_mode)
    } else {
        block_end_side(style.writing_mode)
    };
    simple_multicol_physical_edge_value(style.padding, side) <= 0.0
        && simple_multicol_physical_edge_value(used_border_widths(style), side) <= 0.0
}

fn simple_multicol_physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}
