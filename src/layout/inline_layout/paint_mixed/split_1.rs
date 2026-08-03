use super::*;
use crate::layout::assets::{
    apply_object_fit, native_generated_gradient_primitive, raster_image_interpolation,
    svg_replaced_group,
};
use crate::layout::text_paint::{
    TextDecorationLineGeometry, TextDecorationLineGlyphCoverage, TextDecorationLineGlyphSequence,
    TextDecorationLineKind, TextDecorationOriginLineGeometry, TextDecorationStrokeAxis,
    TextInlineSpan, active_text_decoration_layers, positioned_rendered_runs_for_writing_mode,
    text_decoration_positioned_glyphs, text_decoration_skip_self_suppresses,
};
use std::rc::Rc;

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
        let first_letter_is_initial = block_style
            .first_letter_style
            .as_deref()
            .is_some_and(|style| !style.initial_letter.is_normal());
        let apply_typographic_pseudos = context.is_first_line
            && !first_letter_is_initial
            && block_style.first_line_style.is_some();
        let line = if apply_typographic_pseudos {
            let mut source_items = measured_inline_items(line_fragment.items());
            // First-letter splitting is already part of the opportunity graph:
            // it affects shaping, metrics, and the legal line-break set. At
            // paint time reapplying it would split the graph's source-shaped
            // Arabic run and lose its joining context.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
            // The graph has already split the first-letter fragment for line
            // fitting. Apply its cascaded style again after the first-line
            // delta so `::first-letter` retains precedence without changing
            // the graph's shaping boundary.
            // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
            apply_first_line_pseudos_to_line_items(&mut source_items, block_style, true);
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
        // A baseline-aligned atomic inline's block-start margin belongs to
        // the line participant, not only to the atom's border-box replay.
        // Anchor the whole horizontal line after that leading margin so its
        // text siblings and captured atomic fragments share the same logical
        // line placement. Vertical writing projects this at the line-geometry
        // boundary instead of changing the legacy physical page cursor.
        // <https://www.w3.org/TR/css-inline-3/#line-layout>
        let line_block_start_margin = line
            .iter()
            .filter_map(|item| match item.as_ref() {
                InlineLineItem::Atom(atom) => {
                    Some(inline_atom_logical_block_start_margin(atom, block_style))
                }
                InlineLineItem::Fragment(_) | InlineLineItem::Float(_) => None,
            })
            .fold(0.0, f32::max);
        let line_top = self.cursor_y
            - if block_style.writing_mode == WritingMode::HorizontalTb {
                line_block_start_margin
            } else {
                0.0
            };
        let mut line_geometry = InlineLineGeometry::new(
            self.content_left,
            self.content_right,
            line_top,
            context.line_block_size,
            context,
        );
        if block_style.writing_mode.has_vertical_lines() {
            line_geometry.block_start =
                match WritingModeAxes::new(block_style.writing_mode, block_style.used_direction())
                    .physical_side(LogicalSide::BlockStart)
                {
                    PhysicalSide::Left => line_geometry.block_start + line_block_start_margin,
                    PhysicalSide::Right => line_geometry.block_start - line_block_start_margin,
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("vertical writing modes always have a horizontal block axis")
                    }
                };
        }
        // Unicode space separators hang unconditionally at a selected visual
        // line edge. Their advance is excluded from both fitting and
        // alignment, but remains in the paint sequence past that aligned
        // edge. Conditional `pre-wrap` document-space hanging has separate
        // Phase II rules and is deliberately not folded into this effect.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let line_alignment_width = (line_metrics.width
            - visual_leading_inline_end_box_edge_width(&line, line_geometry))
        .max(0.0);
        let hanging_widths = line_fragment.hanging_widths;
        let line_available_width = line_geometry.inline_size;
        let line_align = context.text_align;
        let line_baseline_offset = line_metrics.baseline_offset;
        let parent_fragment_metrics = self.inline_text_box_metrics(block_style, None, 0.0);
        let line_layout_baseline_y = line_top - line_baseline_offset;
        // Text groups are positioned explicitly on `line_layout_baseline_y`
        // below. Atomic boxes must use that same CSS baseline; do not infer a
        // separate offset from a selected glyph program before the groups are
        // prepared.
        let mut line_rendered_baseline_shift = None;
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
                // Justification distributes from the logical inline start.
                // Physical `left` becomes inline-end for some vertical RTL
                // lines, which shifts the whole justified run before the
                // inter-word gap is applied.
                // <https://www.w3.org/TR/css-text-3/#text-justify>
                TextAlign::Start
            } else {
                line_align
            },
        );
        line_logical_inline_start += line_geometry.hanging_punctuation_offset(hanging_widths);
        // RTL physical origin is measured from the rendered line end. A
        // justified line occupies the available inline span after expansion,
        // not its pre-justification text measure.
        // <https://www.w3.org/TR/css-text-3/#text-justify>
        let positioned_line_inline_size = if should_justify_line {
            line_available_width
        } else {
            line_alignment_width
        };
        let line_physical_origin = line_geometry
            .visual_line_origin(line_logical_inline_start, positioned_line_inline_size);
        // CSS Text Phase II keeps a selected trailing preserved-space sequence
        // paintable while excluding its advance from fitting and alignment. In
        // an RTL line UAX #9 places that logical end sequence at the visual
        // start, so it must begin before the aligned content and advance into
        // it. LTR streams encounter the same sequence at their visual end.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        let hanging_paint_advance = line_fragment.edge_effects.pre_wrap_hanging_width
            + line_fragment.edge_effects.hanging_space_separator_width;
        let mut inline_position =
            if context.direction == Direction::Rtl && hanging_paint_advance > 0.0 {
                -hanging_paint_advance
            } else {
                0.0
            };
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
                    let leading_tracking = fragment.leading_tracking().points();
                    inline_position += leading_tracking;
                    let out_of_flow_paint_inline_advance = fragment
                        .out_of_flow_paint_inline_advance()
                        .map(|advance| advance.points());
                    let out_of_flow_paint_block_size = fragment
                        .out_of_flow_paint_block_size()
                        .map(|size| size.points());
                    let fragment =
                        PendingInlineFragment::new(fragment, measured_item.shaped.as_deref());
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_visual_offset = fragment.visual_offset();
                            pending_preserve_leading_summary_space =
                                item_index > 0 || previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        continue;
                    }
                    let fragment_metrics = self.inline_text_box_metrics(
                        fragment.style(),
                        measured_item.shaped.as_deref(),
                        fragment.baseline_shift(),
                    );
                    let fragment_content_block_size =
                        out_of_flow_paint_block_size.unwrap_or(fragment_metrics.content_block_size);
                    let fragment_content_trim = self
                        .inline_text_box_content_trim_for_style(fragment.style(), fragment_metrics);
                    let fragment_edge_content_bottom_y =
                        inline_fragment_horizontal_content_bottom_y(
                            &fragment,
                            line_top,
                            line_metrics.height,
                            line_baseline_offset,
                            fragment_metrics,
                            parent_fragment_metrics,
                        );
                    let fragment_background_y = fragment_edge_content_bottom_y.unwrap_or({
                        line_top - line_baseline_offset
                            + fragment.baseline_shift()
                            + fragment_metrics.content_baseline_offset
                            - fragment_content_block_size
                    });
                    let width = out_of_flow_paint_inline_advance
                        .unwrap_or(measured_item.width - leading_tracking)
                        .max(0.0);
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
                        if !decoration.paints_background_or_border {
                            continue;
                        }
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
                    let preserves_justification_policy =
                        pending_fragments.last().is_none_or(|previous| {
                            previous.style().text_justify == fragment.style().text_justify
                        });
                    // A tracking boundary belongs to this visual successor,
                    // not to either shaped text run.  It therefore also
                    // terminates paint-time shaping: allowing Parley to join
                    // the fragments would place glyphs as though no boundary
                    // advance existed (and can re-enable contextual forms).
                    let can_append = leading_tracking == 0.0
                        && preserves_justification_policy
                        && ((extra_space_width == 0.0
                            || justification_plan.justifies_inter_word())
                            && pending_fragments.last().is_some_and(|previous| {
                                can_queue_inline_fragments_for_shaping(previous, &fragment)
                            })
                            || pending_fragments.last().is_some_and(|previous| {
                                inline_fragment_can_append_collapsible_space(previous, &fragment)
                            })
                            || pending_inline_fragments_are_collapsible_space(&pending_fragments));
                    if pending_fragments.is_empty() {
                        pending_inline_position = inline_position;
                        pending_visual_offset = fragment.visual_offset();
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    } else if !can_append {
                        if let Some(group) = if justification_plan.justifies_inter_word() {
                            self.prepare_justified_inline_text_group_at_inline_position(
                                &pending_fragments,
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
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
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            )
                        } {
                            Self::update_line_rendered_baseline_shift(
                                &mut line_rendered_baseline_shift,
                                line_layout_baseline_y,
                                &group,
                            );
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
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            ) {
                                Self::update_line_rendered_baseline_shift(
                                    &mut line_rendered_baseline_shift,
                                    line_layout_baseline_y,
                                    &group,
                                );
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
                    // Bidi/default-ignorable controls can be emitted between
                    // an atomic marker image and its required suffix space.
                    // They are transparent to CSS Text's leading-space
                    // decision, so retain the atomic-boundary state until a
                    // source character that can establish a text context is
                    // encountered.
                    if !fragment
                        .text()
                        .chars()
                        .all(character_is_default_ignorable_code_point)
                    {
                        previous_item_was_opaque_atom = false;
                    }
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content(), InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    let preserves_pending_shaping =
                        inline_atom_preserves_pending_text_shaping(atom);
                    if (!preserves_pending_shaping || atom.leading_tracking().points() != 0.0)
                        && let Some(group) = if justification_plan.justifies_inter_word() {
                            self.prepare_justified_inline_text_group_at_inline_position(
                                &pending_fragments,
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
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
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            )
                        }
                    {
                        Self::update_line_rendered_baseline_shift(
                            &mut line_rendered_baseline_shift,
                            line_layout_baseline_y,
                            &group,
                        );
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    if !preserves_pending_shaping || atom.leading_tracking().points() != 0.0 {
                        pending_fragments.clear();
                        pending_horizontal_content_bottom_y = None;
                        pending_visual_offset = InlineVisualOffset::zero();
                        pending_preserve_leading_summary_space = false;
                    }
                    inline_position += atom.leading_tracking().points();
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
                            line_top,
                            line_metrics.height,
                            line_baseline_offset,
                            atom_metrics,
                            parent_fragment_metrics,
                        );
                        if edge.positioning_containing_block_id.is_some()
                            || !inline_box_edge_is_painted_by_adjacent_fragment(
                                &line,
                                item_index,
                                atom.style(),
                                *edge,
                            )
                        {
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
                        let atom_expansion_count =
                            justification_plan.expansion_count_after_item(item_index);
                        if extra_space_width > 0.0 && atom_expansion_count > 0 {
                            inline_position += extra_space_width * atom_expansion_count as f32;
                        }
                        if !preserves_pending_shaping {
                            previous_item_was_opaque_atom =
                                inline_atom_content_preserves_adjacent_space_summary(
                                    atom.content(),
                                );
                        }
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
                    // Captured formatting-context paint already uses the
                    // selected font metrics of its own source lines. Its
                    // exported CSS baseline can therefore be placed directly
                    // against the containing line's CSS baseline; applying
                    // the containing line's glyph-origin correction again
                    // would shift the whole captured subtree. Inline-box
                    // sequences with a visible overflow baseline, in
                    // contrast, are painted in this line and still need that
                    // conversion. An inline-block with non-visible overflow
                    // exports its margin-box edge as its CSS baseline, not
                    // an internal rendered text baseline, so it belongs with
                    // the captured-box case.
                    // <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>
                    let atom_uses_box_edge_baseline =
                        matches!(
                            atom.content(),
                            InlineAtomContent::StaticPositionPlaceholder
                                | InlineAtomContent::InlineFragment { .. }
                        ) || matches!(atom.content(), InlineAtomContent::InlineBox { .. })
                            && atom.style().overflow != css::Overflow::Visible;
                    let atom_rendered_baseline_shift = if atom_uses_box_edge_baseline {
                        0.0
                    } else {
                        line_rendered_baseline_shift.unwrap_or(0.0)
                    };
                    let y = inline_atom_horizontal_content_y(
                        atom,
                        block_style,
                        InlineAtomHorizontalPlacement {
                            line_top,
                            line_height: line_metrics.height,
                            line_baseline_offset,
                            line_rendered_baseline_shift: atom_rendered_baseline_shift,
                            content_block_size,
                            parent_metrics: parent_fragment_metrics,
                        },
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
                    let atom_expansion_count =
                        justification_plan.expansion_count_after_item(item_index);
                    if extra_space_width > 0.0 && atom_expansion_count > 0 {
                        inline_position += extra_space_width * atom_expansion_count as f32;
                    }
                    previous_item_was_opaque_atom =
                        inline_atom_content_preserves_adjacent_space_summary(atom.content());
                }
                InlineLineItem::Float(_) => {
                    // A float marker normally remains a zero-advance source
                    // checkpoint, preserving shaping and whitespace across
                    // an empty or overflowing inline float.  A float that
                    // shared this line after preceding source, however,
                    // carries the physical gap between the prefix and its
                    // suffix in `MeasuredInlineItem::width`.  Flush the
                    // prefix before crossing that gap: one text group cannot
                    // paint both sides at a single inline origin.
                    // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
                    // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
                    if measured_item.width > 0.0 {
                        if let Some(group) = if justification_plan.justifies_inter_word() {
                            self.prepare_justified_inline_text_group_at_inline_position(
                                &pending_fragments,
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
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
                                block_style,
                                line_geometry,
                                line_layout_baseline_y,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_visual_offset,
                                pending_horizontal_content_bottom_y,
                                pending_preserve_leading_summary_space,
                            )
                        } {
                            Self::update_line_rendered_baseline_shift(
                                &mut line_rendered_baseline_shift,
                                line_layout_baseline_y,
                                &group,
                            );
                            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                        }
                        pending_fragments.clear();
                        pending_horizontal_content_bottom_y = None;
                    }
                    inline_position += measured_item.width;
                    if measured_item.width > 0.0 {
                        pending_inline_position = inline_position;
                        pending_visual_offset = InlineVisualOffset::zero();
                    }
                    previous_item_was_opaque_atom = false;
                }
            }
        }
        if let Some(group) = if justification_plan.justifies_inter_word() {
            self.prepare_justified_inline_text_group_at_inline_position(
                &pending_fragments,
                block_style,
                line_geometry,
                line_layout_baseline_y,
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
                block_style,
                line_geometry,
                line_layout_baseline_y,
                line_logical_inline_start,
                line_physical_origin,
                pending_inline_position,
                pending_visual_offset,
                pending_horizontal_content_bottom_y,
                pending_preserve_leading_summary_space,
            )
        } {
            Self::update_line_rendered_baseline_shift(
                &mut line_rendered_baseline_shift,
                line_layout_baseline_y,
                &group,
            );
            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
        }
        Some(PreparedInlineLine {
            metrics: line_metrics,
            paint_items,
        })
    }

    fn update_line_rendered_baseline_shift(
        line_rendered_baseline_shift: &mut Option<f32>,
        line_layout_baseline_y: f32,
        group: &PreparedInlineTextGroup,
    ) {
        if group.style.font_size == 0.0 || group.style.line_height == 0.0 {
            return;
        }
        line_rendered_baseline_shift.get_or_insert(line_layout_baseline_y - group.y());
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
                    let leading_tracking = fragment.leading_tracking().points();
                    inline_position += leading_tracking;
                    let out_of_flow_paint_inline_advance = fragment
                        .out_of_flow_paint_inline_advance()
                        .map(|advance| advance.points());
                    let fragment =
                        PendingInlineFragment::new(fragment, measured_item.shaped.as_deref());
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
                        .unwrap_or(measured_item.width - leading_tracking)
                        .max(0.0);

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

                    inline_position += atom.leading_tracking().points();

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
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, Some(&group.shaped), 0.0);
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
        if let Some(content_bottom_y) = horizontal_content_bottom_y {
            let metrics = self.inline_text_box_metrics(&group.style, Some(&group.shaped), 0.0);
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
        let decoration_geometries = self.prepared_line_text_decoration_geometries(line);
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.rect.paint_rect(),
                    );
                }
                PreparedInlinePaintItem::TextGroup(group) => {
                    if let Some(source) = text_source {
                        self.paint_prepared_inline_text_group_with_source_and_decoration_geometries(
                            group,
                            source,
                            &decoration_geometries,
                        );
                    } else {
                        self.paint_prepared_inline_text_group_with_decoration_geometries(
                            group,
                            &decoration_geometries,
                        );
                    }
                }
                PreparedInlinePaintItem::Atom(atom) => {
                    self.paint_prepared_inline_atom(atom);
                }
            }
        }
    }

    /// Select one considered-text geometry for every decoration origin on a
    /// prepared line.
    ///
    /// Decorations propagate through eligible in-flow descendants, but CSS
    /// Text Decoration requires a decorating box to use one uniform position
    /// and thickness for all of its selected text on a line.  The prepared
    /// line is the first point where all shaped descendants and their physical
    /// baselines coexist, so collection belongs here rather than in an
    /// individual text-group painter.
    ///
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    /// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-line-uniformity>
    fn prepared_line_text_decoration_geometries(
        &mut self,
        line: &PreparedInlineLine,
    ) -> Vec<TextDecorationOriginLineGeometry> {
        let mut geometries: Vec<TextDecorationOriginLineGeometry> = Vec::new();
        for item in &line.paint_items {
            let PreparedInlinePaintItem::TextGroup(group) = item else {
                continue;
            };
            if group.style.visibility != Visibility::Visible || group.width() <= 0.0 {
                continue;
            }
            let decorations = active_text_decoration_layers(&group.style);
            if decorations.is_empty() {
                continue;
            }
            let font_id = self.font_system.resolve_style(&group.style);
            let metrics = self
                .font_system
                .text_decoration_metrics(font_id, &group.style);
            let reference = group
                .decoration_paint_rect
                .map(|rect| rect.origin)
                .unwrap_or_else(|| PaintPoint::new(group.x(), group.y()));
            let coverage = match group.style.writing_mode {
                WritingMode::HorizontalTb => TextDecorationLineGlyphCoverage {
                    span: TextInlineSpan::from_start_and_length(
                        reference.x,
                        group.width().max(group.shaped.advance_width()),
                    ),
                },
                WritingMode::VerticalRl
                | WritingMode::VerticalLr
                | WritingMode::SidewaysRl
                | WritingMode::SidewaysLr => TextDecorationLineGlyphCoverage {
                    span: TextInlineSpan::from_start_and_length(
                        reference.y,
                        group.width().max(group.shaped.advance_width()),
                    ),
                },
            };
            let axis = if group.style.writing_mode == WritingMode::HorizontalTb {
                TextDecorationStrokeAxis::Horizontal
            } else {
                TextDecorationStrokeAxis::Vertical
            };
            let mut glyph_runs =
                positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
            self.align_sideways_runs_to_vertical_line_box(
                &mut glyph_runs,
                &group.shaped,
                &group.style,
            );
            let positioned_glyphs = text_decoration_positioned_glyphs(
                axis,
                reference.x,
                reference.y,
                coverage.span.start,
                coverage.span.length(),
                &glyph_runs,
            );
            for decoration in decorations {
                let participates = (decoration.decoration.underline
                    && !text_decoration_skip_self_suppresses(
                        &group.style,
                        TextDecorationLineKind::Underline,
                    ))
                    || (decoration.decoration.overline
                        && !text_decoration_skip_self_suppresses(
                            &group.style,
                            TextDecorationLineKind::Overline,
                        ))
                    || (decoration.decoration.line_through
                        && !text_decoration_skip_self_suppresses(
                            &group.style,
                            TextDecorationLineKind::LineThrough,
                        ));
                if !participates {
                    continue;
                }
                if let Some(existing) = geometries
                    .iter_mut()
                    .find(|existing| Rc::ptr_eq(&existing.origin_style, &decoration.origin_style))
                {
                    // The selected text with the largest em box is the
                    // conservative shared metric source: it keeps automatic
                    // decorations clear of every eligible descendant rather
                    // than letting a later, smaller receiver pull the common
                    // line through it.  The physical outside reference is
                    // likewise the furthest text-under edge of the selected
                    // line in each writing-axis projection.
                    if group.style.font_size > existing.geometry.considered_font_size {
                        existing.geometry.considered_font_size = group.style.font_size;
                        existing.geometry.considered_metrics = metrics;
                    }
                    existing.selected_inline_span = Some(
                        existing
                            .selected_inline_span
                            .map(|span| {
                                TextInlineSpan::new(
                                    span.start.min(coverage.span.start),
                                    span.end.max(coverage.span.end),
                                )
                            })
                            .unwrap_or(coverage.span),
                    );
                    existing
                        .glyph_sequence
                        .glyphs
                        .extend(positioned_glyphs.iter().cloned());
                    match group.style.writing_mode {
                        WritingMode::HorizontalTb => {
                            existing.line_reference.y = existing.line_reference.y.min(reference.y);
                        }
                        WritingMode::VerticalRl
                        | WritingMode::VerticalLr
                        | WritingMode::SidewaysRl
                        | WritingMode::SidewaysLr => {
                            existing.line_reference.x = existing.line_reference.x.min(reference.x);
                        }
                    }
                    continue;
                }
                geometries.push(TextDecorationOriginLineGeometry {
                    origin_style: Rc::clone(&decoration.origin_style),
                    geometry: TextDecorationLineGeometry::from_origin_and_considered_text(
                        decoration.origin_style.as_ref(),
                        &group.style,
                        metrics,
                    ),
                    selected_inline_span: Some(coverage.span),
                    glyph_sequence: TextDecorationLineGlyphSequence {
                        glyphs: positioned_glyphs.clone(),
                    },
                    line_reference: reference,
                });
            }
        }
        for geometry in &mut geometries {
            geometry.glyph_sequence.glyphs.sort_by(|left, right| {
                left.inline_start
                    .partial_cmp(&right.inline_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        geometries
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
            self.replay_escaped_inline_atom_positioned_layers(prepared);
            return;
        }
        if matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) {
            self.replay_escaped_inline_atom_positioned_layers(prepared);
            return;
        }
        let checkpoint = self.current_page.paint_checkpoint();
        if let Some(marker) = atom.outside_marker() {
            let borders = used_border_widths(atom.style());
            let text_top = prepared.content_rect.y() + prepared.content_rect.height()
                - borders.top
                - atom.style().padding.top;
            let formatted_line_block_start = PageTopBlockPosition::new(text_top);
            let fallback_baseline_offset =
                self.inline_box_text_line_layout_baseline_offset(atom.style());
            self.paint_outside_marker(
                marker,
                atom.style(),
                OutsideMarkerAnchor {
                    content_inline_span: PageInlineSpan::from_edges(
                        prepared.content_rect.x() + borders.left,
                        prepared.content_rect.x() + prepared.content_rect.width() - borders.right,
                    ),
                    formatted_line_block_start,
                    alphabetic_baseline: formatted_line_block_start
                        .toward_block_end(layout_pt(fallback_baseline_offset)),
                },
            );
        }
        self.paint_prepared_inline_atom_contents(prepared);
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if !fragment.is_empty() {
            let mut bounds = prepared.content_rect.paint_clip();
            if matches!(
                atom.content(),
                InlineAtomContent::Image(_) | InlineAtomContent::Gradient { .. }
            ) && self.root_principal_flow_context.active_body.is_some()
                && self.principal_flow.writing_mode == WritingMode::VerticalRl
            {
                // The propagated vertical-rl body owns the initial canvas's
                // inline-end inset. Keep that extent in the atomic image's
                // layout clip without altering the image's used content
                // rect or resampling it.
                // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                bounds = PaintClip::from_paint_rect(paint_space_rect(
                    bounds.x(),
                    bounds.y(),
                    bounds.width(),
                    bounds.height()
                        + self
                            .root_principal_flow_context
                            .active_body_inline_end_inset
                            .points(),
                ));
            }
            let policy = StackingContextPolicy::for_atomic(atom.style(), PaintBand::Inline, bounds);
            let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                .with_source_order(self.next_paint_source_order())
                .with_effects(policy.effects)
                .with_bounds(bounds);
            let fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
        self.replay_escaped_inline_atom_positioned_layers(prepared);
    }

    /// Replays positioned descendants attached to an inline source atom.
    ///
    /// A positioned inline's start edge may be the only selected fragment
    /// that owns its descendants.  Inline-edge atoms do not otherwise paint
    /// content, but must still replay those layers.
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
    fn replay_escaped_inline_atom_positioned_layers(&mut self, prepared: &PreparedInlineAtom) {
        if let Some(layers) = prepared.atom.escaped_positioned_layers() {
            for layer in layers.iter() {
                let atom_offset = layer
                    .escaped_atom_translation
                    .atom_offset(prepared.content_rect.x(), prepared.content_rect.y());
                let mut layer = layer.clone().translated(atom_offset);
                layer.page_index = layer
                    .escaped_atom_translation
                    .replay_page_index(self.pages.len(), layer.page_index);
                self.positioned_layers.push(layer);
            }
        }
    }

    fn paint_inline_atom_box_background(&mut self, border_rect: PaintRect, style: &ComputedStyle) {
        for primitive in self.box_background_primitives(border_rect, style) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
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
        if !matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) && (atom.style().background_color.is_potentially_visible()
            || atom.style().background_image.is_image()
            || used_border_width(atom.style()) > layout_pt(0.0))
        {
            self.paint_inline_atom_box_background(
                paint_space_rect(content_x, y, content_width, content_height),
                atom.style(),
            );
        }
        match atom.content() {
            InlineAtomContent::InlineEdge(_)
            | InlineAtomContent::Leader(_)
            | InlineAtomContent::StaticPositionPlaceholder => {}
            InlineAtomContent::Canvas => {}
            InlineAtomContent::Iframe(element_id) => {
                let Some(document) = self.iframe_documents.get(element_id) else {
                    return;
                };
                let Some(page) = document.pages.first() else {
                    return;
                };
                let borders = used_border_widths(atom.style());
                let iframe_x = content_x + borders.left + atom.style().padding.left;
                let iframe_y = y + borders.bottom + atom.style().padding.bottom;
                let iframe_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(0.0);
                let iframe_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style().padding.top
                    - atom.style().padding.bottom)
                    .max(0.0);
                let clip = PaintClip::from_paint_rect(paint_space_rect(
                    iframe_x,
                    iframe_y,
                    iframe_width,
                    iframe_height,
                ));
                let mut fragment = page.paint_fragment().translated(PaintTranslation::new(
                    iframe_x,
                    iframe_y + iframe_height - page.height(),
                ));
                fragment.promote_page_background_to_in_flow_block();
                fragment.promote_background_border_to_in_flow_block();
                fragment = fragment.with_contents_effect_scoped_to_rect(clip);
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
            InlineAtomContent::Image(decoded) => {
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
                let mut image = RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, image_width, image_height),
                    false,
                    decoded.pixel_width,
                    decoded.pixel_height,
                    None,
                    raster_image_interpolation(atom.style()),
                    decoded.rgb.shared(),
                    decoded.alpha.clone(),
                    atom.alt_text().map(Rc::from),
                )
                .with_raster_color_space(decoded.color_space.clone())
                .with_image_id(decoded.image_id);
                if apply_object_fit(
                    &mut image,
                    atom.style().object_fit,
                    atom.style().object_position.clone(),
                    atom.style().object_view_box.clone(),
                ) {
                    self.push_image_in_band(PaintBand::Inline, image);
                }
            }
            InlineAtomContent::Gradient { image, fallback } => {
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
                let paint_rect = paint_space_rect(image_x, image_y, image_width, image_height);
                if atom.style().object_fit == css::ObjectFit::Fill
                    && matches!(atom.style().object_view_box, css::ObjectViewBox::None)
                    && let Some(primitive) = native_generated_gradient_primitive(
                        image,
                        paint_rect,
                        atom.style().color,
                        None,
                    )
                {
                    self.push_primitive_in_band(PaintBand::Inline, primitive);
                } else {
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_rect,
                        false,
                        fallback.pixel_width,
                        fallback.pixel_height,
                        None,
                        raster_image_interpolation(atom.style()),
                        fallback.rgb.shared(),
                        fallback.alpha.clone(),
                        atom.alt_text().map(Rc::from),
                    )
                    .with_raster_color_space(fallback.color_space.clone())
                    .with_image_id(fallback.image_id);
                    if apply_object_fit(
                        &mut rendered,
                        atom.style().object_fit,
                        atom.style().object_position.clone(),
                        atom.style().object_view_box.clone(),
                    ) {
                        self.push_image_in_band(PaintBand::Inline, rendered);
                    }
                }
            }
            InlineAtomContent::Svg { asset } => {
                if let Some(asset) = asset {
                    let borders = used_border_widths(atom.style());
                    let svg_x = content_x + borders.left + atom.style().padding.left;
                    let svg_y = y + borders.bottom + atom.style().padding.bottom;
                    let svg_width = (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0);
                    let svg_height = (content_height
                        - borders.top
                        - borders.bottom
                        - atom.style().padding.top
                        - atom.style().padding.bottom)
                        .max(0.0);
                    // Inline SVG follows the same concrete-object and source
                    // selection path as block replaced SVG, including its
                    // CSS root viewport specialization.
                    if svg_width > 0.0 && svg_height > 0.0 {
                        self.push_svg_group_in_band(
                            PaintBand::Inline,
                            svg_replaced_group(
                                asset,
                                paint_space_rect(svg_x, svg_y, svg_width, svg_height),
                                atom.style().object_fit,
                                atom.style().object_position.clone(),
                                atom.style().object_view_box.clone(),
                            ),
                        );
                    }
                }
            }
            InlineAtomContent::InlineBox { sequence } => {
                let borders = used_border_widths(atom.style());
                let text_top = y + content_height - borders.top - atom.style().padding.top;
                let text_x = content_x
                    + borders.left
                    + atom.style().padding.left
                    + atom.content_inline_offset();
                let text_available_width = atom.content_inline_paint_width().unwrap_or_else(|| {
                    (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0)
                });
                self.paint_inline_box_sequence(
                    sequence,
                    atom.style(),
                    text_x,
                    text_available_width,
                    text_top,
                );
            }
            InlineAtomContent::Ruby {
                base,
                annotations,
                annotation_sides,
                base_block_size,
                annotation_block_sizes,
                ..
            } => {
                let borders = used_border_widths(atom.style());
                let text_x = content_x
                    + borders.left
                    + atom.style().padding.left
                    + atom.content_inline_offset();
                let text_available_width = atom.content_inline_paint_width().unwrap_or_else(|| {
                    (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0)
                });
                let ruby_origin = ruby::RubyPaintOrigin::new(text_x, y);
                // Paint the base at the ruby atom's logical under side. Each
                // annotation level is stacked toward its logical over side;
                // the horizontal coordinates are a backend boundary, while
                // the nested line sequences retain their own writing mode.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-position>
                // The initial value of `ruby-align` is `space-around`. For a
                // single base/annotation run, its used result is centering the
                // shorter run in the paired column. Multi-base distribution
                // is represented by the normalized column spans and is
                // completed before this paint boundary.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
                let base_x = ruby_origin.inline()
                    + (text_available_width - base.paint_inline_size).max(0.0) / 2.0;
                let all_annotations_are_under = annotation_sides
                    .iter()
                    .all(|side| *side == css::RubyAnnotationSide::Under);
                self.paint_inline_box_sequence(
                    &base.sequence,
                    &base.style,
                    base_x,
                    text_available_width,
                    ruby_origin.block_offset(ruby::RubyBlockExtent::new(
                        if all_annotations_are_under {
                            annotation_block_sizes.iter().sum::<f32>() + *base_block_size
                        } else {
                            *base_block_size
                        },
                    )),
                );
                // The inline-sequence paint boundary is expressed in the
                // page's bottom-origin block coordinate. The first (closest)
                // annotation level therefore starts at the base level's
                // block-start edge; each subsequent level advances toward the
                // logical over side by the preceding annotation extent.
                // Keep this entirely within the ruby atom: annotations are
                // not ordinary parent-line children.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-position>
                let base_paint_top =
                    ruby_origin.block_offset(ruby::RubyBlockExtent::new(*base_block_size));
                let mut over_annotation_baseline = base_paint_top;
                let mut under_annotation_baseline =
                    ruby_origin.block_offset(ruby::RubyBlockExtent::default());
                for ((annotation, annotation_block_size), side) in annotations
                    .iter()
                    .zip(annotation_block_sizes)
                    .zip(annotation_sides)
                {
                    let annotation_available_width =
                        if annotation.starts_span && annotation.column_span > 1 {
                            annotation.containing_inline_size
                        } else {
                            text_available_width
                        };
                    let annotation_x = ruby_origin.inline()
                        + (annotation_available_width - annotation.paint_inline_size).max(0.0)
                            / 2.0;
                    // The captured sequence's first visible glyph may have a
                    // different ascent from the annotation line box (for
                    // example Ahem). Reconcile that glyph baseline with the
                    // line box before placing it at the ruby-level boundary.
                    let annotation_line_box_baseline = self
                        .font_system
                        .rendered_first_line_baseline_offset(&annotation.style)
                        .points();
                    let annotation_baseline = match side {
                        css::RubyAnnotationSide::Over => {
                            let baseline = over_annotation_baseline;
                            over_annotation_baseline += *annotation_block_size;
                            baseline
                        }
                        css::RubyAnnotationSide::Under => {
                            let baseline = under_annotation_baseline;
                            under_annotation_baseline += *annotation_block_size;
                            baseline
                        }
                    };
                    self.paint_inline_box_sequence(
                        &annotation.sequence,
                        &annotation.style,
                        annotation_x,
                        annotation_available_width,
                        annotation_baseline + annotation_line_box_baseline,
                    );
                }
            }
            InlineAtomContent::TextCombineUpright {
                sequence,
                horizontal_style,
                inline_scale,
            } => {
                self.paint_text_combine_upright(
                    sequence,
                    horizontal_style,
                    *inline_scale,
                    prepared.content_rect,
                );
            }
            InlineAtomContent::InlineFragment {
                fragment,
                table_cell_context,
            } => {
                if let Some(context) = table_cell_context {
                    // The fragment is already normalized to its atomic
                    // border box, but preserve the originating table-cell
                    // coordinate context through replay. This keeps a later
                    // writing-mode-aware fragment projection from guessing
                    // at the enclosing inline line's flow.
                    debug_assert!(
                        context.origin.x().is_finite() && context.origin.top_y().is_finite()
                    );
                    debug_assert!(matches!(
                        context.writing_mode,
                        WritingMode::HorizontalTb
                            | WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr
                    ));
                    debug_assert!(matches!(context.direction, Direction::Ltr | Direction::Rtl));
                }
                self.current_page
                    .append_paint_fragment(fragment, PaintTranslation::new(content_x, y));
            }
        }
        for primitive in self.box_outline_primitives(
            paint_space_rect(content_x, y, content_width, content_height),
            atom.style(),
        ) {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
        if let Some(target) = atom.link_target() {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                paint_space_rect(content_x, y, content_width, content_height),
                target.to_string(),
            ));
        }
    }

    /// Paint a horizontal tate-chu-yoko sequence inside its one-em atomic
    /// vertical box.  Capturing the nested sequence before adding the scale
    /// keeps glyphs, shadows, decorations, clipping, and links in one normal
    /// paint subtree rather than applying disconnected per-glyph offsets.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    fn paint_text_combine_upright(
        &mut self,
        sequence: &InlineLineSequence,
        horizontal_style: &ComputedStyle,
        inline_scale: f32,
        content_rect: PhysicalInlineRect,
    ) {
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_inline_box_sequence(
            sequence,
            horizontal_style,
            content_rect.x(),
            sequence.available_width,
            content_rect.y() + content_rect.height(),
        );
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if fragment.is_empty() {
            return;
        }
        let scaled_width = sequence.available_width * inline_scale;
        let centered_x = content_rect.x() + (content_rect.width() - scaled_width) / 2.0;
        let transform = PaintTransform::translate(PaintTranslation::new(centered_x, 0.0))
            .multiply(PaintTransform::scale(inline_scale, 1.0))
            .multiply(PaintTransform::translate(PaintTranslation::new(
                -content_rect.x(),
                0.0,
            )));
        let bounds = content_rect.paint_clip();
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                transform: Some(transform),
                overflow_clip: Some(bounds),
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(PaintBand::Inline, context),
            PaintTranslation::identity(),
        );
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
        if style.background_color.is_transparent()
            && style.background_image.is_none()
            && used_border_width(&style) == layout_pt(0.0)
        {
            return;
        }
        for primitive in self.box_background_primitives(
            paint_space_rect(
                prepared.content_rect.x(),
                prepared.content_rect.y(),
                prepared.content_rect.width(),
                prepared.content_rect.height(),
            ),
            &style,
        ) {
            self.push_primitive_in_band(PaintBand::Inline, primitive);
        }
    }
}

/// Return whether an inline box edge is transparent to a pending text shaping
/// group.
///
/// A plain inline element contributes zero-width start/end edges. CSS Text
/// still shapes across those edges, even when its child has a distinct paint
/// style. Used inline-axis decoration and bidi isolation are the exceptions:
/// they form an actual typographic boundary.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
fn inline_atom_preserves_pending_text_shaping(atom: &InlineAtom) -> bool {
    matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
        if edge.advance == 0.0
            && edge.paint_extent == 0.0
            && !inline_box_edge_breaks_shaping(atom.style())
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}
