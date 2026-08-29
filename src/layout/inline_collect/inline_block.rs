use super::scopes::mark_inline_text_items_as_run_in;
use super::*;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_multicol_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
        // A frozen formatting child stream already contains tree-abiding
        // generated pseudo boxes. Recollecting `::before` here would emit it
        // twice after block-in-inline normalization and, in particular,
        // duplicate its post-list-item counter snapshot.
        // <https://drafts.csswg.org/css-pseudo-4/#generated-content>
        if child_boxes.is_none() {
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
        }
        if let Some(child_boxes) = child_boxes {
            if style.content.is_generated() {
                // A tree-abiding generated pseudo arrives with an empty
                // frozen child list, but its own `content` still forms the
                // item's inline contents.  The frozen path must therefore
                // evaluate that property instead of treating the empty list
                // as an empty line.
                // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
                self.push_element_content_items_from_boxes(
                    element,
                    style,
                    box_tree::CounterEventSource::Principal,
                    child_boxes,
                    stylesheets,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration_origins.effective_layers_vec(),
                    &mut items,
                );
            } else {
                self.collect_inline_box_items(
                    child_boxes,
                    stylesheets,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration_origins.effective_layers_vec(),
                    &mut items,
                );
            }
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
        if child_boxes.is_none() {
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
        let sequence_style = style
            .used_style()
            .clone()
            .map_used_values(|style| style.box_decoration_break = css::BoxDecorationBreak::Clone);
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            &sequence_style,
            available_column_width,
            padding_left,
            0.0,
        );
        let plan = self.plan_multicolumn_inline_layout(
            &sequence,
            style,
            column_count,
            gap,
            column_width,
            available_width,
            content_height,
        );
        self.paint_inline_line_sequence_multicolumn(&sequence, style, plan);
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The block formatting context, frozen child stream, and list-marker state are independent layout inputs."
    )]
    pub(in crate::layout) fn layout_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        padding: (f32, f32),
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) -> Option<inline_layout::InlineLineSequence> {
        let (padding_left, padding_right) = padding;
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if let Some(marker) = marker
            && marker.paints_outside()
            && !self.outside_marker_anchor_is_pending(marker)
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            let anchor = self.outside_marker_fallback_anchor(
                style,
                PageInlineSpan::from_edges(
                    self.content_left + padding_left,
                    self.content_right - padding_right,
                ),
            );
            self.paint_outside_marker(marker, style, anchor);
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
        // Frozen child boxes already contain tree-abiding pseudos. This
        // direct inline collector is also used after block-in-inline
        // normalization, where a second originating-style collection would
        // duplicate `::before` and its planned counter value.
        // <https://drafts.csswg.org/css-pseudo-4/#generated-content>
        if child_boxes.is_none() {
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
        }
        if let Some(child_boxes) = child_boxes {
            if style.content.is_generated() {
                // Tree-abiding generated pseudos have an empty frozen child
                // list, but their own `content` still forms this item's
                // inline contents.
                // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
                self.push_element_content_items_from_boxes(
                    element,
                    style,
                    box_tree::CounterEventSource::Principal,
                    child_boxes,
                    stylesheets,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration_origins.effective_layers_vec(),
                    &mut items,
                );
            } else {
                self.collect_inline_box_items(
                    child_boxes,
                    stylesheets,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    style,
                    style.text_decoration_origins.effective_layers_vec(),
                    &mut items,
                );
            }
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
        if child_boxes.is_none() {
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
        }
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
            return None;
        }
        // A simple vertical inline stream can supply both final paint and its
        // enclosing block's logical-inline extent. Retain that committed
        // sequence so final block geometry does not recollect and reselect
        // the same text after orthogonal auto sizing has chosen its measure.
        // <https://drafts.csswg.org/css-lists-3/#marker-position>
        // <https://drafts.csswg.org/css-writing-modes-4/#vertical-layout>
        if matches!(
            style.writing_mode,
            WritingMode::VerticalRl | WritingMode::VerticalLr
        ) && items
            .iter()
            .all(|item| matches!(item, InlineItem::Word(_) | InlineItem::Atom(_)))
            && let Some(sequence) = self.try_layout_committed_vertical_inline_sequence(
                &mut items,
                style,
                available_width,
                padding_left,
                0.0,
                stylesheets,
            )
        {
            return Some(sequence);
        }
        let _ = self.layout_inline_items(
            items,
            style,
            available_width,
            padding_left,
            0.0,
            stylesheets,
        );
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_run_in_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
            && !self.outside_marker_anchor_is_pending(marker)
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            let anchor = self.outside_marker_fallback_anchor(
                style,
                PageInlineSpan::from_edges(self.content_left, self.content_right),
            );
            self.paint_outside_marker(marker, style, anchor);
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
            style.text_decoration_origins.effective_layers_vec(),
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
                style.text_decoration_origins.effective_layers_vec(),
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
                style.text_decoration_origins.effective_layers_vec(),
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
}
