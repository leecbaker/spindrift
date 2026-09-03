use super::*;
use crate::layout::inline_layout::mixed::measured_item_is_transparent_mixed_inline_edge;

/// A final line-local position after visual bidi run placement.
///
/// This remains in the line's logical-inline coordinate system until
/// [`InlineLineGeometry`] performs the one physical writing-mode projection.
/// Keeping it distinct from measured advances prevents source-order fitting
/// coordinates from being reused as prepared fragment geometry.
#[derive(Debug, Clone, Copy)]
struct PreparedInlineVisualPosition(LayoutLength);

impl PreparedInlineVisualPosition {
    fn points(self) -> f32 {
        self.0.points()
    }
}

/// Resolve source fragments and structural edges inside opposite-direction
/// visual runs to their final line-local positions.
///
/// UAX #9 emits a visual run as one unit. Its source fragments retain logical
/// source order for boundary shaping, so a vertical run whose resolved
/// direction differs from the paragraph occupies the selected span from the
/// opposite edge. This placement table is prepared before paint scopes or
/// text groups are constructed and is therefore shared by backgrounds,
/// positioned-inline geometry, and structural edge markers.
/// <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>
/// <https://drafts.csswg.org/css-writing-modes-4/#bidi-algo>
fn prepared_inline_visual_positions(
    line: &[MeasuredInlineItem],
    block_style: &ComputedStyle,
    paragraph_direction: Direction,
    line_start: LayoutLength,
    additional_advances: &[LayoutLength],
) -> Vec<Option<PreparedInlineVisualPosition>> {
    debug_assert_eq!(line.len(), additional_advances.len());
    let item_advance = |index: usize, item: &MeasuredInlineItem| {
        let content_advance = match &item.item {
            InlineLineItem::Fragment(fragment) => fragment
                .out_of_flow_paint_inline_advance()
                .map(|advance| advance.points())
                .unwrap_or(item.base_advance().points())
                .max(0.0),
            InlineLineItem::Atom(atom) => match atom.content() {
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => edge.advance,
                InlineAtomContent::Leader(_) => 0.0,
                _ => inline_atom_logical_inline_size(atom, block_style),
            },
            InlineLineItem::Float(_) => item.used_advance().points(),
        };
        content_advance + additional_advances[index].points()
    };
    let is_visual_run_item = |item: &MeasuredInlineItem| {
        matches!(item.item, InlineLineItem::Fragment(_))
            || measured_item_is_transparent_mixed_inline_edge(item)
    };
    let resolved_direction = |item: &MeasuredInlineItem| match &item.item {
        InlineLineItem::Fragment(fragment) => {
            fragment
                .resolved_bidi_direction()
                .unwrap_or(match paragraph_direction {
                    Direction::Ltr => ResolvedBidiDirection::Ltr,
                    Direction::Rtl => ResolvedBidiDirection::Rtl,
                })
        }
        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => match paragraph_direction {
            Direction::Ltr => ResolvedBidiDirection::Ltr,
            Direction::Rtl => ResolvedBidiDirection::Rtl,
        },
    };

    let mut natural_starts = Vec::with_capacity(line.len());
    let mut cursor = line_start.points();
    for (index, item) in line.iter().enumerate() {
        cursor += item.advance.boundary_before().points();
        natural_starts.push(cursor);
        cursor += item_advance(index, item);
    }
    // `None` deliberately preserves the existing shaped-group cursor for
    // runs whose selected order is already their final visual order. That
    // cursor can differ from graph measurement after final shaping. Only the
    // opposite-direction vertical case needs an explicit source-fragment
    // placement within its already selected visual run.
    let mut placements = vec![None; line.len()];

    if !block_style.writing_mode.has_vertical_lines() {
        return placements;
    }

    let paragraph_resolved = match paragraph_direction {
        Direction::Ltr => ResolvedBidiDirection::Ltr,
        Direction::Rtl => ResolvedBidiDirection::Rtl,
    };
    let mut run_start = 0;
    while run_start < line.len() {
        if !is_visual_run_item(&line[run_start]) {
            run_start += 1;
            continue;
        }
        let first_fragment = (run_start..line.len()).find(|&index| {
            matches!(line[index].item, InlineLineItem::Fragment(_))
                && line[run_start..index].iter().all(&is_visual_run_item)
        });
        let Some(first_fragment) = first_fragment else {
            break;
        };
        let run_direction = resolved_direction(&line[first_fragment]);
        let mut run_end = first_fragment + 1;
        while run_end < line.len() && is_visual_run_item(&line[run_end]) {
            if matches!(line[run_end].item, InlineLineItem::Fragment(_))
                && resolved_direction(&line[run_end]) != run_direction
            {
                break;
            }
            run_end += 1;
        }
        if run_direction != paragraph_resolved {
            let start = natural_starts[run_start];
            let end = if run_end == line.len() {
                cursor
            } else {
                natural_starts[run_end]
            };
            let extent = (end - start).max(0.0);
            for index in run_start..run_end {
                let local_start = natural_starts[index] - start;
                let width = item_advance(index, &line[index]);
                placements[index] = Some(PreparedInlineVisualPosition(layout_pt(
                    start + extent - local_start - width,
                )));
            }
        }
        run_start = run_end;
    }
    placements
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
        let should_justify_line = context.text_align.justifies()
            && !matches!(block_style.text_justify, TextJustify::None);
        let split_for_inter_character =
            should_justify_line && matches!(block_style.text_justify, TextJustify::InterCharacter);
        let split_for_auto_justification =
            should_justify_line && matches!(block_style.text_justify, TextJustify::Auto);
        let first_letter_is_initial = block_style
            .first_letter_style
            .as_deref()
            .is_some_and(|style| !style.initial_letter.is_normal());
        let apply_typographic_pseudos = !line_fragment.first_line_style_materialized
            && context.is_first_line
            && !first_letter_is_initial
            && block_style.first_line_style.is_some();
        // A prepared text group has one foreground/effect owner. On the
        // generated first line, a collapsible space can carry different
        // descendant or generated-pseudo provenance from the visible text on
        // either side. Keep its selected advance, but do not let it bridge
        // incompatible first-line paint states into one group.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
        let preserve_first_line_paint_boundaries =
            context.is_first_line && block_style.first_line_style.is_some();
        let line = if apply_typographic_pseudos {
            let mut source_items = measured_inline_items(line_fragment.items());
            // First-letter splitting is already part of the opportunity graph:
            // it affects shaping, metrics, and the legal line-break set. At
            // paint time reapplying it would split the graph's source-shaped
            // Arabic run and lose its joining context.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
            // The graph has already materialized every selected first-letter
            // stream fragment for fitting. Reopening selection here would
            // select from an already-split item list, so apply only the
            // first-line delta.
            // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
            apply_first_line_pseudos_to_line_items(
                &mut source_items,
                block_style,
                false,
                &mut self.font_system,
            );
            let source_items = if split_for_inter_character {
                split_mixed_line_into_inter_character_units(&source_items)
            } else if split_for_auto_justification {
                split_mixed_line_into_auto_justification_units(&source_items)
            } else {
                source_items
            };
            source_items
                .into_iter()
                .map(|item| {
                    let shaped = match &item {
                        InlineLineItem::Fragment(fragment) => {
                            self.font_system.shape_untracked_inline_line(
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
                    MeasuredInlineItem::new(item, width, shaped)
                })
                .collect::<Vec<_>>()
        } else if split_for_inter_character || split_for_auto_justification {
            let source_items = measured_inline_items(line_fragment.items());
            let source_items = if split_for_inter_character {
                split_mixed_line_into_inter_character_units(&source_items)
            } else {
                split_mixed_line_into_auto_justification_units(&source_items)
            };
            source_items
                .into_iter()
                .map(|item| {
                    let shaped = match &item {
                        InlineLineItem::Fragment(fragment) => {
                            self.font_system.shape_untracked_inline_line(
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
                    MeasuredInlineItem::new(item, width, shaped)
                })
                .collect::<Vec<_>>()
        } else {
            line_fragment.items().to_vec()
        };
        // Alignment and justification consume the final visual inline
        // advances, not the pre-bidi source estimate recorded during line
        // selection. In particular, a ZWJ/ZWNJ can remain in the source and
        // shaping context while contributing no visual advance after fallback
        // controls are removed. Retain the selected line's block metrics, but
        // reconcile its inline measure with these final paint items.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/css-text-3/#text-encoding>
        let visual_widths =
            crate::layout::inline_layout::graph::inline_content_width_for_line_items(
                &line,
                &mut self.font_system,
                |item| item.used_advance().points(),
            );
        let final_painted_width = self.final_painted_inline_width(&line, block_style);
        let final_content_width = (final_painted_width
            - line_fragment.edge_effects.pre_wrap_hanging_width
            - visual_widths.trailing_space_width)
            .max(0.0);
        let mut line_metrics = line_fragment.metrics;
        // Align using the final visual paint sequence. The source-selection
        // metric can differ from this width not only for CSS bidi controls,
        // but also after fallback shaping or shared boundary-cluster
        // ownership. Preserve the selected Phase II `pre-wrap` hanging
        // advance while reconciling that final paint width: its source stays
        // paintable, but CSS Text excludes it from alignment.
        // CSS alignment applies to the used inline content, not a pre-paint
        // source estimate.
        // <https://www.w3.org/TR/css-text-3/#text-align-property>
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        line_metrics.width = final_content_width;
        // The final visual width still includes a punctuation glyph which
        // CSS Text lets hang outside the line measure.  The selected record
        // normally applied this subtraction before paint, but replacing its
        // source metric above requires preserving that same exclusion here.
        // <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>
        if line_box_uses_hanging_punctuation_alignment(block_style) {
            line_metrics.width = (line_metrics.width
                - line_fragment.hanging_widths.start
                - line_fragment.hanging_widths.end)
                .max(0.0);
        }
        let line_items = measured_inline_items(&line);
        // CSS Inline first aligns every in-flow inline-level box to the
        // shared line baseline, then sizes the line from each participant's
        // layout bounds. Atomic inlines contribute their margin boxes, but
        // their margins must not move sibling text: their own paint path
        // converts the margin-box baseline back to the border-box origin.
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>
        // <https://www.w3.org/TR/CSS22/visudet.html#line-height>
        let mut line_geometry = InlineLineGeometry::new(
            self.content_left,
            self.content_right,
            self.cursor_y,
            context.line_block_size,
            context,
        );
        line_geometry.text_box_line_trim = line_fragment.text_box_trim;
        // Unicode space separators hang unconditionally at a selected visual
        // line edge. Their advance is excluded from both fitting and
        // alignment, but remains in the paint sequence past that aligned
        // edge. Conditional `pre-wrap` document-space hanging has separate
        // Phase II rules and is deliberately not folded into this effect.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let line_alignment_width = line_metrics.width;
        let hanging_widths = line_fragment.hanging_widths;
        let line_available_width = line_geometry.inline_size;
        let line_align = context.text_align;
        let line_baseline_offset = line_metrics.baseline_offset;
        let parent_fragment_metrics = self.inline_text_box_metrics(block_style, 0.0);
        let line_top = self.cursor_y;
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
            let natural_painted_width = self
                .natural_painted_inline_width_for_justification(
                    &line,
                    &justification_plan,
                    block_style,
                )
                .max(0.0);
            if natural_painted_width > 0.0 {
                natural_painted_width
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
        let mut pending_synthesize_leading_summary_space = false;
        let mut pending_horizontal_content_bottom_y = None;
        let mut previous_item_was_opaque_atom = false;
        let mut pending_started_after_opaque_atom = false;
        let mut paint_items = Vec::new();
        let mut positioning_geometry = Vec::new();
        let additional_visual_advances = line
            .iter()
            .enumerate()
            .map(|(item_index, _)| {
                let inter_character_expansion_count =
                    justification_plan.inter_character_expansion_count_after_item(item_index);
                let fragment_expansion_count =
                    justification_plan.expansion_count_after_item(item_index);
                let count = if justification_plan.justifies_inter_character()
                    && inter_character_expansion_count > 0
                {
                    inter_character_expansion_count
                } else {
                    fragment_expansion_count
                };
                layout_pt(extra_space_width * count as f32)
            })
            .collect::<Vec<_>>();
        let visual_positions = prepared_inline_visual_positions(
            &line,
            block_style,
            context.direction,
            layout_pt(inline_position),
            &additional_visual_advances,
        );
        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let leading_tracking = measured_item.advance.boundary_before().points();
                    // Tracking is a boundary between visual text groups.  A
                    // pending group owns its final shaped advance, so flush
                    // it before crossing that boundary instead of first
                    // moving the provisional source-measure cursor.
                    // <https://www.w3.org/TR/css-text-3/#letter-spacing-property>
                    if leading_tracking != 0.0 && !pending_fragments.is_empty() {
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
                                pending_synthesize_leading_summary_space,
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
                                pending_synthesize_leading_summary_space,
                            )
                        } {
                            inline_position = pending_inline_position + group.width();
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
                        pending_synthesize_leading_summary_space = false;
                    }
                    inline_position += leading_tracking;
                    let out_of_flow_paint_inline_advance = fragment
                        .out_of_flow_paint_inline_advance()
                        .map(|advance| advance.points());
                    let out_of_flow_paint_block_size = fragment
                        .out_of_flow_paint_block_size()
                        .map(|size| size.points());
                    let fragment = PendingInlineFragment::new(fragment);
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_visual_offset = fragment.visual_offset();
                            pending_started_after_opaque_atom = previous_item_was_opaque_atom;
                            pending_preserve_leading_summary_space =
                                item_index > 0 || previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        continue;
                    }
                    let fragment_metrics =
                        self.inline_text_box_metrics(fragment.style(), fragment.baseline_shift());
                    let fragment_content_block_size =
                        out_of_flow_paint_block_size.unwrap_or(fragment_metrics.content_block_size);
                    let fragment_content_trim = self
                        .inline_text_box_content_trim_for_style(fragment.style(), fragment_metrics);
                    let fragment_edge_content_bottom_y =
                        inline_fragment_horizontal_content_bottom_y(
                            &fragment,
                            fragment.line_relative_alignment(),
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
                        .unwrap_or(measured_item.base_advance().points())
                        .max(0.0);
                    let prepared_inline_position = visual_positions[item_index]
                        .map(PreparedInlineVisualPosition::points)
                        .unwrap_or(inline_position);
                    let fragment_expansion_count =
                        justification_plan.expansion_count_after_item(item_index);
                    let fragment_rect = trim_inline_content_rect(
                        line_geometry.visual_line_item_rect(
                            line_logical_inline_start,
                            line_physical_origin,
                            prepared_inline_position,
                            width + extra_space_width * fragment_expansion_count as f32,
                            fragment_background_y,
                            fragment_content_block_size,
                        ),
                        block_style.writing_mode,
                        fragment_content_trim,
                    )
                    .translated(fragment.visual_offset());
                    for decoration in fragment.ancestor_inline_decorations() {
                        if let Some(source) = decoration.positioning_containing_block_id {
                            if fragment.excluded_positioning_geometry_source() == Some(source) {
                                continue;
                            }
                            let axes = WritingModeAxes::new(
                                decoration.style.writing_mode,
                                decoration.style.used_direction(),
                            );
                            prepared_inline_positioning_geometry_mut(
                                &mut positioning_geometry,
                                source,
                                axes,
                            )
                            .record_content_rect(PageTopRect::new(
                                fragment_rect.x(),
                                fragment_rect.y() + fragment_rect.height(),
                                fragment_rect.width(),
                                fragment_rect.height(),
                            ));
                        }
                    }
                    for decoration in fragment.ancestor_inline_decorations() {
                        if !decoration.paints_background_or_border {
                            continue;
                        }
                        let decoration_metrics = self
                            .inline_text_box_metrics(&decoration.style, fragment.baseline_shift());
                        let decoration_trim = self.inline_text_box_content_trim_for_style(
                            &decoration.style,
                            decoration_metrics,
                        );
                        let decoration_rect = trim_inline_content_rect(
                            line_geometry.visual_line_item_rect(
                                line_logical_inline_start,
                                line_physical_origin,
                                prepared_inline_position,
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
                    let enforce_space_paint_boundary =
                        preserve_first_line_paint_boundaries || pending_started_after_opaque_atom;
                    let collapsible_space_can_append =
                        pending_fragments.last().is_some_and(|previous| {
                            inline_fragment_can_append_collapsible_space(previous, &fragment)
                                && (!enforce_space_paint_boundary
                                    || can_paint_inline_fragments_together(previous, &fragment))
                        });
                    let pending_spaces_can_append =
                        pending_inline_fragments_are_collapsible_space(&pending_fragments)
                            && (!enforce_space_paint_boundary
                                || pending_fragments.iter().all(|pending| {
                                    can_paint_inline_fragments_together(pending, &fragment)
                                }));
                    let synthesize_pending_space_in_current_summary =
                        pending_preserve_leading_summary_space
                            && pending_inline_fragments_are_collapsible_space(&pending_fragments)
                            && !inline_fragment_is_collapsible_space(&fragment);
                    let can_append = leading_tracking == 0.0
                        && preserves_justification_policy
                        && pending_fragments.last().is_none_or(|previous| {
                            previous.link_target() == fragment.link_target()
                        })
                        && ((extra_space_width == 0.0
                            || justification_plan.justifies_inter_word())
                            && pending_fragments.last().is_some_and(|previous| {
                                can_queue_inline_fragments_for_shaping(previous, &fragment)
                            })
                            || collapsible_space_can_append
                            || pending_spaces_can_append);
                    if pending_fragments.is_empty() {
                        pending_synthesize_leading_summary_space = false;
                        pending_inline_position = prepared_inline_position;
                        pending_visual_offset = fragment.visual_offset();
                        pending_started_after_opaque_atom = previous_item_was_opaque_atom;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (preserve_first_line_paint_boundaries && inline_position > 0.0)
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    } else if !can_append {
                        if !synthesize_pending_space_in_current_summary
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
                                    pending_synthesize_leading_summary_space,
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
                                    pending_synthesize_leading_summary_space,
                                )
                            }
                        {
                            // The final visual group is the authoritative
                            // inline advance.  The selected line's measured
                            // slices can differ after bidi reordering and
                            // boundary shaping, so subsequent visual items
                            // must not retain their provisional source cursor.
                            inline_position = pending_inline_position + group.width();
                            Self::update_line_rendered_baseline_shift(
                                &mut line_rendered_baseline_shift,
                                line_layout_baseline_y,
                                &group,
                            );
                            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                        }
                        pending_fragments.clear();
                        pending_horizontal_content_bottom_y = None;
                        pending_inline_position = prepared_inline_position;
                        pending_visual_offset = fragment.visual_offset();
                        pending_started_after_opaque_atom = previous_item_was_opaque_atom;
                        pending_synthesize_leading_summary_space =
                            synthesize_pending_space_in_current_summary;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom
                            || (preserve_first_line_paint_boundaries && inline_position > 0.0)
                            || (item_index > 0 && !inline_fragment_is_collapsible_space(&fragment));
                    }
                    if pending_inline_fragments_are_join_control_only(&pending_fragments) {
                        pending_visual_offset = fragment.visual_offset();
                    }
                    if !pending_fragments.is_empty() {
                        pending_inline_position =
                            pending_inline_position.min(prepared_inline_position);
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
                    let inter_character_expansion_count =
                        justification_plan.inter_character_expansion_count_after_item(item_index);
                    let add_inter_character_gap = justification_plan.justifies_inter_character()
                        && inter_character_expansion_count > 0;
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
                                pending_synthesize_leading_summary_space,
                            ) {
                                inline_position = pending_inline_position + group.width();
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
                            pending_synthesize_leading_summary_space = false;
                        }
                        inline_position += if add_inter_character_gap {
                            extra_space_width * inter_character_expansion_count as f32
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
                    let boundary_before = measured_item.advance.boundary_before().points();
                    if (!preserves_pending_shaping || boundary_before != 0.0)
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
                                pending_synthesize_leading_summary_space,
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
                                pending_synthesize_leading_summary_space,
                            )
                        }
                    {
                        inline_position = pending_inline_position + group.width();
                        Self::update_line_rendered_baseline_shift(
                            &mut line_rendered_baseline_shift,
                            line_layout_baseline_y,
                            &group,
                        );
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    if !preserves_pending_shaping || boundary_before != 0.0 {
                        pending_fragments.clear();
                        pending_horizontal_content_bottom_y = None;
                        pending_visual_offset = InlineVisualOffset::zero();
                        pending_preserve_leading_summary_space = false;
                        pending_synthesize_leading_summary_space = false;
                    }
                    inline_position += boundary_before;
                    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.content()
                    {
                        let atom_metrics =
                            self.inline_text_box_metrics(atom.style(), atom.baseline_shift());
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
                            let paint_inline_start = visual_positions[item_index]
                                .map(PreparedInlineVisualPosition::points)
                                .unwrap_or(inline_position)
                                + inline_box_edge_paint_offset(*edge);
                            let border_box = trim_inline_content_rect(
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
                            if let Some(source) = edge.positioning_containing_block_id {
                                let axes = WritingModeAxes::new(
                                    atom.style().writing_mode,
                                    atom.style().used_direction(),
                                );
                                prepared_inline_positioning_geometry_mut(
                                    &mut positioning_geometry,
                                    source,
                                    axes,
                                )
                                .record_marker(
                                    edge.logical_edge,
                                    PageTopRect::new(
                                        border_box.x(),
                                        border_box.y() + border_box.height(),
                                        border_box.width(),
                                        border_box.height(),
                                    ),
                                );
                            }
                            paint_items.push(PreparedInlinePaintItem::Atom(PreparedInlineAtom {
                                atom: atom.clone(),
                                border_box,
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
                            InlineAtomContent::StaticPositionHypothetical { .. }
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
                    let border_box = line_geometry
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
                        border_box,
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
                    if measured_item.used_advance().points() > 0.0 {
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
                                pending_synthesize_leading_summary_space,
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
                                pending_synthesize_leading_summary_space,
                            )
                        } {
                            inline_position = pending_inline_position + group.width();
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
                    inline_position += measured_item.used_advance().points();
                    if measured_item.used_advance().points() > 0.0 {
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
                pending_synthesize_leading_summary_space,
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
                pending_synthesize_leading_summary_space,
            )
        } {
            Self::update_line_rendered_baseline_shift(
                &mut line_rendered_baseline_shift,
                line_layout_baseline_y,
                &group,
            );
            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
        }
        let paint_items = first_line_effect_scoped_paint_items(
            paint_items,
            context
                .is_first_line
                .then_some(block_style.first_line_style.as_deref())
                .flatten()
                .map(|style| style.opacity),
        );
        Some(PreparedInlineLine {
            metrics: line_metrics,
            paint_items,
            positioning_geometry,
            decoration_origin_fragments: Rc::default(),
        })
    }
}

fn prepared_inline_positioning_geometry_mut(
    geometries: &mut Vec<PreparedInlinePositioningGeometry>,
    source: InlinePositioningContainingBlockId,
    axes: WritingModeAxes,
) -> &mut PreparedInlinePositioningGeometry {
    if let Some(index) = geometries
        .iter()
        .position(|geometry| geometry.source == source)
    {
        debug_assert_eq!(geometries[index].axes, axes);
        return &mut geometries[index];
    }
    geometries.push(PreparedInlinePositioningGeometry::new(source, axes));
    geometries
        .last_mut()
        .expect("a positioning geometry record was just appended")
}

/// Materialize the generated `::first-line` box as one prepared paint subtree.
///
/// Opacity is non-inherited and applies after the pseudo-element and all of
/// its descendants have painted. CSS Overflow's block ellipsis is a sibling
/// anonymous inline inserted after first-line selection, so it deliberately
/// remains outside this scope even when bidi places it at the visual start.
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-styling>
/// <https://drafts.csswg.org/css-color-4/#transparency>
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
fn first_line_effect_scoped_paint_items(
    paint_items: Vec<PreparedInlinePaintItem>,
    opacity: Option<css::Opacity>,
) -> Vec<PreparedInlinePaintItem> {
    let Some(opacity) = opacity.filter(|opacity| opacity.value() < 1.0) else {
        return paint_items;
    };
    let mut first_line_items = Vec::with_capacity(paint_items.len());
    let mut following_anonymous_items = Vec::new();
    for item in paint_items {
        if prepared_paint_item_is_block_ellipsis(&item) {
            following_anonymous_items.push(item);
        } else {
            first_line_items.push(item);
        }
    }
    if first_line_items.is_empty() {
        return following_anonymous_items;
    }
    let mut scoped = Vec::with_capacity(1 + following_anonymous_items.len());
    scoped.push(PreparedInlinePaintItem::Scope(Box::new(
        PreparedInlinePaintScope {
            kind: PreparedInlinePaintScopeKind::FirstLine,
            opacity,
            items: first_line_items,
        },
    )));
    scoped.extend(following_anonymous_items);
    scoped
}

fn prepared_paint_item_is_block_ellipsis(item: &PreparedInlinePaintItem) -> bool {
    match item {
        PreparedInlinePaintItem::FragmentBackground(fragment) => {
            matches!(fragment.fragment.source(), InlineTextSource::BlockEllipsis)
        }
        PreparedInlinePaintItem::TextGroup(group) => {
            matches!(group.source, InlineTextSource::BlockEllipsis)
        }
        PreparedInlinePaintItem::Atom(_) => false,
        PreparedInlinePaintItem::Scope(scope) => scope
            .items
            .iter()
            .all(prepared_paint_item_is_block_ellipsis),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_fragment(
        text: &str,
        width: f32,
        direction: ResolvedBidiDirection,
    ) -> MeasuredInlineItem {
        let mut fragment = InlineFragment::new(
            text,
            ComputedStyle::initial(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        fragment.set_resolved_bidi_direction(Some(direction));
        MeasuredInlineItem::new(InlineLineItem::Fragment(fragment), width, None)
    }

    fn starts(
        writing_mode: WritingMode,
        paragraph_direction: Direction,
        run_direction: ResolvedBidiDirection,
    ) -> Vec<Option<f32>> {
        let mut style = ComputedStyle::initial();
        style.writing_mode = writing_mode;
        prepared_inline_visual_positions(
            &[
                measured_fragment("aa", 20.0, run_direction),
                measured_fragment("bbb", 30.0, run_direction),
            ],
            &style,
            paragraph_direction,
            layout_pt(0.0),
            &[layout_pt(0.0), layout_pt(0.0)],
        )
        .into_iter()
        .map(|position| position.map(PreparedInlineVisualPosition::points))
        .collect()
    }

    #[test]
    fn opposite_direction_vertical_run_places_source_fragments_from_opposite_edge() {
        for writing_mode in [
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            assert_eq!(
                starts(writing_mode, Direction::Rtl, ResolvedBidiDirection::Ltr),
                vec![Some(30.0), Some(0.0)],
                "{writing_mode:?}"
            );
            assert_eq!(
                starts(writing_mode, Direction::Ltr, ResolvedBidiDirection::Rtl),
                vec![Some(30.0), Some(0.0)],
                "{writing_mode:?}"
            );
        }
    }

    #[test]
    fn same_direction_and_horizontal_runs_keep_visual_sequence_positions() {
        assert_eq!(
            starts(
                WritingMode::VerticalRl,
                Direction::Rtl,
                ResolvedBidiDirection::Rtl
            ),
            vec![None, None]
        );
        assert_eq!(
            starts(
                WritingMode::HorizontalTb,
                Direction::Rtl,
                ResolvedBidiDirection::Ltr
            ),
            vec![None, None]
        );
    }
}
