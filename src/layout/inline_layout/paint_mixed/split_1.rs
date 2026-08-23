use std::rc::Rc;

use super::super::items::InlineLineSequenceSlice;
use super::*;
use crate::layout::assets::{
    ReplacedObjectOverflow, apply_object_fit, native_generated_gradient_primitive,
    raster_image_sampling, replaced_content_contour, svg_replaced_group_with_overflow_clip,
};
use crate::layout::text_paint::{
    TextDecorationLineGeometry, TextDecorationLineGlyphCoverage, TextDecorationLineGlyphSequence,
    TextDecorationLineKind, TextDecorationOriginFragmentGeometry, TextDecorationOriginLineGeometry,
    TextDecorationStrokeAxis, TextInlineSpan, VerticalInlineAxis,
    positioned_rendered_runs_for_writing_mode, text_decoration_positioned_glyphs,
    text_decoration_skip_self_suppresses,
};

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
        // Atomic inline baseline metrics are expressed from their logical
        // margin-box start, while captured atom contents replay from their
        // border box. Align the shared line anchor with that same margin-box
        // reference before positioning text and atoms. Line-relative atoms
        // and inline-table wrappers opt out in the shared helper because
        // their placement owns the margin at a different boundary.
        // <https://drafts.csswg.org/css-inline-3/#line-layout>
        let line_block_start_margin = inline_line_anchor_block_start_margin(&line, block_style);
        let line_top = if block_style.writing_mode == WritingMode::HorizontalTb {
            self.cursor_y - line_block_start_margin
        } else {
            self.cursor_y
        };
        let mut line_geometry = InlineLineGeometry::new(
            self.content_left,
            self.content_right,
            line_top,
            context.line_block_size,
            context,
        );
        line_geometry.text_box_line_trim = line_fragment.text_box_trim;
        if block_style.writing_mode.has_vertical_lines() {
            line_geometry.apply_logical_block_start_margin(line_block_start_margin);
        }
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
        let mut pending_horizontal_content_bottom_y = None;
        let mut previous_item_was_opaque_atom = false;
        let mut paint_items = Vec::new();
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
                        && pending_fragments.last().is_none_or(|previous| {
                            previous.link_target() == fragment.link_target()
                        })
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
                    }
                    inline_position += boundary_before;
                    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.content()
                    {
                        let atom_metrics =
                            self.inline_text_box_metrics(atom.style(), atom.baseline_shift);
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
            decoration_origin_fragments: Rc::default(),
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
        let mut phaseable_text_groups = Vec::new();
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.rect.paint_rect(),
                    );
                }
                PreparedInlinePaintItem::TextGroup(group) => {
                    let has_paint_effect = if group.paint_scope_ancestry.is_empty() {
                        group.style.opacity.value() < 1.0
                    } else {
                        group.paint_opacity < 1.0
                    } || group.positioned_paint_style.is_some();
                    if has_paint_effect {
                        if !phaseable_text_groups.is_empty() {
                            self.paint_prepared_inline_text_groups_in_phases(
                                &phaseable_text_groups,
                                text_source,
                                &decoration_geometries,
                            );
                            phaseable_text_groups.clear();
                        }
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
                    } else {
                        phaseable_text_groups.push(group);
                    }
                }
                PreparedInlinePaintItem::Atom(atom) => {
                    if !phaseable_text_groups.is_empty() {
                        self.paint_prepared_inline_text_groups_in_phases(
                            &phaseable_text_groups,
                            text_source,
                            &decoration_geometries,
                        );
                        phaseable_text_groups.clear();
                    }
                    self.paint_prepared_inline_atom(atom);
                }
            }
        }
        if !phaseable_text_groups.is_empty() {
            self.paint_prepared_inline_text_groups_in_phases(
                &phaseable_text_groups,
                text_source,
                &decoration_geometries,
            );
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
            // A group can retain its shaped glyph advance while its fitted
            // paint bounds collapse at an inline-fragment boundary. Line
            // decorations still cover that text, so use the shaped advance
            // as the non-empty geometry criterion and coverage span.
            if group.width().max(group.shaped.advance_width()) <= 0.0 {
                continue;
            }
            if group.decoration_provenance.is_empty() {
                continue;
            }
            let mut reference = group
                .decoration_paint_rect
                .map(|rect| rect.origin)
                .unwrap_or_else(|| PaintPoint::new(group.x(), group.y()));
            if let (Some(rect), Some(inline_axis)) = (
                group.decoration_paint_rect,
                VerticalInlineAxis::for_style(&group.style),
            ) {
                reference.y = inline_axis.logical_start_for_paint_rect(rect).y();
            }
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
            for provenance in &group.decoration_provenance {
                for receiver in &provenance.receivers {
                    if receiver.style.visibility != Visibility::Visible {
                        continue;
                    }
                    let coverage = TextDecorationLineGlyphCoverage {
                        span: match receiver.style.writing_mode {
                            WritingMode::HorizontalTb => TextInlineSpan::new(
                                reference.x + receiver.inline_span.start,
                                reference.x + receiver.inline_span.end,
                            ),
                            WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr => {
                                VerticalInlineAxis::for_style(&receiver.style)
                                    .expect("vertical writing modes have a vertical inline axis")
                                    .project_span_from_start(
                                        layout_pt(reference.y),
                                        receiver.inline_span,
                                    )
                            }
                        },
                    };
                    let positioned_glyphs = text_decoration_positioned_glyphs(
                        axis,
                        reference.x,
                        reference.y,
                        coverage.span.start,
                        coverage.span.length(),
                        &glyph_runs,
                    );
                    let font_id = self.font_system.resolve_style(&receiver.style);
                    let metrics = self
                        .font_system
                        .text_decoration_metrics(font_id, &receiver.style);
                    for decoration in &provenance.layers {
                        let participates = (decoration.decoration.underline
                            && !text_decoration_skip_self_suppresses(
                                &receiver.style,
                                TextDecorationLineKind::Underline,
                            ))
                            || (decoration.decoration.overline
                                && !text_decoration_skip_self_suppresses(
                                    &receiver.style,
                                    TextDecorationLineKind::Overline,
                                ))
                            || (decoration.decoration.line_through
                                && !text_decoration_skip_self_suppresses(
                                    &receiver.style,
                                    TextDecorationLineKind::LineThrough,
                                ));
                        if !participates {
                            continue;
                        }
                        if let Some(existing) = geometries.iter_mut().find(|existing| {
                            Rc::ptr_eq(&existing.layer.origin_style, &decoration.origin_style)
                        }) {
                            // The selected text with the largest em box is the
                            // conservative shared metric source: it keeps automatic
                            // decorations clear of every eligible descendant rather
                            // than letting a later, smaller receiver pull the common
                            // line through it.  The physical outside reference is
                            // likewise the furthest text-under edge of the selected
                            // line in each writing-axis projection.
                            if receiver.style.font_size > existing.geometry.considered_font_size {
                                existing.geometry.considered_font_size = receiver.style.font_size;
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
                            existing.receiver_spans.push(coverage.span);
                            existing
                                .glyph_sequence
                                .glyphs
                                .extend(positioned_glyphs.iter().cloned());
                            match receiver.style.writing_mode {
                                WritingMode::HorizontalTb => {
                                    existing.line_reference.y =
                                        existing.line_reference.y.min(reference.y);
                                }
                                WritingMode::VerticalRl
                                | WritingMode::VerticalLr
                                | WritingMode::SidewaysRl
                                | WritingMode::SidewaysLr => {
                                    existing.line_reference.x =
                                        existing.line_reference.x.min(reference.x);
                                }
                            }
                            continue;
                        }
                        geometries.push(TextDecorationOriginLineGeometry {
                            layer: decoration.clone(),
                            geometry: TextDecorationLineGeometry::from_origin_and_considered_text(
                                decoration.origin_style.as_ref(),
                                &receiver.style,
                                metrics,
                            ),
                            origin_inline_axis: VerticalInlineAxis::for_style(
                                decoration.origin_style.as_ref(),
                            ),
                            selected_inline_span: Some(coverage.span),
                            receiver_spans: vec![coverage.span],
                            glyph_sequence: TextDecorationLineGlyphSequence {
                                glyphs: positioned_glyphs.clone(),
                            },
                            line_reference: reference,
                            origin_fragment: line
                                .decoration_origin_fragments
                                .iter()
                                .find(|fragment| {
                                    Rc::ptr_eq(&fragment.origin_style, &decoration.origin_style)
                                })
                                .cloned()
                                // Direct record preparation is also used by a
                                // few isolated layout tests. Those records do
                                // not have an enclosing line sequence from
                                // which to derive fragment geometry.
                                .unwrap_or_else(|| TextDecorationOriginFragmentGeometry {
                                    origin_style: Rc::clone(&decoration.origin_style),
                                    total_inline_extent: layout_pt(coverage.span.length()),
                                    fragment_inline_extent: layout_pt(coverage.span.length()),
                                    preceding_inline_extent: layout_pt(0.0),
                                    following_inline_extent: layout_pt(0.0),
                                    is_first_fragment: true,
                                    is_last_fragment: true,
                                }),
                        });
                    }
                }
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
            let text_top = prepared.border_box.y() + prepared.border_box.height()
                - borders.top
                - atom.style().padding.top;
            let formatted_line_block_start = PageTopBlockPosition::new(text_top);
            let fallback_baseline_offset =
                self.inline_box_text_line_layout_baseline_offset(atom.style());
            self.paint_outside_marker(
                marker,
                atom.style(),
                OutsideMarkerAnchor {
                    principal_line_inline_span: PageInlineSpan::from_edges(
                        prepared.border_box.x() + borders.left,
                        prepared.border_box.x() + prepared.border_box.width() - borders.right,
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
            let bounds = prepared.border_box.paint_clip();
            let mut policy = match atom.content() {
                // SVG 2 requires an embedded outermost SVG element to be an
                // isolated, atomic stacking context.  Its box decorations
                // and rendered SVG scene have already been recorded in this
                // fragment in source paint order; the policy gives that
                // semantic group a single compositing boundary.
                // <https://www.w3.org/TR/SVG2/render.html#EstablishingANewStackingContext>
                InlineAtomContent::Svg { .. } => StackingContextPolicy::for_inline_svg_root(
                    atom.style(),
                    PaintBand::Inline,
                    bounds,
                ),
                // A non-atomic inline's `InlineBox` atom is only a retained
                // line-layout sequence. Do not give it the atomic replay
                // policy: that would retain a negative positioned descendant
                // inside the inline's own background fragment.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                InlineAtomContent::InlineBox { .. }
                    if !property_containment_applies_to_style(atom.style()) =>
                {
                    StackingContextPolicy::for_non_positioned_style_effect(atom.style(), bounds)
                }
                _ => StackingContextPolicy::for_atomic(atom.style(), PaintBand::Inline, bounds),
            };
            // A replaced atom's CSS content clip is attached directly to its
            // image/SVG primitive.  The atomic context also contains the
            // principal decoration, so a generic padding-box overflow effect
            // here would both clip that decoration and introduce a duplicate
            // rectangular raster edge before the exact contour. Captured
            // formatting contexts, including inline tables, do not have that
            // primitive-owned contour and must retain their own CSS overflow
            // clip around the replayed descendant fragment.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            if matches!(
                atom.content(),
                InlineAtomContent::Canvas
                    | InlineAtomContent::Iframe(_)
                    | InlineAtomContent::Image(_)
                    | InlineAtomContent::Gradient { .. }
                    | InlineAtomContent::Svg { .. }
                    | InlineAtomContent::InlineFragment {
                        contents_overflow_clip_applied: true,
                        ..
                    }
            ) {
                policy.effects.clear_overflow_clip_effects();
            }
            // Preserve the atom's original stack level through this atomic
            // replay boundary.  `escaped_positioned_layers` later extracts
            // such contexts and uses their level to insert them into the
            // nearest real parent stacking context; rebuilding it as `auto`
            // would incorrectly move a negative descendant above the
            // parent's in-flow inline paint.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                Vec::new(),
            )
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
                    .atom_offset(prepared.border_box.x(), prepared.border_box.y());
                let mut layer = layer.clone().translated(atom_offset);
                layer.page_index = layer
                    .escaped_atom_translation
                    .replay_page_index(self.pages.len(), layer.page_index);
                // The atomic inline was measured on a scratch page, so a
                // descendant's original source order predates the atom's
                // final normal-flow paint.  Its containing block is a
                // `z-index:auto` positioned inline-block, which means the
                // descendant belongs in the enclosing stacking context's
                // auto/zero phase after that normal-flow paint.  Reserve its
                // order at replay, rather than retaining the scratch cursor.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                layer.context.source_order = self.next_paint_source_order();
                self.positioned_layers.push(layer);
            }
        }
    }

    fn paint_inline_atom_box_background(&mut self, border_rect: PaintRect, style: &ComputedStyle) {
        for primitive in self.box_background_primitives(border_rect, style) {
            // The atomic inline context itself is inserted in its parent's
            // inline phase. Within that context, its own decoration remains
            // before its in-flow block descendants.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }
    }

    pub(in crate::layout) fn paint_prepared_inline_atom_contents(
        &mut self,
        prepared: &PreparedInlineAtom,
    ) {
        let atom = &prepared.atom;
        let content_x = prepared.border_box.x();
        let y = prepared.border_box.y();
        let content_width = prepared.border_box.width();
        let content_height = prepared.border_box.height();
        if !matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) && (atom
            .style()
            .background
            .background_color
            .is_potentially_visible()
            || atom.style().background.background_image.is_image()
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
                fragment.promote_outline_to_in_flow_outline();
                // An embedded page background is still child browsing-context
                // paint. Keep it in the iframe viewport along with the
                // translated child scroll contents.
                // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
                fragment = fragment.with_effect_scoped_to_rect_all_bands(clip);
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
            InlineAtomContent::Image(decoded) => {
                let borders = used_border_widths(atom.style());
                let overflow = ReplacedObjectOverflow::from_style(atom.style());
                let content_contour = replaced_content_contour(
                    paint_space_rect(content_x, y, content_width, content_height),
                    atom.style(),
                    borders,
                );
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
                    decoded.pixel_size.width,
                    decoded.pixel_size.height,
                    decoded.source_rect,
                    raster_image_sampling(atom.style()),
                    decoded.rgb.shared(),
                    decoded.alpha.clone(),
                    atom.alt_text().map(Rc::from),
                )
                .with_raster_color_space(decoded.color_space.clone())
                .with_image_id(decoded.image_id);
                if let Some(clip) = content_contour
                    .as_ref()
                    .and_then(ResolvedBoxContentClip::path_clip)
                {
                    image = image.with_clip(clip);
                }
                if apply_object_fit(
                    &mut image,
                    decoded.natural_layout_size(),
                    atom.style().object_fit,
                    atom.style().object_position.clone(),
                    atom.style().object_view_box.clone(),
                    overflow,
                    atom.style().effective_zoom,
                ) {
                    self.push_image_in_band(PaintBand::Inline, image);
                }
            }
            InlineAtomContent::Gradient { image, fallback } => {
                let borders = used_border_widths(atom.style());
                let overflow = ReplacedObjectOverflow::from_style(atom.style());
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
                        fallback.pixel_size.width,
                        fallback.pixel_size.height,
                        fallback.source_rect,
                        raster_image_sampling(atom.style()),
                        fallback.rgb.shared(),
                        fallback.alpha.clone(),
                        atom.alt_text().map(Rc::from),
                    )
                    .with_raster_color_space(fallback.color_space.clone())
                    .with_image_id(fallback.image_id);
                    if apply_object_fit(
                        &mut rendered,
                        fallback.natural_layout_size(),
                        atom.style().object_fit,
                        atom.style().object_position.clone(),
                        atom.style().object_view_box.clone(),
                        overflow,
                        atom.style().effective_zoom,
                    ) {
                        self.push_image_in_band(PaintBand::Inline, rendered);
                    }
                }
            }
            InlineAtomContent::Svg { asset } => {
                if let Some(asset) = asset {
                    let borders = used_border_widths(atom.style());
                    let border_rect = paint_space_rect(content_x, y, content_width, content_height);
                    let overflow_edge = resolve_overflow_clip_edge(
                        border_rect,
                        atom.style(),
                        borders,
                        UsedOverflowAxes::from_svg_viewport_style(atom.style()),
                        atom.style().contain.paint,
                        None,
                    );
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
                    // selection path as block replaced SVG. Unlike an image
                    // asset, however, an embedded SVG's root viewport obeys
                    // the element's computed CSS overflow.
                    if svg_width > 0.0 && svg_height > 0.0 {
                        let group = svg_replaced_group_with_overflow_clip(
                            asset,
                            paint_space_rect(svg_x, svg_y, svg_width, svg_height),
                            atom.style().object_fit,
                            atom.style().object_position.clone(),
                            atom.style().object_view_box.clone(),
                            overflow_edge.as_ref(),
                        );
                        self.push_svg_group_in_band(PaintBand::Inline, group);
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
                let resolved_placement = atom.ruby_placement();
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
                let base_x = resolved_placement.map_or_else(
                    || {
                        ruby_origin.inline()
                            + (text_available_width - base.paint_inline_size.points()).max(0.0)
                                / 2.0
                    },
                    |placement| ruby_origin.inline() + placement.base_inline_offset.points(),
                );
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
                for (annotation_index, ((annotation, annotation_block_size), side)) in annotations
                    .iter()
                    .zip(annotation_block_sizes)
                    .zip(annotation_sides)
                    .enumerate()
                {
                    let annotation_available_width =
                        if annotation.starts_span && annotation.column_span > 1 {
                            annotation.containing_inline_size.points()
                        } else {
                            text_available_width
                        };
                    let annotation_x = resolved_placement.map_or_else(
                        || {
                            ruby_origin.inline()
                                + (annotation_available_width
                                    - annotation.paint_inline_size.points())
                                .max(0.0)
                                    / 2.0
                        },
                        |placement| {
                            ruby_origin.inline()
                                + placement
                                    .annotation_inline_offsets
                                    .get(annotation_index)
                                    .copied()
                                    .unwrap_or(ruby::RubyInlineDisplacement::ZERO)
                                    .points()
                        },
                    );
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
                    let annotation_block_top = annotation_baseline + annotation_line_box_baseline;
                    // A ruby-text container owns a generated principal box;
                    // its child sequence retains only descendant fragments.
                    // Paint its decoration even when no direct text fragment
                    // exists, such as an `rt` containing a positioned span.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-text-container>
                    let annotation_height = annotation_block_size
                        .max(annotation.style.line_height)
                        .max(0.0);
                    let annotation_width = annotation.paint_inline_size.points().max(0.0);
                    if annotation.style.visibility == Visibility::Visible
                        && annotation_width > 0.0
                        && annotation_height > 0.0
                    {
                        for primitive in self.box_background_primitives(
                            paint_space_rect(
                                annotation_x,
                                annotation_block_top - annotation_height,
                                annotation_width,
                                annotation_height,
                            ),
                            &annotation.style,
                        ) {
                            self.push_primitive_in_band(PaintBand::Inline, primitive);
                        }
                    }
                    self.paint_inline_box_sequence(
                        &annotation.sequence,
                        &annotation.style,
                        annotation_x,
                        annotation_available_width,
                        annotation_block_top,
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
                    prepared.border_box,
                );
            }
            InlineAtomContent::InlineFragment {
                fragment,
                replay_coordinates,
                table_cell_context,
                ..
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
                self.current_page.append_paint_fragment(
                    fragment,
                    replay_coordinates.replay_translation(prepared.border_box),
                );
            }
        }
        for primitive in self.box_outline_primitives(
            paint_space_rect(content_x, y, content_width, content_height),
            atom.style(),
        ) {
            self.push_primitive_in_band(PaintBand::InFlowOutline, primitive);
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
    /// keeps glyphs, shadows, decorations, and links in one normal paint
    /// subtree rather than applying disconnected per-glyph offsets. The
    /// measured square limits layout only: CSS permits glyph ink to extend
    /// outside it, so this replay deliberately establishes no overflow clip.
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
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                transform: Some(transform),
                ..PaintEffects::default()
            });
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
        self.paint_inline_box_sequence_with_float_policy(
            sequence,
            style,
            content_left,
            available_width,
            block_top,
            NestedInlinePaintFloatPolicy::ReapplyActiveFloatBands,
        );
    }

    pub(in crate::layout) fn paint_inline_box_sequence_with_float_policy(
        &mut self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
        float_policy: NestedInlinePaintFloatPolicy,
    ) {
        // This is a nested paint replay. Its selected lines may establish an
        // internal baseline (for example a ruby base or annotation), but
        // that baseline is not a line of the enclosing formatting context
        // and therefore must not escape through inline-block baseline export.
        // <https://drafts.csswg.org/css-inline-3/#baseline-layout>
        // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_last_in_flow_line_baseline_y = self.last_in_flow_line_baseline_y;
        self.content_left = content_left;
        self.content_right = content_left + available_width;
        self.cursor_y = block_top;
        self.paint_inline_line_sequence_slice_with_text_source(
            sequence,
            style,
            InlineLineSequenceSlice {
                block_top,
                top: block_top,
                bottom: f32::NEG_INFINITY,
            },
            RenderedLineSource::InlineAtom,
            float_policy,
        );
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
        self.last_in_flow_line_baseline_y = saved_last_in_flow_line_baseline_y;
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
        if edge.paint_extent <= 0.0 || prepared.border_box.height() <= 0.0 {
            return;
        }
        let mut style = prepared.atom.style().clone();
        if let Some(color) = prepared.atom.current_color_override() {
            style.color = color;
        }
        apply_inline_box_edge_paint_style(&mut style, edge);
        if style.background.background_color.is_transparent()
            && style.background.background_image.is_none()
            && used_border_width(&style) == layout_pt(0.0)
        {
            return;
        }
        for primitive in self.box_background_primitives(
            paint_space_rect(
                prepared.border_box.x(),
                prepared.border_box.y(),
                prepared.border_box.width(),
                prepared.border_box.height(),
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
