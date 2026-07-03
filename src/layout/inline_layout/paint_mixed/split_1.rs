use super::*;
use std::rc::Rc;

impl<'a> LayoutBuilder<'a> {
    /// Prepare one inline line fragment for painting.
    ///
    /// CSS Inline first resolves the line box and positions inline-level
    /// fragments within it; CSS Text shaping then produces glyph runs for
    /// eligible adjacent text fragments. This function records those used
    /// positions and shaped groups before any page paint operation is emitted:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
    pub(in crate::layout) fn prepare_inline_line_fragment(
        &mut self,
        line_fragment: &InlineLineFragment,
        context: InlinePaintContext<'_>,
    ) -> Option<PreparedInlineLine> {
        let block_style = context.block_style;
        let line_metrics = line_fragment.metrics;
        let should_justify_line = context.text_align.justifies()
            && !matches!(block_style.text_justify, TextJustify::None);
        let split_for_inter_character =
            should_justify_line && matches!(block_style.text_justify, TextJustify::InterCharacter);
        let apply_typographic_pseudos = context.is_first_line
            && (block_style.first_line_style.is_some() || block_style.first_letter_style.is_some());
        let line = if apply_typographic_pseudos {
            let mut source_items = measured_inline_items(line_fragment.items());
            apply_first_line_pseudos_to_line_items(&mut source_items, block_style);
            let source_items = if split_for_inter_character {
                split_mixed_line_into_inter_character_units(&source_items)
            } else {
                source_items
            };
            source_items
                .into_iter()
                .map(|item| {
                    let shaped = match &item {
                        InlineLineItem::Fragment(fragment) => {
                            self.font_system.shape_unwrapped_line(
                                fragment.text(),
                                fragment.style(),
                                fragment.style().line_height,
                            )
                        }
                        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                    };
                    let width = match &item {
                        InlineLineItem::Fragment(_) => shaped
                            .as_ref()
                            .map(ShapedInlineLine::advance_width)
                            .unwrap_or(0.0),
                        InlineLineItem::Atom(atom) => {
                            inline_atom_logical_inline_size(atom, block_style)
                        }
                        InlineLineItem::Float(_) => 0.0,
                    };
                    let shaped = shaped.map(Rc::new);
                    MeasuredInlineItem {
                        item,
                        width,
                        shaped,
                    }
                })
                .collect::<Vec<_>>()
        } else if split_for_inter_character {
            split_mixed_line_into_inter_character_units(&measured_inline_items(
                line_fragment.items(),
            ))
            .into_iter()
            .map(|item| {
                let shaped = match &item {
                    InlineLineItem::Fragment(fragment) => self.font_system.shape_unwrapped_line(
                        fragment.text(),
                        fragment.style(),
                        fragment.style().line_height,
                    ),
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                };
                let width = match &item {
                    InlineLineItem::Fragment(_) => shaped
                        .as_ref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or(0.0),
                    InlineLineItem::Atom(atom) => {
                        inline_atom_logical_inline_size(atom, block_style)
                    }
                    InlineLineItem::Float(_) => 0.0,
                };
                let shaped = shaped.map(Rc::new);
                MeasuredInlineItem {
                    item,
                    width,
                    shaped,
                }
            })
            .collect::<Vec<_>>()
        } else {
            line_fragment.items().to_vec()
        };
        let line_items = measured_inline_items(&line);
        let line_geometry = InlineLineGeometry::new(self.content_left, self.cursor_y, context);
        let line_alignment_width = (line_metrics.width
            - visual_leading_inline_end_box_edge_width(&line, line_geometry))
        .max(0.0);
        let hanging_widths = line_fragment.hanging_widths;
        let line_available_width = line_geometry.inline_size;
        let line_align = context.text_align;
        let line_baseline_offset = line_metrics.baseline_offset;
        let parent_fragment_metrics = self.inline_text_box_metrics(block_style, None, 0.0);
        let justification_plan = InlineJustificationPlan::for_line(
            &line_items,
            block_style.text_justify,
            should_justify_line,
        );
        let justification_line_width = if should_justify_line {
            let natural_width = self
                .natural_painted_inline_width_for_justification(
                    &line,
                    &justification_plan,
                    block_style,
                )
                .max(0.0);
            if natural_width > 0.0 {
                natural_width
            } else {
                line_metrics.width
            }
        } else {
            line_metrics.width
        };
        let extra_space_width =
            justification_plan.extra_space_width(justification_line_width, line_available_width);
        let mut line_logical_inline_start = line_geometry.alignment_offset(
            line_alignment_width,
            if should_justify_line {
                TextAlign::Left
            } else {
                line_align
            },
        );
        line_logical_inline_start += line_geometry.hanging_punctuation_offset(hanging_widths);
        let line_physical_origin =
            line_geometry.visual_line_origin(line_logical_inline_start, line_alignment_width);
        let mut inline_position = 0.0;
        let mut pending_fragments = Vec::new();
        let mut pending_inline_position = inline_position;
        let mut pending_visual_offset = InlineVisualOffset::zero();
        let mut pending_preserve_leading_summary_space = false;
        let mut pending_horizontal_content_bottom_y = None;
        let mut previous_item_was_opaque_atom = false;
        let mut paint_items = Vec::new();
        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let mut fragment = PendingInlineFragment::new(fragment);
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_visual_offset = fragment.visual_offset();
                            pending_preserve_leading_summary_space =
                                item_index > 0 || previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        previous_item_was_opaque_atom = false;
                        continue;
                    }
                    let fragment_metrics = self.inline_text_box_metrics(
                        fragment.style(),
                        measured_item.shaped.as_deref(),
                        fragment.baseline_shift(),
                    );
                    let fragment_baseline_offset = fragment_metrics.line_baseline_offset;
                    let fragment_content_block_size = fragment_metrics.content_block_size;
                    let fragment_content_trim = self
                        .inline_text_box_content_trim_for_style(fragment.style(), fragment_metrics);
                    let fragment_edge_content_bottom_y =
                        inline_fragment_horizontal_content_bottom_y(
                            &fragment,
                            self.cursor_y,
                            line_metrics.height,
                            line_baseline_offset,
                            fragment_metrics,
                            parent_fragment_metrics,
                        );
                    let fragment_background_y = fragment_edge_content_bottom_y.unwrap_or({
                        self.cursor_y - line_baseline_offset
                            + fragment.baseline_shift()
                            + fragment_metrics.content_baseline_offset
                            - fragment_content_block_size
                    });
                    if !fragment
                        .style()
                        .vertical_align
                        .has_line_relative_baseline_shift()
                        && fragment_edge_content_bottom_y.is_none()
                    {
                        let natural_baseline_offset =
                            fragment_baseline_offset + fragment.baseline_shift();
                        fragment.add_baseline_shift(natural_baseline_offset - line_baseline_offset);
                    }
                    let mut width = measured_item
                        .shaped
                        .as_deref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or(measured_item.width);
                    if item_index + 1 == line.len() {
                        width = (width
                            - trailing_letter_spacing_width_for_line_items(std::slice::from_ref(
                                &measured_item.item,
                            )))
                        .max(0.0);
                    }
                    let fragment_expansion_count =
                        justification_plan.expansion_count_after_item(item_index);
                    let fragment_rect = trim_inline_content_rect(
                        line_geometry.visual_line_item_rect(
                            line_logical_inline_start,
                            line_physical_origin,
                            inline_position,
                            width + extra_space_width * fragment_expansion_count as f32,
                            fragment_background_y,
                            fragment_content_block_size,
                        ),
                        block_style.writing_mode,
                        fragment_content_trim,
                    )
                    .translated(fragment.visual_offset());
                    for decoration in fragment.ancestor_inline_decorations() {
                        let decoration_metrics = self.inline_text_box_metrics(
                            &decoration.style,
                            measured_item.shaped.as_deref(),
                            fragment.baseline_shift(),
                        );
                        let decoration_trim = self.inline_text_box_content_trim_for_style(
                            &decoration.style,
                            decoration_metrics,
                        );
                        let decoration_rect = trim_inline_content_rect(
                            line_geometry.visual_line_item_rect(
                                line_logical_inline_start,
                                line_physical_origin,
                                inline_position,
                                width + extra_space_width * fragment_expansion_count as f32,
                                fragment_background_y,
                                fragment_content_block_size,
                            ),
                            block_style.writing_mode,
                            decoration_trim,
                        )
                        .translated(fragment.visual_offset());
                        paint_items.push(PreparedInlinePaintItem::FragmentBackground(
                            PreparedInlineFragment {
                                fragment: InlineFragment::new(
                                    fragment.text(),
                                    decoration.style.clone(),
                                    fragment.baseline_shift(),
                                    fragment.link_target().map(ToOwned::to_owned),
                                    false,
                                    fragment.source(),
                                    false,
                                    decoration.hanging_edges,
                                    Vec::new(),
                                ),
                                rect: decoration_rect,
                            },
                        ));
                    }
                    paint_items.push(PreparedInlinePaintItem::FragmentBackground(
                        PreparedInlineFragment {
                            fragment: fragment.to_owned_fragment(),
                            rect: fragment_rect,
                        },
                    ));
                    let can_append = ((extra_space_width == 0.0
                        || justification_plan.justifies_inter_word())
                        && pending_fragments.last().is_some_and(|previous| {
                            can_queue_inline_fragments_for_shaping(previous, &fragment)
                        }))
                        || pending_fragments.last().is_some_and(|previous| {
                            inline_fragment_can_append_collapsible_space(previous, &fragment)
                        })
                        || pending_inline_fragments_are_collapsible_space(&pending_fragments);
                    if pending_fragments.is_empty() {
                        pending_inline_position = inline_position;
                        pending_visual_offset = fragment.visual_offset();
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    } else if !can_append {
                        if let Some(group) = if justification_plan.justifies_inter_word() {
                            self.prepare_justified_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                extra_space_width,
                                pending_preserve_leading_summary_space,
                            )
                        } else {
                            self.prepare_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            )
                        } {
                            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                        }
                        pending_fragments.clear();
                        pending_horizontal_content_bottom_y = None;
                        pending_inline_position = inline_position;
                        pending_visual_offset = fragment.visual_offset();
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    }
                    if pending_inline_fragments_are_join_control_only(&pending_fragments) {
                        pending_visual_offset = fragment.visual_offset();
                    }
                    if fragment.style().visibility == Visibility::Visible
                        && inline_fragment_has_visible_text_paint(&fragment)
                    {
                        if pending_horizontal_content_bottom_y.is_none() {
                            pending_horizontal_content_bottom_y = fragment_edge_content_bottom_y;
                        }
                        pending_fragments.push(fragment);
                    }
                    inline_position += width;
                    let add_inter_character_gap = justification_plan.justifies_inter_character()
                        && fragment_expansion_count > 0;
                    if extra_space_width > 0.0
                        && (add_inter_character_gap || fragment_expansion_count > 0)
                    {
                        if add_inter_character_gap {
                            if let Some(group) = self.prepare_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            ) {
                                paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                            }
                            pending_fragments.clear();
                            pending_horizontal_content_bottom_y = None;
                            pending_visual_offset = InlineVisualOffset::zero();
                            pending_preserve_leading_summary_space = false;
                        }
                        inline_position += if add_inter_character_gap {
                            extra_space_width
                        } else {
                            extra_space_width * fragment_expansion_count as f32
                        };
                    }
                    previous_item_was_opaque_atom = false;
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content(), InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    if let Some(group) = if justification_plan.justifies_inter_word() {
                        self.prepare_justified_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_visual_offset,
                            pending_horizontal_content_bottom_y,
                            extra_space_width,
                            pending_preserve_leading_summary_space,
                        )
                    } else {
                        self.prepare_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_visual_offset,
                            pending_horizontal_content_bottom_y,
                            pending_preserve_leading_summary_space,
                        )
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    pending_horizontal_content_bottom_y = None;
                    pending_visual_offset = InlineVisualOffset::zero();
                    pending_preserve_leading_summary_space = false;
                    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.content()
                    {
                        let atom_metrics =
                            self.inline_text_box_metrics(atom.style(), None, atom.baseline_shift);
                        let content_block_size = atom_metrics.content_block_size;
                        let content_trim =
                            self.inline_text_box_content_trim_for_style(atom.style(), atom_metrics);
                        let y = inline_edge_horizontal_content_y(
                            atom,
                            self.cursor_y,
                            line_metrics.height,
                            line_baseline_offset,
                            atom_metrics,
                            parent_fragment_metrics,
                        );
                        if !inline_box_edge_is_painted_by_adjacent_fragment(
                            &line,
                            item_index,
                            atom.style(),
                            *edge,
                        ) {
                            let paint_inline_start =
                                inline_position + inline_box_edge_paint_offset(*edge);
                            let content_rect = trim_inline_content_rect(
                                line_geometry.visual_line_item_rect(
                                    line_logical_inline_start,
                                    line_physical_origin,
                                    paint_inline_start,
                                    edge.paint_extent,
                                    y,
                                    content_block_size,
                                ),
                                block_style.writing_mode,
                                content_trim,
                            )
                            .translated(atom.visual_offset);
                            paint_items.push(PreparedInlinePaintItem::Atom(PreparedInlineAtom {
                                atom: atom.clone(),
                                content_rect,
                            }));
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
                    let content_block_size =
                        inline_atom_logical_border_block_size(atom, block_style);
                    // CSS 2.2 inline formatting treats inline-block/replaced
                    // boxes as atomic inline-level margin boxes; the border
                    // box is painted inside the atom's logical margins.
                    let y = inline_atom_horizontal_content_y(
                        atom,
                        block_style,
                        self.cursor_y,
                        line_metrics.height,
                        line_baseline_offset,
                        content_block_size,
                        parent_fragment_metrics,
                    );
                    let content_rect = line_geometry
                        .visual_line_item_rect(
                            line_logical_inline_start,
                            line_physical_origin,
                            inline_position + logical_inline_start_margin,
                            content_inline_size,
                            y,
                            content_block_size,
                        )
                        .translated(atom.visual_offset);
                    paint_items.push(PreparedInlinePaintItem::Atom(PreparedInlineAtom {
                        atom: atom.clone(),
                        content_rect,
                    }));
                    inline_position += inline_atom_logical_inline_size(atom, block_style);
                    previous_item_was_opaque_atom =
                        inline_atom_content_preserves_adjacent_space_summary(atom.content());
                }
                InlineLineItem::Float(_) => {
                    if let Some(group) = if justification_plan.justifies_inter_word() {
                        self.prepare_justified_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_visual_offset,
                            pending_horizontal_content_bottom_y,
                            extra_space_width,
                            pending_preserve_leading_summary_space,
                        )
                    } else {
                        self.prepare_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_visual_offset,
                            pending_horizontal_content_bottom_y,
                            pending_preserve_leading_summary_space,
                        )
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    pending_horizontal_content_bottom_y = None;
                    pending_visual_offset = InlineVisualOffset::zero();
                    pending_preserve_leading_summary_space = false;
                    inline_position += measured_item.width;
                    previous_item_was_opaque_atom = false;
                }
            }
        }
        if let Some(group) = if justification_plan.justifies_inter_word() {
            self.prepare_justified_inline_text_group_at_inline_position(
                &pending_fragments,
                line_geometry,
                line_logical_inline_start,
                line_physical_origin,
                pending_inline_position,
                pending_visual_offset,
                pending_horizontal_content_bottom_y,
                extra_space_width,
                pending_preserve_leading_summary_space,
            )
        } else {
            self.prepare_inline_text_group_at_inline_position(
                &pending_fragments,
                line_geometry,
                line_logical_inline_start,
                line_physical_origin,
                pending_inline_position,
                pending_visual_offset,
                pending_horizontal_content_bottom_y,
                pending_preserve_leading_summary_space,
            )
        } {
            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
        }
        Some(PreparedInlineLine {
            metrics: line_metrics,
            paint_items,
        })
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

        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
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

                    let mut width = measured_item
                        .shaped
                        .as_deref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or(measured_item.width);
                    if item_index + 1 == line.len() {
                        width = (width
                            - trailing_letter_spacing_width_for_line_items(std::slice::from_ref(
                                &measured_item.item,
                            )))
                        .max(0.0);
                    }

                    let can_append = (justification_plan.justifies_inter_word()
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
                        &mut natural_width,
                    );
                    pending_fragments.clear();
                    pending_preserve_leading_summary_space = false;

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
                    self.extend_natural_width_with_pending_text_group(
                        &pending_fragments,
                        pending_inline_position,
                        pending_preserve_leading_summary_space,
                        &mut natural_width,
                    );
                    pending_fragments.clear();
                    pending_preserve_leading_summary_space = false;
                    inline_position += measured_item.width;
                    natural_width = natural_width.max(inline_position);
                    previous_item_was_opaque_atom = false;
                }
            }
        }

        self.extend_natural_width_with_pending_text_group(
            &pending_fragments,
            pending_inline_position,
            pending_preserve_leading_summary_space,
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
        natural_width: &mut f32,
    ) {
        if let Some(group) = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            preserve_leading_summary_space,
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
        line_geometry: InlineLineGeometry,
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
        )?;
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, Some(&group.shaped), 0.0);
            position_horizontal_text_group_at_content_bottom(&mut group, content_bottom_y, metrics);
        }
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
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
        line_geometry: InlineLineGeometry,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        visual_offset: InlineVisualOffset,
        horizontal_content_bottom_y: Option<f32>,
        extra_per_separator: f32,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_justified_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            extra_per_separator,
            preserve_leading_summary_space,
        )?;
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, Some(&group.shaped), 0.0);
            position_horizontal_text_group_at_content_bottom(&mut group, content_bottom_y, metrics);
        }
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
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
        let metrics = self.inline_text_box_metrics(&group.style, Some(&group.shaped), 0.0);
        let trim = self.inline_text_box_content_trim_for_style(&group.style, metrics);
        if trim.block_start <= 0.0 && trim.block_end <= 0.0 {
            return;
        }
        let untrimmed_rect = match line_geometry.writing_mode {
            WritingMode::HorizontalTb => {
                let content_bottom_y =
                    group.y() + metrics.content_baseline_offset - metrics.content_block_size;
                PhysicalInlineRect::new(
                    group.x(),
                    content_bottom_y,
                    group.width(),
                    metrics.content_block_size,
                )
            }
            WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalInlineRect::new(
                group.x(),
                group.y().min(group.y() - group.width()),
                metrics.content_block_size,
                group.width(),
            ),
        };
        let rect = trim_inline_content_rect(untrimmed_rect, line_geometry.writing_mode, trim);
        let rect = rect.paint_rect();
        if group.link_target.is_some() {
            group.link_paint_rect = Some(rect);
        }
        group.decoration_paint_rect = Some(rect);
    }

    /// Paint a prepared inline line without reshaping text.
    ///
    /// PDF text and CSS decoration emission consume the shaped glyph runs
    /// stored during line preparation, keeping fallback fonts and glyph
    /// advances stable after line fitting:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(in crate::layout) fn paint_prepared_inline_line(&mut self, line: &PreparedInlineLine) {
        self.paint_prepared_inline_line_with_text_source(line, None);
    }

    pub(in crate::layout) fn paint_prepared_inline_line_with_text_source(
        &mut self,
        line: &PreparedInlineLine,
        text_source: Option<RenderedLineSource>,
    ) {
        debug_assert!(line.metrics.height.is_finite());
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.rect.x(),
                        fragment.rect.y(),
                        fragment.rect.width(),
                        fragment.rect.height(),
                    );
                }
                PreparedInlinePaintItem::TextGroup(group) => {
                    if let Some(source) = text_source {
                        self.paint_prepared_inline_text_group_with_source(group, source);
                    } else {
                        self.paint_prepared_inline_text_group(group);
                    }
                }
                PreparedInlinePaintItem::Atom(atom) => {
                    self.paint_prepared_inline_atom(atom);
                }
            }
        }
    }

    /// Paint one prepared atomic inline box.
    ///
    /// CSS Inline treats replaced and inline-block descendants as atomic
    /// inline-level boxes. The prepared atom stores the resolved content box so
    /// painting does not recompute line positioning:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
    pub(in crate::layout) fn paint_prepared_inline_atom(&mut self, prepared: &PreparedInlineAtom) {
        let atom = &prepared.atom;
        if atom.style().visibility != Visibility::Visible {
            return;
        }
        if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() {
            self.paint_prepared_inline_box_edge(prepared, *edge);
            return;
        }
        if matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) {
            return;
        }
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_prepared_inline_atom_contents(prepared);
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if !fragment.is_empty() {
            let bounds = prepared.content_rect.paint_clip();
            let policy = StackingContextPolicy::for_atomic(atom.style(), PaintBand::Inline, bounds);
            let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                .with_source_order(self.next_paint_source_order())
                .with_effects(policy.effects)
                .with_bounds(bounds);
            let fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            self.current_page
                .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
        }
        if let Some(layers) = atom.escaped_positioned_layers() {
            for layer in layers.iter() {
                let atom_offset = layer
                    .escaped_atom_translation
                    .atom_offset(prepared.content_rect.x(), prepared.content_rect.y());
                let mut layer = layer.clone().translated(atom_offset);
                layer.page_index = self.pages.len();
                self.positioned_layers.push(layer);
            }
        }
    }

    pub(in crate::layout) fn paint_prepared_inline_atom_contents(
        &mut self,
        prepared: &PreparedInlineAtom,
    ) {
        let atom = &prepared.atom;
        let content_x = prepared.content_rect.x();
        let y = prepared.content_rect.y();
        let content_width = prepared.content_rect.width();
        let content_height = prepared.content_rect.height();
        match atom.content() {
            InlineAtomContent::InlineEdge(_)
            | InlineAtomContent::Leader(_)
            | InlineAtomContent::StaticPositionPlaceholder => {}
            InlineAtomContent::Canvas => {
                if atom.style().background_color.is_some() || used_border_width(atom.style()) > 0.0
                {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, atom.style());
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::BackgroundBorder, path);
                    }
                    for stroke in strokes {
                        self.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
                    }
                }
            }
            InlineAtomContent::Image(decoded) => {
                if atom.style().background_color.is_some() || used_border_width(atom.style()) > 0.0
                {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, atom.style());
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::BackgroundBorder, path);
                    }
                    self.extend_strokes_in_band(PaintBand::BackgroundBorder, strokes);
                }
                let borders = used_border_widths(atom.style());
                let image_x = content_x + borders.left + atom.style().padding.left;
                let image_y = y + borders.bottom + atom.style().padding.bottom;
                let image_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(0.0);
                let image_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style().padding.top
                    - atom.style().padding.bottom)
                    .max(0.0);
                self.push_image_in_band(
                    PaintBand::Inline,
                    RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, image_width, image_height),
                        false,
                        decoded.pixel_width,
                        decoded.pixel_height,
                        None,
                        false,
                        decoded.rgb.clone(),
                        decoded.alpha.clone(),
                        atom.alt_text().map(ToOwned::to_owned),
                    ),
                );
            }
            InlineAtomContent::Svg { fill } => {
                self.push_rect_in_band(
                    PaintBand::Inline,
                    RenderedRect::from_paint_rect(
                        paint_space_rect(content_x, y, content_width, content_height),
                        Some(*fill),
                    ),
                );
            }
            InlineAtomContent::InlineBox { sequence } => {
                if atom.style().background_color.is_some() || used_border_width(atom.style()) > 0.0
                {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, atom.style());
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::BackgroundBorder, path);
                    }
                    self.extend_strokes_in_band(PaintBand::BackgroundBorder, strokes);
                }
                let borders = used_border_widths(atom.style());
                let text_top = y + content_height - borders.top - atom.style().padding.top;
                let text_x = content_x + borders.left + atom.style().padding.left;
                let text_available_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(1.0);
                self.paint_inline_box_sequence(
                    sequence,
                    atom.style(),
                    text_x,
                    text_available_width,
                    text_top,
                );
            }
            InlineAtomContent::InlineFragment(fragment) => {
                if atom.style().background_color.is_some() || used_border_width(atom.style()) > 0.0
                {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, atom.style());
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::BackgroundBorder, path);
                    }
                    self.extend_strokes_in_band(PaintBand::BackgroundBorder, strokes);
                }
                self.current_page
                    .append_paint_fragment(fragment, PaintVector::new(content_x, y));
            }
        }
        for primitive in
            self.box_outline_primitives(content_x, y, content_width, content_height, atom.style())
        {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
        if let Some(target) = atom.link_target() {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                paint_space_rect(content_x, y, content_width, content_height),
                target.to_string(),
            ));
        }
    }

    pub(in crate::layout) fn paint_inline_box_sequence(
        &mut self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
    ) {
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        self.content_left = content_left;
        self.content_right = content_left + available_width;
        self.cursor_y = block_top;
        self.paint_inline_line_sequence_slice_with_text_source(
            sequence,
            style,
            block_top,
            block_top,
            f32::NEG_INFINITY,
            RenderedLineSource::InlineAtom,
        );
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
    }

    /// Paint the owned decoration of a split inline box edge.
    ///
    /// CSS margins affect layout advance, but backgrounds, borders, and padding
    /// paint over the border/padding area. Keeping this separate for box-edge
    /// atoms preserves negative-margin behavior without clipping the border:
    /// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>.
    pub(in crate::layout) fn paint_prepared_inline_box_edge(
        &mut self,
        prepared: &PreparedInlineAtom,
        edge: InlineBoxEdgeFragment,
    ) {
        if edge.paint_extent <= 0.0 || prepared.content_rect.height() <= 0.0 {
            return;
        }
        let mut style = prepared.atom.style().clone();
        apply_inline_box_edge_paint_style(&mut style, edge);
        if style.background_color.is_none()
            && style.background_image.is_none()
            && used_border_width(&style) == 0.0
        {
            return;
        }
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(
            prepared.content_rect.x(),
            prepared.content_rect.y(),
            prepared.content_rect.width(),
            prepared.content_rect.height(),
            &style,
        );
        for rect in rects {
            self.push_rect_in_band(PaintBand::Inline, rect);
        }
        for rounded_rect in rounded_rects {
            self.push_rounded_rect_in_band(PaintBand::Inline, rounded_rect);
        }
        for path in paths {
            self.push_path_in_band(PaintBand::Inline, path);
        }
        self.extend_strokes_in_band(PaintBand::Inline, strokes);
    }
}
