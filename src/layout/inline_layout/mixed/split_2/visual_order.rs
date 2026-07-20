use super::*;
use crate::css::{Edges, LineFitEdge, TextBoxTrim};
use crate::text::line_end_letter_spacing_width;
use std::rc::Rc;

fn inline_fragment_uses_text_edge_layout(fragment: &InlineFragment) -> bool {
    !matches!(fragment.style().text_box_trim, TextBoxTrim::None)
        || !matches!(fragment.style().line_fit_edge, LineFitEdge::Leading)
}

/// Return whether an inline atom is a zero-width, non-isolating boundary for
/// logical cursive shaping.
///
/// Plain inline element edges do not separate CSS Text typographic character
/// units. Decoration in the inline axis and bidi isolation deliberately do:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn inline_atom_is_transparent_to_logical_shaping(atom: &InlineAtom) -> bool {
    matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
        if edge.advance == 0.0
            && edge.paint_extent == 0.0
            && !inline_box_edge_breaks_shaping(atom.style())
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}

fn inline_fragment_block_axis_outer_extras(
    style: &ComputedStyle,
    include_margin_border_padding: bool,
) -> (f32, f32) {
    if !include_margin_border_padding {
        return (0.0, 0.0);
    }
    let borders = used_border_widths(style);
    let block_start = block_start_side(style.writing_mode);
    let block_end = block_end_side(style.writing_mode);
    // CSS Inline `line-fit-edge` uses the margin-box bounds of non-root inline boxes:
    // <https://drafts.csswg.org/css-inline-3/#line-fit-edge-property>.
    (
        physical_edge_value(style.margin, block_start)
            + physical_edge_value(style.padding, block_start)
            + physical_edge_value(borders, block_start),
        physical_edge_value(style.margin, block_end)
            + physical_edge_value(style.padding, block_end)
            + physical_edge_value(borders, block_end),
    )
}

fn physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}

/// CSS metrics for one non-replaced inline text box.
///
/// CSS 2.2 separates the inline content area from the line-height box:
/// backgrounds, borders, and padding are anchored to the content area, while
/// only `line-height` contributes to line box sizing. The content-area height
/// is intentionally undefined by CSS 2.2; Quire uses its existing em-box
/// policy and resolves block-start/block-end leading separately because
/// fallback-font unions need not be symmetric around the content box:
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineTextBoxMetrics {
    pub(in crate::layout) content_block_size: f32,
    pub(in crate::layout) content_baseline_offset: f32,
    pub(in crate::layout) line_block_size: f32,
    pub(in crate::layout) block_start_leading: f32,
    pub(in crate::layout) block_end_leading: f32,
    pub(in crate::layout) line_baseline_offset: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineBaselineExtents {
    baseline_offset: f32,
    descent: f32,
}

impl InlineBaselineExtents {
    fn new(baseline_offset: f32, descent: f32) -> Self {
        Self {
            baseline_offset,
            descent,
        }
    }

    fn from_baseline_and_block_size(baseline_offset: f32, block_size: f32) -> Self {
        Self::new(baseline_offset, block_size - baseline_offset)
    }

    /// Return signed extents for a baseline-aligned box after CSS baseline shifting.
    ///
    /// CSS Inline Layout applies `<length-percentage>` `baseline-shift` after
    /// baseline-table alignment. Positive shifts raise the aligned subtree, so
    /// they increase the line-over extent and reduce the line-under extent:
    /// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
    fn from_shifted_baseline_and_block_size(
        baseline_offset: f32,
        block_size: f32,
        baseline_shift: f32,
    ) -> Self {
        Self::new(
            baseline_offset + baseline_shift,
            block_size - baseline_offset - baseline_shift,
        )
    }

    fn height(self) -> f32 {
        (self.baseline_offset + self.descent).max(0.0)
    }

    fn union(self, other: Self) -> Self {
        Self {
            baseline_offset: self.baseline_offset.max(other.baseline_offset),
            descent: self.descent.max(other.descent),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Retain logical source shaping for a joining run before UAX #9 splits it
    /// into visual fragments.
    ///
    /// CSS Text shapes a typographic character unit in logical order, then
    /// the bidi algorithm selects visual line fragments. Re-shaping those
    /// fragments independently changes Arabic joining forms at transparent
    /// inline boundaries. This deliberately applies only to joining scripts:
    /// non-joining ligatures are already shaped as one paint-preparation
    /// group, while a source slice through a ligature cluster cannot be
    /// represented by separate inline fragments.
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>.
    fn preserve_logical_joining_source_shapes(
        &mut self,
        items: &[MeasuredInlineItem],
        tab_metric_style: &ComputedStyle,
    ) -> Vec<MeasuredInlineItem> {
        let mut output = Vec::with_capacity(items.len());
        let mut index = 0;

        while index < items.len() {
            let InlineLineItem::Fragment(_) = &items[index].item else {
                output.push(items[index].clone());
                index += 1;
                continue;
            };

            let start = index;
            let mut fragment_indices = vec![index];
            index += 1;
            while let Some(item) = items.get(index) {
                match &item.item {
                    InlineLineItem::Fragment(right) => {
                        let InlineLineItem::Fragment(left) =
                            &items[*fragment_indices.last().expect("first fragment index")].item
                        else {
                            unreachable!("fragment index always names a fragment");
                        };
                        if !can_shape_inline_fragments_together(left, right) {
                            break;
                        }
                        fragment_indices.push(index);
                        index += 1;
                    }
                    InlineLineItem::Atom(atom)
                        if inline_atom_is_transparent_to_logical_shaping(atom) =>
                    {
                        index += 1;
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => break,
                }
            }

            let joins = fragment_indices.iter().any(|&fragment_index| {
                match &items[fragment_index].item {
                    InlineLineItem::Fragment(fragment) => {
                        fragment.text().chars().any(|character| {
                            // An authored join-control-only span is a shaping
                            // boundary participant in its own right. It must
                            // keep adjacent text in one group even when neither
                            // visible neighbor has Arabic/Syriac-style joining
                            // behavior (for example `A<span>ZWNJ</span>B`).
                            character_is_join_control(character)
                                || character_has_joining_behavior(character)
                        })
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
                }
            });
            if fragment_indices.len() < 2 || !joins {
                output.extend_from_slice(&items[start..index]);
                continue;
            }

            let mut spans = Vec::with_capacity(fragment_indices.len());
            let mut text = String::new();
            let mut ranges = Vec::with_capacity(fragment_indices.len());
            let mut unshaped_width = 0.0;
            let mut line_height = None;
            let InlineLineItem::Fragment(first_fragment) = &items[fragment_indices[0]].item else {
                unreachable!("the fragment grouping loop only yields fragments");
            };
            let source_style = first_fragment.style();
            // CSS Text has already applied `text-transform` before shaping,
            // and `display` creates a transparent inline boundary. If those
            // are the only style differences, use one Parley text style so a
            // ligature such as lam-alef remains eligible across the element
            // edge.
            let one_text_style = fragment_indices.iter().all(|&fragment_index| {
                let InlineLineItem::Fragment(fragment) = &items[fragment_index].item else {
                    return false;
                };
                styles_have_equivalent_text_shaping_inputs(source_style, fragment.style())
            });
            for &fragment_index in &fragment_indices {
                let item = &items[fragment_index];
                let InlineLineItem::Fragment(fragment) = &item.item else {
                    unreachable!("the fragment grouping loop only yields fragments");
                };
                line_height.get_or_insert(fragment.style().line_height);
                let start = text.len();
                text.push_str(fragment.text());
                ranges.push(start..text.len());
                spans.push(StyledTextSpan {
                    text: fragment.text(),
                    style: if one_text_style {
                        source_style
                    } else {
                        fragment.style()
                    },
                });
                unshaped_width += item.width;
            }
            let Some(shaped) = self.font_system.shape_styled_inline_fragments(
                &spans,
                text,
                unshaped_width,
                line_height.expect("a fragment group always has a first style"),
                0.0,
                tab_metric_style,
            ) else {
                output.extend_from_slice(&items[start..index]);
                continue;
            };
            let shaped = Rc::new(shaped);
            let slices: Option<Vec<_>> = ranges
                .into_iter()
                .map(|range| shaped.source_slice(range).map(Rc::new))
                .collect();
            let Some(slices) = slices else {
                output.extend_from_slice(&items[start..index]);
                continue;
            };

            let mut shaped_fragments = slices.into_iter();
            for item in &items[start..index] {
                if let InlineLineItem::Fragment(fragment) = &item.item {
                    let shaped = shaped_fragments
                        .next()
                        .expect("every logical shaping fragment has one slice");
                    let mut fragment = fragment.clone();
                    fragment.set_preserves_source_shaping(true);
                    output.push(MeasuredInlineItem {
                        item: InlineLineItem::Fragment(fragment),
                        width: shaped.advance_width(),
                        shaped: Some(shaped),
                    });
                } else {
                    output.push(item.clone());
                }
            }
        }

        output
    }

    /// Return mixed inline line items in UBA visual order.
    ///
    /// CSS Writing Modes applies the Unicode Bidirectional Algorithm to inline
    /// content. CSS 2.2 inline boxes contribute transparent start/end edge
    /// decoration around text, while real atomic inline boxes participate in
    /// UAX #9 as object replacement characters and paint as indivisible
    /// inline-level boxes in the resolved visual order:
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>, and
    /// <https://www.unicode.org/reports/tr9/#L1>.
    pub(in crate::layout) fn visual_ordered_mixed_inline_line_items(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
        bidi_scope_continuations: &BidiLineScopeContinuations,
    ) -> Vec<MeasuredInlineItem> {
        let logical_items = self.preserve_logical_joining_source_shapes(items, block_style);
        let items = &logical_items;
        let bidi_scope_continuations =
            bidi_scope_continuations_after_selected_controls(items, bidi_scope_continuations);
        let line_has_bidi_scope_controls = items.iter().any(|item| {
            matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.text().chars().any(character_is_bidi_format_control))
        }) || !bidi_scope_continuations.prefix.is_empty()
            || !bidi_scope_continuations.suffix.is_empty();
        if items
            .iter()
            .all(|item| matches!(item.as_ref(), InlineLineItem::Fragment(_)))
            && !items.iter().any(|item| {
                matches!(
                    &item.item,
                    InlineLineItem::Fragment(fragment)
                        if fragment.is_selected_discretionary_marker()
                )
            })
            && items.iter().any(|item| match item {
                MeasuredInlineItem {
                    item: InlineLineItem::Fragment(fragment),
                    ..
                } => fragment.text().chars().any(|character| {
                    character_is_join_control(character) || character_is_arabic_tatweel(character)
                }),
                _ => false,
            })
        {
            let mut output = items
                .iter()
                .filter_map(|item| match item {
                    MeasuredInlineItem {
                        item: InlineLineItem::Fragment(fragment),
                        ..
                    } => {
                        let text = text_without_bidi_format_controls(fragment.text()).into_owned();
                        self.measured_fragment_with_text(fragment, text)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            apply_visual_tracking_boundaries(&mut output);
            return output;
        }
        if !mixed_inline_line_needs_bidi_ordering(items, block_style) {
            let mut output = items.to_vec();
            apply_visual_tracking_boundaries(&mut output);
            return output;
        }
        let (text, ranged_items) =
            mixed_measured_inline_line_bidi_text(items, &bidi_scope_continuations);
        let mut visual_ranges = normalize_mixed_inline_visual_ranges(
            &text,
            self.font_system
                .visual_ranges_for_unwrapped_text(&text, block_style),
            match block_style.used_direction() {
                Direction::Ltr => ResolvedBidiDirection::Ltr,
                Direction::Rtl => ResolvedBidiDirection::Rtl,
            },
        );
        merge_owned_join_control_visual_ranges(&text, &mut visual_ranges, &ranged_items);
        let visual_ranges = split_mixed_inline_visual_ranges_at_transparent_inline_edges(
            visual_ranges,
            &ranged_items,
            &text,
        );
        let mut output = Vec::new();
        let mut emitted = vec![false; ranged_items.len()];
        for visual_range in visual_ranges {
            let visual_fragment_start = output.len();
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.start,
                true,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.end,
                true,
                &mut emitted,
                &mut output,
            );
            for (index, ranged) in ranged_items.iter().enumerate() {
                let start = ranged.range.start.max(visual_range.range.start);
                let end = ranged.range.end.min(visual_range.range.end);
                if start >= end {
                    continue;
                }
                if let Some(item) = self.measured_visual_item_slice(
                    ranged,
                    start,
                    end,
                    visual_range.direction,
                    block_style,
                    line_has_bidi_scope_controls,
                ) {
                    output.push(item);
                    emitted[index] = true;
                }
            }
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.start,
                false,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.end,
                false,
                &mut emitted,
                &mut output,
            );
            if visual_fragment_start > 0
                && let Some(MeasuredInlineItem {
                    item: InlineLineItem::Fragment(fragment),
                    ..
                }) = output[visual_fragment_start..]
                    .iter_mut()
                    .find(|item| matches!(item.item, InlineLineItem::Fragment(_)))
            {
                fragment.mark_starts_visual_fragment();
            }
        }
        let mut output = if output.is_empty() {
            items
                .iter()
                .filter_map(|item| match &item.item {
                    InlineLineItem::Fragment(fragment) => {
                        let text = text_without_bidi_format_controls(fragment.text()).into_owned();
                        self.measured_fragment_with_text(fragment, text)
                    }
                    InlineLineItem::Atom(atom) => {
                        (!matches!(atom.content(), InlineAtomContent::Leader(_)))
                            .then(|| item.clone())
                    }
                    InlineLineItem::Float(_) => None,
                })
                .collect()
        } else {
            reconcile_mixed_inline_fragment_edge_ownership(&mut output);
            output
        };
        apply_visual_tracking_boundaries(&mut output);
        output
    }

    pub(in crate::layout) fn measured_visual_item_slice(
        &mut self,
        ranged: &RangedMeasuredMixedInlineLineItem,
        start: usize,
        end: usize,
        resolved_direction: ResolvedBidiDirection,
        block_style: &ComputedStyle,
        line_has_bidi_scope_controls: bool,
    ) -> Option<MeasuredInlineItem> {
        match &ranged.item.item {
            InlineLineItem::Fragment(fragment) => {
                let visual_slice = expand_visual_slice_with_owned_join_controls(
                    fragment.text(),
                    ranged.range.clone(),
                    start..end,
                );
                let relative_start = visual_slice.start - ranged.range.start;
                let relative_end = visual_slice.end - ranged.range.start;
                let mut text = char_boundary_slice(fragment.text(), relative_start..relative_end)?;
                text = text_without_bidi_format_controls(&text).into_owned();
                if text.is_empty() {
                    return None;
                }
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                hanging_edges.blocks_start = hanging_edges.blocks_start && relative_start == 0;
                hanging_edges.blocks_end =
                    hanging_edges.blocks_end && relative_end == ranged.range.len();
                fragment = fragment.with_hanging_edges(hanging_edges);
                // The enclosing line has already gone through UAX #9 above.
                // Reapplying this fragment's CSS direction or unicode-bidi
                // scope here would resolve its edge neutrals in a new context.
                // The complete line has already resolved an inline CSS bidi
                // scope. Retaining any graph source slice would let its
                // pre-resolution shaping leak into visual painting, so shape
                // selected visual text under the internal unscoped style.
                // <https://drafts.csswg.org/css-writing-modes-4/#bidi-algo>
                let mut selected_shaped =
                    (!line_has_bidi_scope_controls)
                        .then(|| {
                            ranged.item.shaped.as_deref().and_then(|shaped| {
                                shaped.source_slice(relative_start..relative_end)
                            })
                        })
                        .flatten();
                fragment.set_text(text);
                fragment.set_resolved_bidi_direction(Some(resolved_direction));
                // The source line has already been shaped and resolved by
                // UAX #9. Preserve its selected glyph slice in every visual
                // direction: re-shaping an RTL slice independently can
                // resolve edge neutrals differently and loses the source
                // glyph form even when no cursive context is involved.
                // <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>
                if let Some(selected_shaped) = &mut selected_shaped {
                    // The graph source shape predates visual-level selection.
                    // Apply UAX #9 L4 while handing it to this visual slice,
                    // before its advance becomes part of line layout.
                    self.font_system
                        .apply_resolved_bidi_glyph_mirroring(selected_shaped, resolved_direction);
                }
                fragment.set_preserves_source_shaping(selected_shaped.is_some());
                if selected_shaped.is_some() {
                    // `set_text()` resets this source-dependent flag, but a
                    // slice of the graph's durable shaped line already had
                    // its backend terminal tracking normalized.
                    fragment.mark_terminal_tracking_normalized();
                }
                let shaped = selected_shaped.or_else(|| {
                    self.font_system.shape_visual_ordered_line(
                        fragment.text(),
                        fragment.style(),
                        fragment.style().line_height,
                        resolved_direction,
                    )
                });
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                let shaped = shaped.map(Rc::new);
                Some(MeasuredInlineItem {
                    item: InlineLineItem::Fragment(fragment),
                    width,
                    shaped,
                })
            }
            InlineLineItem::Atom(atom)
                if mixed_inline_atom_participates_in_bidi_ordering(atom)
                    && start == ranged.range.start
                    && end == ranged.range.end =>
            {
                Some(MeasuredInlineItem {
                    item: InlineLineItem::Atom(atom.clone()),
                    width: inline_atom_logical_inline_size(atom, block_style),
                    shaped: None,
                })
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
        }
    }

    pub(in crate::layout) fn measured_fragment_with_text(
        &mut self,
        fragment: &InlineFragment,
        text: String,
    ) -> Option<MeasuredInlineItem> {
        if text.is_empty() {
            return None;
        }
        let mut fragment = fragment.clone();
        fragment.set_text(text);
        let shaped = self.font_system.shape_unwrapped_line(
            fragment.text(),
            fragment.style(),
            fragment.style().line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        let shaped = shaped.map(Rc::new);
        Some(MeasuredInlineItem {
            item: InlineLineItem::Fragment(fragment),
            width,
            shaped,
        })
    }

    pub(in crate::layout) fn push_mixed_inline_transparent_edges_at_visual_boundary(
        &self,
        ranged_items: &[RangedMeasuredMixedInlineLineItem],
        boundary: usize,
        precedes_visual_content: bool,
        emitted: &mut [bool],
        output: &mut Vec<MeasuredInlineItem>,
    ) {
        for (edge_index, ranged) in ranged_items.iter().enumerate() {
            if emitted[edge_index]
                || ranged.range.start != boundary
                || !measured_item_is_transparent_mixed_inline_edge(&ranged.item)
                || transparent_inline_edge_precedes_visual_content(&ranged.item)
                    .is_none_or(|precedes| precedes != precedes_visual_content)
            {
                continue;
            }
            emitted[edge_index] = true;
            output.push(ranged.item.clone());
        }
    }

    /// Return a mixed inline item's signed extents around its baseline.
    ///
    /// CSS Inline Layout defines line box height from the logical extents of
    /// inline-level boxes placed around the shared line baseline. Text
    /// fragments keep the CSS `line-height` logical box even when selected font
    /// ink metrics are taller; CSS 2.2 permits negative leading, so glyph ink
    /// can overflow without increasing the line box. The descent can therefore
    /// be negative when the baseline falls below the used `line-height` box:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(in crate::layout) fn inline_line_item_baseline_extents(
        &mut self,
        item: &MeasuredInlineItem,
        block_style: &ComputedStyle,
    ) -> InlineBaselineExtents {
        match &item.item {
            InlineLineItem::Fragment(fragment) => {
                if fragment.style().font_size <= 0.01 || fragment.style().line_height <= 0.01 {
                    return InlineBaselineExtents::new(0.0, 0.0);
                }
                let metrics = self.inline_text_box_metrics(
                    fragment.style(),
                    item.shaped.as_deref(),
                    fragment.baseline_shift,
                );
                if inline_fragment_uses_text_edge_layout(fragment) {
                    return self.inline_text_edge_baseline_extents(
                        fragment.style(),
                        fragment.baseline_shift,
                        metrics,
                    );
                }
                let block_size = metrics.line_block_size;
                if block_size <= 0.0 {
                    return InlineBaselineExtents::new(0.0, 0.0);
                }
                InlineBaselineExtents::from_shifted_baseline_and_block_size(
                    metrics.line_baseline_offset,
                    block_size,
                    fragment.baseline_shift,
                )
            }
            // Inline box-edge atoms represent only the start/end contribution
            // in the line's inline axis. They retain the originating style so
            // their background and border can paint, but are not atomic
            // margin boxes in the block axis: CSS 2.2 specifies that an
            // inline box's block-axis margin, border, and padding do not
            // affect the line box height.
            // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) =>
            {
                let metrics = self.inline_text_box_metrics(atom.style(), None, atom.baseline_shift);
                if !matches!(atom.style().text_box_trim, TextBoxTrim::None)
                    || !matches!(atom.style().line_fit_edge, LineFitEdge::Leading)
                {
                    // An inline box is represented by both its text fragments
                    // and zero-inline-advance edge atoms. Its edges still
                    // establish the box's block-axis participation, so they
                    // must use the same trimmed text edges as the fragments
                    // instead of restoring the untrimmed `line-height`.
                    // <https://drafts.csswg.org/css-inline-3/#text-box-trim>
                    self.inline_text_edge_baseline_extents(
                        atom.style(),
                        atom.baseline_shift,
                        metrics,
                    )
                } else {
                    self.inline_style_line_extents(atom.style(), atom.baseline_shift)
                }
            }
            InlineLineItem::Atom(atom) => {
                let atom_block_size = inline_atom_logical_block_size(atom, block_style);
                // A baseline-aligned atomic inline that fits within the
                // parent's strut does not move either line-box edge. In
                // vertical writing, using its block-end edge as a synthetic
                // baseline instead adds the parent's descent a second time;
                // intrinsic collection then selects a wider orthogonal root
                // than final inline flow. Only an atom larger than the strut
                // needs its own baseline extents to enlarge the line.
                // <https://www.w3.org/TR/css-inline-3/#line-height-property>
                if block_style.writing_mode.has_vertical_lines()
                    && atom.baseline_shift.abs() <= 0.01
                    && atom_block_size <= block_style.line_height + 0.01
                {
                    self.inline_style_line_extents(block_style, 0.0)
                } else {
                    Self::inline_atom_line_baseline_extents(atom, block_style)
                }
            }
            InlineLineItem::Float(_) => InlineBaselineExtents::new(0.0, 0.0),
        }
    }

    fn inline_text_edge_baseline_extents(
        &mut self,
        style: &ComputedStyle,
        baseline_shift: f32,
        metrics: InlineTextBoxMetrics,
    ) -> InlineBaselineExtents {
        let use_line_fit_edge = !matches!(style.line_fit_edge, LineFitEdge::Leading);
        let pair = style.line_fit_edge.text_box_pair();
        let mut over_edge = if use_line_fit_edge {
            self.text_edge_over_position(style, metrics, pair.over)
        } else {
            metrics.block_start_leading
        };
        let mut under_edge = if use_line_fit_edge {
            self.text_edge_under_position(style, metrics, pair.under)
        } else {
            metrics.block_start_leading + metrics.content_block_size
        };
        let trim = self.inline_text_box_content_trim_for_style(style, metrics);
        over_edge += trim.block_start;
        under_edge -= trim.block_end;
        if under_edge < over_edge {
            under_edge = over_edge;
        }
        let (block_start_extra, block_end_extra) =
            inline_fragment_block_axis_outer_extras(style, use_line_fit_edge);
        let layout_over_edge = over_edge - block_start_extra;
        let layout_under_edge = under_edge + block_end_extra;
        InlineBaselineExtents::new(
            metrics.line_baseline_offset - layout_over_edge + baseline_shift,
            layout_under_edge - metrics.line_baseline_offset - baseline_shift,
        )
    }

    /// Return an atomic inline margin box's signed extents around the line baseline.
    ///
    /// CSS 2.2 `vertical-align` shifts the whole inline-level box relative to
    /// the parent baseline. Line metrics must enclose the shifted margin-box
    /// top and bottom, instead of reusing the shifted baseline offset as the
    /// box's unshifted ascent; otherwise `vertical-align: middle` lowers an
    /// inline-block and incorrectly inflates the row advance. The returned
    /// descent remains signed for consistency with negative-leading text boxes:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn inline_atom_line_baseline_extents(
        atom: &InlineAtom,
        containing_style: &ComputedStyle,
    ) -> InlineBaselineExtents {
        let block_size = inline_atom_logical_block_size(atom, containing_style);
        let unshifted_baseline = match containing_style.writing_mode {
            WritingMode::HorizontalTb => atom.style().margin.top + atom.baseline_offset,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => {
                // Replaced inline content participates in vertical line
                // layout with the central baseline.  Treating its block-end
                // edge as a horizontal alphabetic fallback baseline leaves
                // the parent strut's descent outside every image, inflating
                // both the used line column and its intrinsic Grid row
                // contribution.
                // <https://www.w3.org/TR/css-writing-modes-4/#intro-baselines>
                match atom.content() {
                    InlineAtomContent::Image(_)
                    | InlineAtomContent::Svg { asset: Some(_) }
                    | InlineAtomContent::Canvas
                    | InlineAtomContent::Iframe(_) => block_size * 0.5,
                    _ => {
                        inline_atom_logical_block_start_margin(atom, containing_style)
                            + inline_atom_logical_border_block_size(atom, containing_style)
                    }
                }
            }
        };
        InlineBaselineExtents::from_baseline_and_block_size(
            unshifted_baseline + atom.baseline_shift,
            block_size,
        )
    }

    /// Return whether the item is positioned relative to the line box instead
    /// of the shared baseline.
    ///
    /// CSS Inline Layout defines `baseline-shift: top | center | bottom` as
    /// line-relative alignment. Those boxes still contribute to the line box
    /// block-size, but they must not add their ascent/descent to the
    /// baseline-aligned strut:
    /// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
    pub(in crate::layout) fn inline_line_item_has_line_relative_baseline_shift(
        item: &InlineLineItem,
    ) -> bool {
        let vertical_align = match item {
            InlineLineItem::Fragment(fragment) => fragment.style().vertical_align.clone(),
            InlineLineItem::Atom(atom) => {
                // A layout-contained principal box exports no descendant
                // baseline. Treat its otherwise baseline-aligned margin box
                // as block-end aligned while deriving line metrics, instead
                // of letting a scalar fallback baseline enlarge the line's
                // ascent.
                // <https://www.w3.org/TR/css-contain-1/#containment-layout>
                if !atom.exports_internal_baseline() {
                    return true;
                }
                atom.style().vertical_align.clone()
            }
            InlineLineItem::Float(_) => VerticalAlign::BASELINE,
        };
        vertical_align.has_line_relative_baseline_shift()
    }

    fn inline_line_item_parent_content_edge_extents(
        &mut self,
        item: &MeasuredInlineItem,
        block_style: &ComputedStyle,
    ) -> Option<InlineBaselineExtents> {
        let (vertical_align, block_size) = match &item.item {
            InlineLineItem::Fragment(fragment) => {
                let metrics =
                    self.inline_text_box_metrics(fragment.style(), item.shaped.as_deref(), 0.0);
                (
                    fragment.style().vertical_align.clone(),
                    metrics.line_block_size,
                )
            }
            InlineLineItem::Atom(atom) => (
                atom.style().vertical_align.clone(),
                inline_line_item_logical_block_size(&item.item, block_style),
            ),
            InlineLineItem::Float(_) => return None,
        };
        let parent_metrics = self.inline_text_box_metrics(block_style, None, 0.0);
        let parent_content_above = parent_metrics.content_baseline_offset;
        let parent_content_below =
            parent_metrics.content_block_size - parent_metrics.content_baseline_offset;
        let (baseline_offset, descent) = match vertical_align.alignment_baseline {
            AlignmentBaseline::Metric(BaselineMetric::TextTop) => {
                (parent_content_above, block_size - parent_content_above)
            }
            AlignmentBaseline::Metric(BaselineMetric::TextBottom) => {
                (block_size - parent_content_below, parent_content_below)
            }
            AlignmentBaseline::Baseline
            | AlignmentBaseline::Metric(
                BaselineMetric::Alphabetic
                | BaselineMetric::Ideographic
                | BaselineMetric::Middle
                | BaselineMetric::Central
                | BaselineMetric::Mathematical
                | BaselineMetric::Hanging,
            ) => return None,
        };
        Some(InlineBaselineExtents::new(baseline_offset, descent))
    }

    /// Return the parent line strut's signed extents around its baseline.
    ///
    /// The strut participates in every inline formatting context line. Text
    /// painting in this renderer uses the selected-font ascent as the line
    /// baseline coordinate, while `line-height` remains the used block-axis
    /// line advance. CSS 2.2 allows negative leading, so either side of the
    /// baseline can be negative; callers clamp only the final line height:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-inline-3/#line-height-property>.
    pub(in crate::layout) fn inline_style_line_extents(
        &mut self,
        style: &ComputedStyle,
        baseline_shift: f32,
    ) -> InlineBaselineExtents {
        let metrics = self.inline_text_box_metrics(style, None, baseline_shift);
        InlineBaselineExtents::from_shifted_baseline_and_block_size(
            metrics.line_baseline_offset,
            metrics.line_block_size,
            baseline_shift,
        )
    }

    pub(in crate::layout) fn inline_text_box_metrics(
        &mut self,
        style: &ComputedStyle,
        shaped: Option<&ShapedInlineLine>,
        _baseline_shift: f32,
    ) -> InlineTextBoxMetrics {
        let resolved = self.font_system.resolved_inline_text_metrics(style, shaped);
        let content_block_size = layout_points(resolved.content_block_size()).max(0.0);
        let content_baseline_offset = layout_points(resolved.content.above_baseline);
        let line_block_size = layout_points(resolved.line_block_size()).max(0.0);
        let block_start_leading = layout_points(resolved.block_start_leading());
        let block_end_leading = layout_points(resolved.block_end_leading());
        let line_baseline_offset = block_start_leading + content_baseline_offset;
        InlineTextBoxMetrics {
            content_block_size,
            content_baseline_offset,
            line_block_size,
            block_start_leading,
            block_end_leading,
            line_baseline_offset,
        }
    }

    /// Return line metrics for mixed inline line-box participants.
    ///
    /// CSS Inline Layout creates every line box from the parent strut plus the
    /// inline-level boxes placed on that line. Soft-wrapped fragments and
    /// hard-break fragments must therefore use the same strut and baseline
    /// calculation:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(in crate::layout) fn mixed_inline_line_metrics(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
        width: f32,
    ) -> InlineLineMetrics {
        let baseline_extents = self.mixed_inline_line_baseline_extents(items, block_style);
        let non_baseline_aligned_height =
            self.mixed_inline_line_non_baseline_aligned_height(items, block_style);
        InlineLineMetrics {
            width,
            height: baseline_extents.height().max(non_baseline_aligned_height),
            baseline_offset: baseline_extents.baseline_offset,
        }
    }

    pub(in crate::layout) fn mixed_inline_line_baseline_extents(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
    ) -> InlineBaselineExtents {
        let mut extents = self.inline_style_line_extents(block_style, 0.0);
        for item in items {
            if matches!(item.as_ref(), InlineLineItem::Float(_)) {
                continue;
            }
            // A line containing only CSS Text other-space separators still
            // owns the parent strut, but an invisible fallback glyph must not
            // enlarge that line beyond its specified `line-height`. The
            // separator's inline advance is handled independently by Phase
            // II hanging and line fitting.
            if matches!(
                item.as_ref(),
                InlineLineItem::Fragment(fragment)
                    if !fragment.text().is_empty()
                        && fragment
                            .text()
                            .chars()
                            .all(crate::text::character_is_css_other_space_separator)
            ) {
                continue;
            }
            if Self::inline_line_item_is_initial_letter(&item.item) {
                continue;
            }
            if Self::inline_line_item_has_line_relative_baseline_shift(&item.item) {
                continue;
            }
            if let Some(item_extents) =
                self.inline_line_item_parent_content_edge_extents(item, block_style)
            {
                extents = extents.union(item_extents);
                continue;
            }
            extents = extents.union(self.inline_line_item_baseline_extents(item, block_style));
        }
        extents
    }

    pub(in crate::layout) fn mixed_inline_line_non_baseline_aligned_height<T>(
        &mut self,
        items: &[T],
        block_style: &ComputedStyle,
    ) -> f32
    where
        T: AsRef<InlineLineItem>,
    {
        let mut height: f32 = 0.0;
        for item in items {
            let item = item.as_ref();
            if Self::inline_line_item_is_initial_letter(item) {
                continue;
            }
            if Self::inline_line_item_has_line_relative_baseline_shift(item) {
                height = height.max(match item {
                    InlineLineItem::Fragment(fragment) => {
                        self.font_system.used_line_height(fragment.style()).points()
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                        inline_line_item_logical_block_size(item, block_style)
                    }
                });
            }
        }
        height
    }

    pub(in crate::layout) fn inline_line_item_is_initial_letter(item: &InlineLineItem) -> bool {
        match item {
            InlineLineItem::Fragment(fragment) => !fragment.style().initial_letter.is_normal(),
            InlineLineItem::Atom(atom) => !atom.style().initial_letter.is_normal(),
            InlineLineItem::Float(_) => false,
        }
    }
}

/// Resolve CSS Text tracking on the final UBA visual sequence.
///
/// Source fragments retain their lexical [`InlineTrackingScope`] even when a
/// visual slice reverses their order.  The resolver deliberately owns the
/// only non-content advance: a successor receives the boundary's advance,
/// leaving atom and inline-background geometry at their authored extent.
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>.
pub(in crate::layout) fn apply_visual_tracking_boundaries(items: &mut [MeasuredInlineItem]) {
    #[derive(Clone)]
    enum UnitKind {
        Text,
        AtomicRun,
    }

    #[derive(Clone)]
    struct VisualUnit {
        kind: UnitKind,
        first_item: usize,
        last_item: usize,
        first_scope: Rc<InlineTrackingScope>,
        last_scope: Rc<InlineTrackingScope>,
        first_text: Option<String>,
        last_text: Option<String>,
        starts_visual_fragment: bool,
    }

    for item in items.iter_mut() {
        let leading = match &item.item {
            InlineLineItem::Fragment(fragment) => fragment.leading_tracking().points(),
            InlineLineItem::Atom(atom) => atom.leading_tracking().points(),
            InlineLineItem::Float(_) => 0.0,
        };
        item.width = (item.width - leading).max(0.0);
        let terminal_tracking = match &item.item {
            InlineLineItem::Fragment(fragment) if !fragment.terminal_tracking_normalized() => {
                Some(line_end_letter_spacing_width(fragment.text(), fragment.style()).points())
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                None
            }
        };
        if let Some(terminal_tracking) = terminal_tracking {
            // The shaping backend owns this advance in its glyph stream.
            // Normalize the visual artifact together with its scalar width;
            // final visual-boundary tracking is added to successors below.
            item.width = (item.width - terminal_tracking).max(0.0);
            if let Some(shaped) = &item.shaped {
                let mut shaped = (**shaped).clone();
                shaped.remove_terminal_letter_spacing(terminal_tracking);
                item.shaped = Some(Rc::new(shaped));
            }
        }
        match &mut item.item {
            InlineLineItem::Fragment(fragment) => {
                fragment.set_leading_tracking(layout_pt(0.0));
                if terminal_tracking.is_some() {
                    fragment.mark_terminal_tracking_normalized();
                }
            }
            InlineLineItem::Atom(atom) => atom.set_leading_tracking(layout_pt(0.0)),
            InlineLineItem::Float(_) => {}
        }
    }

    let mut units = Vec::<VisualUnit>::new();
    let mut sequence_is_open = true;
    for (item_index, item) in items.iter().enumerate() {
        match &item.item {
            InlineLineItem::Fragment(fragment)
                if inline_fragment_is_inter_character_unit(fragment) =>
            {
                let Some(scope) = fragment.tracking_scope().cloned() else {
                    sequence_is_open = false;
                    continue;
                };
                units.push(VisualUnit {
                    kind: UnitKind::Text,
                    first_item: item_index,
                    last_item: item_index,
                    first_scope: Rc::clone(&scope),
                    last_scope: scope,
                    first_text: Some(fragment.text().to_string()),
                    last_text: Some(fragment.text().to_string()),
                    starts_visual_fragment: fragment.starts_visual_fragment(),
                });
                sequence_is_open = true;
            }
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_unit(atom) => {
                let Some(scope) = atom.tracking_scope().cloned() else {
                    sequence_is_open = false;
                    continue;
                };
                if sequence_is_open
                    && matches!(
                        units.last().map(|unit| &unit.kind),
                        Some(UnitKind::AtomicRun)
                    )
                {
                    let last = units.last_mut().expect("checked atomic run");
                    last.last_item = item_index;
                    last.last_scope = scope;
                } else {
                    units.push(VisualUnit {
                        kind: UnitKind::AtomicRun,
                        first_item: item_index,
                        last_item: item_index,
                        first_scope: Rc::clone(&scope),
                        last_scope: scope,
                        first_text: None,
                        last_text: None,
                        starts_visual_fragment: false,
                    });
                }
                sequence_is_open = true;
            }
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_transparent(atom) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => sequence_is_open = false,
            InlineLineItem::Fragment(_) => {}
        }
    }

    for pair in units.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if right.starts_visual_fragment {
            continue;
        }
        let permits_gap = match (&left.kind, &right.kind) {
            (UnitKind::Text, UnitKind::Text) => {
                let left_text = left.last_text.as_deref().expect("text unit retains text");
                let right_text = right.first_text.as_deref().expect("text unit retains text");
                crate::text::inter_character_gap_allowed_between_text(left_text, right_text)
            }
            _ => true,
        };
        if !permits_gap {
            continue;
        }
        let owner = InlineTrackingScope::lowest_common(&left.last_scope, &right.first_scope);
        let advance = owner.letter_spacing();
        if advance.points() == 0.0 {
            continue;
        }
        let target = &mut items[right.first_item];
        target.width += advance.points();
        match &mut target.item {
            InlineLineItem::Fragment(fragment) => fragment.set_leading_tracking(advance),
            InlineLineItem::Atom(atom) => atom.set_leading_tracking(advance),
            InlineLineItem::Float(_) => unreachable!("typographic unit is never a float"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked_style(points: f32) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.letter_spacing = crate::css::ComputedLengthPercentage::from_points(points);
        style
    }

    fn tracked_fragment(
        text: &str,
        style: ComputedStyle,
        scope: Rc<InlineTrackingScope>,
    ) -> InlineFragment {
        InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        )
        .with_tracking_scope(scope)
    }

    fn measured_fragment(fragment: InlineFragment, width: f32) -> MeasuredInlineItem {
        MeasuredInlineItem {
            item: InlineLineItem::Fragment(fragment),
            width,
            shaped: None,
        }
    }

    #[test]
    fn positive_baseline_shift_raises_text_extents() {
        let extents = InlineBaselineExtents::from_shifted_baseline_and_block_size(12.0, 20.0, 5.0);

        assert_eq!(extents.baseline_offset, 17.0);
        assert_eq!(extents.descent, 3.0);
    }

    #[test]
    fn negative_baseline_shift_lowers_atomic_extents() {
        let style = ComputedStyle::initial();
        let atom = InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(10.0, 20.0),
            20.0,
            -4.0,
            None,
            None,
        );
        let extents = LayoutBuilder::inline_atom_line_baseline_extents(&atom, &style);

        assert_eq!(extents.baseline_offset, 16.0);
        assert_eq!(extents.descent, 4.0);
    }

    #[test]
    fn tracking_uses_scope_lca_and_is_leading_only() {
        let root_style = tracked_style(11.0);
        let child_style = tracked_style(3.0);
        let root = InlineTrackingScope::root(&root_style);
        let child = InlineTrackingScope::child(Rc::clone(&root), &child_style);
        // Widths include the terminal shaper advance. The resolver removes it
        // and represents only the owned visual boundary on the successor.
        let mut items = vec![
            measured_fragment(tracked_fragment("a", child_style, child), 13.0),
            measured_fragment(tracked_fragment("b", root_style, Rc::clone(&root)), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        let InlineLineItem::Fragment(first) = &items[0].item else {
            panic!("test setup creates text")
        };
        let InlineLineItem::Fragment(second) = &items[1].item else {
            panic!("test setup creates text")
        };
        assert_eq!(first.leading_tracking().points(), 0.0);
        assert_eq!(second.leading_tracking().points(), 11.0);
        assert_eq!(items[0].width, 10.0);
        assert_eq!(items[1].width, 21.0);
    }

    #[test]
    fn terminal_tracking_is_removed_before_a_nested_inline_boundary() {
        let style = tracked_style(11.0);
        let root = InlineTrackingScope::root(&style);
        let child = InlineTrackingScope::child(Rc::clone(&root), &style);
        let mut items = vec![
            measured_fragment(tracked_fragment("a", style.clone(), child), 21.0),
            measured_fragment(tracked_fragment("b", style, root), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        // The nested fragment has no trailing visual tracking; its successor
        // owns the single LCA-resolved boundary instead.
        assert_eq!(items[0].width, 10.0);
        assert_eq!(items[1].width, 21.0);
    }

    #[test]
    fn tracking_uses_the_final_visual_sequence() {
        let style = tracked_style(11.0);
        let root = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(tracked_fragment("b", style.clone(), Rc::clone(&root)), 21.0),
            measured_fragment(tracked_fragment("a", style, root), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        let InlineLineItem::Fragment(first) = &items[0].item else {
            panic!("test setup creates text")
        };
        let InlineLineItem::Fragment(second) = &items[1].item else {
            panic!("test setup creates text")
        };
        assert_eq!(first.leading_tracking().points(), 0.0);
        assert_eq!(second.leading_tracking().points(), 11.0);
    }

    #[test]
    fn consecutive_atomic_inlines_form_one_typographic_unit() {
        let style = tracked_style(11.0);
        let root = InlineTrackingScope::root(&style);
        let atom = || {
            InlineAtom::new(
                InlineAtomContent::Canvas,
                style.clone(),
                None,
                InlineSize::new(20.0, 20.0),
                20.0,
                0.0,
                None,
                None,
            )
            .with_tracking_scope(Rc::clone(&root))
        };
        let mut items = vec![
            measured_fragment(tracked_fragment("A", style.clone(), Rc::clone(&root)), 21.0),
            MeasuredInlineItem {
                item: InlineLineItem::Atom(atom()),
                width: 20.0,
                shaped: None,
            },
            MeasuredInlineItem {
                item: InlineLineItem::Atom(atom()),
                width: 20.0,
                shaped: None,
            },
            measured_fragment(tracked_fragment("D", style, root), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        let InlineLineItem::Atom(first_atom) = &items[1].item else {
            panic!("test setup creates an atom")
        };
        let InlineLineItem::Atom(second_atom) = &items[2].item else {
            panic!("test setup creates an atom")
        };
        let InlineLineItem::Fragment(last_text) = &items[3].item else {
            panic!("test setup creates text")
        };
        assert_eq!(first_atom.leading_tracking().points(), 11.0);
        assert_eq!(second_atom.leading_tracking().points(), 0.0);
        assert_eq!(last_text.leading_tracking().points(), 11.0);
        assert_eq!(items.iter().map(|item| item.width).sum::<f32>(), 82.0);
    }
}
