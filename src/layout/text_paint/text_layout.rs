use super::*;
use crate::css::BoxDecorationBreak;
use crate::layout::inline_collect::InlinePlacement;
use crate::layout::inline_layout::InlineLayoutOutcome;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_space_width(&mut self, style: &ComputedStyle) -> LayoutLength {
        layout_pt(
            self.font_system
                .measure_text(" ", style)
                .max(style.font_size * 0.25),
        )
    }

    pub(in crate::layout) fn layout_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
    ) -> InlineLayoutOutcome {
        if self.layout_multicol_text_block(
            text,
            style,
            padding_left,
            padding_right,
            link_target,
            style.box_values.height.length_if_no_percent(),
        ) {
            return InlineLayoutOutcome::default();
        }
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let sequence = self.inline_line_sequence_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence(&sequence, style);
        sequence.layout_outcome()
    }

    pub(in crate::layout) fn layout_multicol_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        content_height: Option<f32>,
    ) -> bool {
        let available_width = self.current_content_logical_inline_size().max(1.0);
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
        let available_column_width = (column_width - padding_left - padding_right).max(1.0);
        let text = transform_text(text, style);
        let mut sequence_style = style.clone();
        sequence_style.box_decoration_break = BoxDecorationBreak::Clone;
        let sequence = self.inline_line_sequence_for_prepared_text(
            &text,
            &sequence_style,
            available_column_width,
            padding_left,
            link_target,
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
        let natural_column_height = content_height
            .or(auto_fill_max_height)
            .unwrap_or(balanced_height);
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
                .unwrap_or(balanced_height),
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll => definite_fragment_height
                .map(|limit| balanced_height.min(limit))
                .unwrap_or(balanced_height),
        };
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
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_text_block_slice(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let sequence = self.inline_line_sequence_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence_slice(&sequence, style, block_top, slice_top, slice_bottom);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_element_inline_block_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
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
            link_target,
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        );
        self.paint_inline_line_sequence_slice(&sequence, style, block_top, slice_top, slice_bottom);
    }

    pub(in crate::layout) fn inline_line_sequence_for_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let text = transform_text(text, style);
        self.inline_line_sequence_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        )
    }

    pub(in crate::layout) fn inline_line_sequence_for_raw_inline_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            link_target.map(str::to_string),
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        )
    }

    pub(in crate::layout) fn inline_line_sequence_for_prepared_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            link_target.map(str::to_string),
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        )
    }

    pub(in crate::layout) fn layout_list_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let Some(marker) = marker else {
            self.layout_text_block(text, style, padding_left, padding_right, link_target);
            return;
        };

        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let text = transform_text(text, style);
        if marker.participates_in_first_line() {
            let link_target = link_target.map(str::to_string);
            let mut items = Vec::new();
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_start(
                    style,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    &mut items,
                );
            }
            if !marker.follows_content_in_first_line() {
                self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
            }
            self.push_inline_words(
                &text,
                style,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
            if marker.follows_content_in_first_line() {
                self.push_inside_marker_items(marker, style, link_target, &mut items);
            }
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
            }
            let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                available_width,
                padding_left,
                0.0,
            );
            self.paint_inline_line_sequence(&sequence, style);
            return;
        }

        let sequence = self.inline_line_sequence_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence_with_outside_marker(
            &sequence,
            style,
            marker,
            self.content_left + padding_left,
            self.content_right - padding_right,
        );
    }
}
