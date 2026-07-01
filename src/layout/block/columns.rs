use super::super::*;

impl<'a> LayoutBuilder<'a> {
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

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            self.content_left = previous_left + (column_width + gap) * column_index as f32;
            self.content_right = self.content_left + column_width;
            self.cursor_y = column_cursors[column_index];

            for item in group {
                self.push_ancestor_signature(item.signature.clone());
                self.layout_element_with_child_boxes(
                    item.element,
                    &item.style,
                    stylesheets,
                    item.children,
                );
                self.ancestors.pop();
            }

            column_cursors[column_index] = self.cursor_y;
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = column_cursors
            .into_iter()
            .fold(previous_cursor_y, |bottom, cursor| bottom.min(cursor));
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
        let sibling_tags = element_sibling_tags(element);
        let mut element_index = 0usize;
        let mut inline_nodes = Vec::new();
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut pending_end_margin_collapse = None;
        let mut float_run = self.float_run_state();

        for child in &element.children {
            let NodeKind::Element(child_element) = &child.kind else {
                inline_nodes.push(child.clone());
                continue;
            };

            let child_signature = ElementSignature::with_siblings(
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
                if self.layout_inline_fragment_block(&inline_nodes, style, stylesheets) {
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

            if self.layout_inline_fragment_block(&inline_nodes, style, stylesheets) {
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

            self.flush_float_run(&mut float_run);
            self.push_ancestor_signature(child_signature);
            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            self.layout_element(child_element, &child_style, stylesheets);
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

        if self.layout_inline_fragment_block(&inline_nodes, style, stylesheets) {
            previous_flow_bottom_margin = None;
            self.flush_float_run(&mut float_run);
        }
        self.flush_float_run(&mut float_run);

        let _ = previous_flow_bottom_margin;
        pending_end_margin_collapse
    }
}
