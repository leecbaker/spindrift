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
        stylesheets: &Stylesheets<'_>,
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
