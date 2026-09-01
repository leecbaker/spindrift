use super::*;

/// Whether a shaped text group owns the initial-letter pseudo itself.
///
/// The pseudo's used font metrics are intentionally isolated from its parent
/// line box, whereas ordinary companion source always uses that parent line
/// baseline: <https://drafts.csswg.org/css-inline-3/#initial-letter-position>.
fn first_inline_fragment_is_initial_letter<F: InlineFragmentAccess>(fragments: &[F]) -> bool {
    fragments
        .first()
        .is_some_and(|fragment| !fragment.style().initial_letter.is_normal())
}

/// Anchor a vertical glyph run at the same physical visual span selected by
/// its containing line, even when the run's bidi direction advances from the
/// opposite physical inline edge.
///
/// Inline line layout resolves visual order and allocates a physical span
/// using the containing block's writing-mode axes. Text painting then applies
/// the run style's own vertical advance direction. If those directions
/// differ, retaining the containing-line glyph origin makes the run advance
/// outside its allocated span. This is the sole line-to-glyph projection
/// boundary; callers retain logical inline positions until the line geometry
/// has chosen the physical span.
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
fn reposition_vertical_text_group_at_visual_inline_span(
    group: &mut PreparedInlineTextGroup,
    line_style: &ComputedStyle,
) {
    if !line_style.writing_mode.has_vertical_lines()
        || !group.style.writing_mode.has_vertical_lines()
    {
        return;
    }
    let Some(line_axis) = VerticalInlineAxis::for_style(line_style) else {
        return;
    };
    let Some(run_axis) = VerticalInlineAxis::for_style(&group.style) else {
        return;
    };
    // The two signs are +/-1. Their half-difference is exactly the signed
    // shift from the line-selected glyph edge to the run's own advance edge.
    let origin_shift = (line_axis.advance_sign() - run_axis.advance_sign()) * group.width() * 0.5;
    group.set_y(group.y() + origin_shift);
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn update_line_rendered_baseline_shift(
        line_rendered_baseline_shift: &mut Option<f32>,
        line_layout_baseline_y: f32,
        group: &PreparedInlineTextGroup,
    ) {
        if group.style.font_size == 0.0 || group.style.line_height == 0.0 {
            return;
        }
        line_rendered_baseline_shift.get_or_insert(line_layout_baseline_y - group.y());
    }

    /// Measure the final visual paint sequence used to position an inline line.
    ///
    /// A retained source-shaped slice can intentionally include the advance of
    /// a neighboring cluster when a default-ignorable join control shares its
    /// source provenance. Its paint group is re-shaped from the final visual
    /// sequence, where the control has no advance. Alignment must use that
    /// final advance rather than the source-slice fitting estimate.
    ///
    /// This mirrors paint-time text-group ownership but does not create paint
    /// operations. The caller applies the common trailing-space and tracking
    /// ownership policy afterwards.
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>
    pub(in crate::layout) fn final_painted_inline_width(
        &mut self,
        line: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
    ) -> f32 {
        let mut natural_width = 0.0f32;
        let mut inline_position = 0.0f32;
        let mut pending_fragments = Vec::new();
        let mut pending_inline_position = 0.0;
        let mut pending_preserve_leading_summary_space = false;
        let mut previous_item_was_opaque_atom = false;

        let flush_pending = |this: &mut Self,
                             pending_fragments: &mut Vec<PendingInlineFragment<'_>>,
                             pending_inline_position: f32,
                             pending_preserve_leading_summary_space: bool,
                             inline_position: &mut f32,
                             natural_width: &mut f32| {
            if let Some(group) = this.prepare_inline_text_group_with_summary_policy(
                pending_fragments,
                0.0,
                pending_preserve_leading_summary_space,
                pending_inline_position,
                block_style,
            ) {
                let end = pending_inline_position + group.width();
                *inline_position = end;
                *natural_width = (*natural_width).max(end);
            }
            pending_fragments.clear();
        };

        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let leading_tracking = measured_item.advance.boundary_before().points();
                    if leading_tracking != 0.0 && !pending_fragments.is_empty() {
                        flush_pending(
                            self,
                            &mut pending_fragments,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                            &mut inline_position,
                            &mut natural_width,
                        );
                    }
                    inline_position += leading_tracking;
                    let out_of_flow_paint_inline_advance = fragment
                        .out_of_flow_paint_inline_advance()
                        .map(|advance| advance.points());
                    let fragment = PendingInlineFragment::new(fragment);
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_preserve_leading_summary_space =
                                item_index > 0 || previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        continue;
                    }

                    let can_append = leading_tracking == 0.0
                        && pending_fragments.last().is_some_and(|previous| {
                            previous.style().text_justify == fragment.style().text_justify
                        })
                        && pending_fragments.last().is_some_and(|previous| {
                            previous.link_target() == fragment.link_target()
                        })
                        && (pending_fragments.last().is_some_and(|previous| {
                            can_queue_inline_fragments_for_shaping(previous, &fragment)
                        }) || pending_fragments.last().is_some_and(|previous| {
                            inline_fragment_can_append_collapsible_space(previous, &fragment)
                        }) || pending_inline_fragments_are_collapsible_space(
                            &pending_fragments,
                        ));
                    if pending_fragments.is_empty() {
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    } else if !can_append {
                        flush_pending(
                            self,
                            &mut pending_fragments,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                            &mut inline_position,
                            &mut natural_width,
                        );
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    }

                    if fragment.style().visibility == Visibility::Visible
                        && inline_fragment_has_visible_text_paint(&fragment)
                    {
                        pending_fragments.push(fragment);
                    } else {
                        let width = out_of_flow_paint_inline_advance
                            .unwrap_or(measured_item.base_advance().points())
                            .max(0.0);
                        inline_position += width;
                        natural_width = natural_width.max(inline_position);
                    }
                    previous_item_was_opaque_atom = false;
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content(), InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    if !inline_atom_preserves_pending_text_shaping(atom)
                        || measured_item.advance.boundary_before().points() != 0.0
                    {
                        flush_pending(
                            self,
                            &mut pending_fragments,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                            &mut inline_position,
                            &mut natural_width,
                        );
                        pending_preserve_leading_summary_space = false;
                    }
                    inline_position += measured_item.advance.boundary_before().points();

                    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.content()
                    {
                        if !inline_box_edge_is_painted_by_adjacent_fragment(
                            line,
                            item_index,
                            atom.style(),
                            *edge,
                        ) {
                            natural_width = natural_width.max(
                                inline_position
                                    + inline_box_edge_paint_offset(*edge)
                                    + edge.paint_extent,
                            );
                        }
                        inline_position += edge.advance;
                    } else {
                        inline_position += inline_atom_logical_inline_size(atom, block_style);
                    }
                    natural_width = natural_width.max(inline_position);
                    previous_item_was_opaque_atom =
                        inline_atom_content_preserves_adjacent_space_summary(atom.content());
                }
                InlineLineItem::Float(_) => {
                    flush_pending(
                        self,
                        &mut pending_fragments,
                        pending_inline_position,
                        pending_preserve_leading_summary_space,
                        &mut inline_position,
                        &mut natural_width,
                    );
                    inline_position += measured_item.used_advance().points();
                    natural_width = natural_width.max(inline_position);
                    previous_item_was_opaque_atom = false;
                }
            }
        }

        flush_pending(
            self,
            &mut pending_fragments,
            pending_inline_position,
            pending_preserve_leading_summary_space,
            &mut inline_position,
            &mut natural_width,
        );
        natural_width
    }

    pub(in crate::layout) fn natural_painted_inline_width_for_justification(
        &mut self,
        line: &[MeasuredInlineItem],
        justification_plan: &InlineJustificationPlan,
        block_style: &ComputedStyle,
    ) -> f32 {
        let mut natural_width = 0.0f32;
        let mut inline_position = 0.0f32;
        let mut pending_fragments = Vec::new();
        let mut pending_inline_position = inline_position;
        let mut pending_preserve_leading_summary_space = false;
        let mut previous_item_was_opaque_atom = false;
        // Phase II removes a terminal `pre-wrap` document-space sequence from
        // the line's fitting and alignment measure. It remains a paintable
        // source suffix, but must not reduce the extra space distributed by
        // `text-align: justify`:
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2> and
        // <https://www.w3.org/TR/css-text-3/#text-align-property>.
        let terminal_pre_wrap_hanging_start = line
            .iter()
            .rposition(|item| {
                !matches!(
                    &item.item,
                    InlineLineItem::Fragment(fragment)
                        if inline_fragment_is_pre_wrap_hanging_space(fragment)
                )
            })
            .map_or(0, |index| index + 1);

        for (item_index, measured_item) in line.iter().enumerate() {
            if item_index >= terminal_pre_wrap_hanging_start {
                continue;
            }
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let leading_tracking = measured_item.advance.boundary_before().points();
                    inline_position += leading_tracking;
                    let out_of_flow_paint_inline_advance = fragment
                        .out_of_flow_paint_inline_advance()
                        .map(|advance| advance.points());
                    let fragment = PendingInlineFragment::new(fragment);
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_preserve_leading_summary_space =
                                item_index > 0 || previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        previous_item_was_opaque_atom = false;
                        continue;
                    }

                    let width = out_of_flow_paint_inline_advance
                        .unwrap_or(measured_item.base_advance().points())
                        .max(0.0);

                    let can_append = pending_fragments
                        .last()
                        .is_some_and(|previous| previous.link_target() == fragment.link_target())
                        && (justification_plan.justifies_inter_word()
                            && pending_fragments.last().is_some_and(|previous| {
                                can_queue_inline_fragments_for_shaping(previous, &fragment)
                            }))
                        || pending_fragments.last().is_some_and(|previous| {
                            inline_fragment_can_append_collapsible_space(previous, &fragment)
                        })
                        || pending_inline_fragments_are_collapsible_space(&pending_fragments);
                    if pending_fragments.is_empty() {
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    } else if !can_append {
                        self.extend_natural_width_with_pending_text_group(
                            &pending_fragments,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                            block_style,
                            &mut natural_width,
                        );
                        pending_fragments.clear();
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    }

                    if fragment.style().visibility == Visibility::Visible
                        && inline_fragment_has_visible_text_paint(&fragment)
                    {
                        pending_fragments.push(fragment);
                    } else {
                        natural_width = natural_width.max(inline_position + width);
                    }
                    inline_position += width;
                    previous_item_was_opaque_atom = false;
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content(), InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    self.extend_natural_width_with_pending_text_group(
                        &pending_fragments,
                        pending_inline_position,
                        pending_preserve_leading_summary_space,
                        block_style,
                        &mut natural_width,
                    );
                    pending_fragments.clear();
                    pending_preserve_leading_summary_space = false;

                    inline_position += measured_item.advance.boundary_before().points();

                    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.content()
                    {
                        if !inline_box_edge_is_painted_by_adjacent_fragment(
                            line,
                            item_index,
                            atom.style(),
                            *edge,
                        ) {
                            let paint_inline_start =
                                inline_position + inline_box_edge_paint_offset(*edge);
                            natural_width =
                                natural_width.max(paint_inline_start + edge.paint_extent);
                        }
                        inline_position += edge.advance;
                        previous_item_was_opaque_atom =
                            inline_atom_content_preserves_adjacent_space_summary(atom.content());
                        continue;
                    }

                    let logical_inline_start_margin =
                        inline_atom_logical_inline_start_margin(atom, block_style);
                    let content_inline_size =
                        inline_atom_logical_border_inline_size(atom, block_style);
                    natural_width = natural_width
                        .max(inline_position + logical_inline_start_margin + content_inline_size);
                    inline_position += inline_atom_logical_inline_size(atom, block_style);
                    previous_item_was_opaque_atom =
                        inline_atom_content_preserves_adjacent_space_summary(atom.content());
                }
                InlineLineItem::Float(_) => {
                    inline_position += measured_item.used_advance().points();
                    natural_width = natural_width.max(inline_position);
                    previous_item_was_opaque_atom = false;
                }
            }
        }

        self.extend_natural_width_with_pending_text_group(
            &pending_fragments,
            pending_inline_position,
            pending_preserve_leading_summary_space,
            block_style,
            &mut natural_width,
        );
        natural_width
    }

    pub(in crate::layout) fn extend_natural_width_with_pending_text_group<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        inline_position: f32,
        preserve_leading_summary_space: bool,
        tab_metric_style: &ComputedStyle,
        natural_width: &mut f32,
    ) {
        if let Some(group) = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            preserve_leading_summary_space,
            inline_position,
            tab_metric_style,
        ) {
            *natural_width = natural_width.max(inline_position + group.width());
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn prepare_inline_text_group_at_inline_position<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        tab_metric_style: &ComputedStyle,
        line_geometry: InlineLineGeometry,
        line_baseline_y: f32,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        visual_offset: InlineVisualOffset,
        horizontal_content_bottom_y: Option<f32>,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            preserve_leading_summary_space,
            visual_inline_start + line_geometry.inline_start_offset,
            tab_metric_style,
        )?;
        group.line_block_size = line_geometry.line_block_size;
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, 0.0);
            position_horizontal_text_group_at_content_bottom(&mut group, content_bottom_y, metrics);
        } else if !first_inline_fragment_is_initial_letter(fragments) {
            // An initial letter contributes an exclusion but not its oversized
            // text metrics to the originating line box. Normal companion
            // source therefore aligns to the parent line baseline, not the
            // baseline implied by its own independently shaped text box.
            // For ordinary inline runs both baselines are identical; keeping
            // the assignment explicit makes the exceptional initial-letter
            // case share the normal text-group path.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            group.set_y(
                line_baseline_y
                    + fragments
                        .iter()
                        .find(|fragment| inline_fragment_has_visible_text_paint(*fragment))
                        .map(InlineFragmentAccess::baseline_shift)
                        .unwrap_or(0.0),
            );
        }
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
        reposition_vertical_text_group_at_visual_inline_span(&mut group, tab_metric_style);
        group.set_x(group.x() + visual_offset.x());
        group.set_y(group.y() + visual_offset.y());
        self.apply_inline_text_group_text_box_trim_link_rect(&mut group, line_geometry);
        Some(group)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn prepare_justified_inline_text_group_at_inline_position<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        tab_metric_style: &ComputedStyle,
        line_geometry: InlineLineGeometry,
        line_baseline_y: f32,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        visual_offset: InlineVisualOffset,
        horizontal_content_bottom_y: Option<f32>,
        extra_per_separator: f32,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let extra_per_separator = if fragments
            .first()
            .is_some_and(|fragment| matches!(fragment.style().text_justify, TextJustify::None))
        {
            0.0
        } else {
            extra_per_separator
        };
        let mut group = self.prepare_justified_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            extra_per_separator,
            preserve_leading_summary_space,
            tab_metric_style,
        )?;
        group.line_block_size = line_geometry.line_block_size;
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, 0.0);
            position_horizontal_text_group_at_content_bottom(&mut group, content_bottom_y, metrics);
        } else if !first_inline_fragment_is_initial_letter(fragments) {
            group.set_y(
                line_baseline_y
                    + fragments
                        .iter()
                        .find(|fragment| inline_fragment_has_visible_text_paint(*fragment))
                        .map(InlineFragmentAccess::baseline_shift)
                        .unwrap_or(0.0),
            );
        }
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
        reposition_vertical_text_group_at_visual_inline_span(&mut group, tab_metric_style);
        group.set_x(group.x() + visual_offset.x());
        group.set_y(group.y() + visual_offset.y());
        self.apply_inline_text_group_text_box_trim_link_rect(&mut group, line_geometry);
        Some(group)
    }

    fn apply_inline_text_group_text_box_trim_link_rect(
        &mut self,
        group: &mut PreparedInlineTextGroup,
        line_geometry: InlineLineGeometry,
    ) {
        let metrics = self.inline_text_box_metrics(&group.style, 0.0);
        let style_trim = self.inline_text_box_content_trim_for_style(&group.style, metrics);
        let trim = if !group.text_box_trim.is_empty() {
            group.text_box_trim
        } else if style_trim.block_start > 0.0 || style_trim.block_end > 0.0 {
            style_trim
        } else {
            line_geometry.text_box_line_trim
        };
        if trim.block_start <= 0.0 && trim.block_end <= 0.0 {
            return;
        }
        let untrimmed_rect = match line_geometry.writing_mode {
            WritingMode::HorizontalTb => {
                let content_bottom_y =
                    group.y() + metrics.content_baseline_offset - metrics.content_block_size;
                PhysicalInlineRect::new(InlineRect::new(
                    InlinePoint::new(group.x(), content_bottom_y),
                    InlineSize::new(group.width(), metrics.content_block_size),
                ))
            }
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(group.x(), group.y().min(group.y() - group.width())),
                InlineSize::new(metrics.content_block_size, group.width()),
            )),
        };
        let rect = trim_inline_content_rect(untrimmed_rect, line_geometry.writing_mode, trim);
        let rect = rect.paint_rect();
        if group.link_target.is_some() {
            group.link_paint_rect = Some(rect);
        }
        group.decoration_paint_rect = Some(rect);
    }
}
