use std::rc::Rc;

use super::*;
use crate::css::{Edges, LineFitEdge, TextBoxTrim};
use crate::text::bidi_mirroring_glyph;

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisualTrackingUnitKind {
    Text,
    RubyColumn,
    AtomicRun,
}

/// The minimal state needed to resolve one visual tracking boundary.
///
/// Tracking depends only on the first item and scope of the right unit, the
/// final scope of the left unit, and the two text edges at that boundary. It
/// intentionally does not retain source strings or the earlier visual units.
#[derive(Clone)]
struct VisualTrackingUnit {
    kind: VisualTrackingUnitKind,
    first_item: usize,
    first_used_spacing: UsedLetterSpacing,
    last_used_spacing: UsedLetterSpacing,
    text_allows_gap_before: bool,
    text_allows_gap_after: bool,
    starts_visual_fragment: bool,
}

fn apply_visual_tracking_boundary(
    items: &mut [MeasuredInlineItem],
    left: &VisualTrackingUnit,
    right: &VisualTrackingUnit,
) {
    if right.starts_visual_fragment {
        return;
    }
    let permits_gap = match (left.kind, right.kind) {
        (VisualTrackingUnitKind::Text, VisualTrackingUnitKind::Text) => {
            left.text_allows_gap_after && right.text_allows_gap_before
        }
        _ => true,
    };
    if !permits_gap {
        return;
    }
    let advance = InlineBoundaryAdvance::between(left.last_used_spacing, right.first_used_spacing);
    if advance.points() == 0.0 {
        return;
    }
    let target = &mut items[right.first_item];
    target.advance.set_boundary_before(advance);
}

fn inline_fragment_uses_text_edge_layout(fragment: &InlineFragment) -> bool {
    !matches!(fragment.style().text_box_trim, TextBoxTrim::None)
        || !matches!(fragment.style().line_fit_edge, LineFitEdge::Leading)
}

/// Return whether an inline item has no line-box extent of its own.
///
/// A zero `line-height` inline still carries source text and transparent box
/// edges for CSS Text shaping, but it must not participate in the baseline
/// extent union. A zero extent is not neutral to that union: it would replace
/// a valid negative descent from the parent strut with zero and spuriously
/// enlarge the line box.
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
fn inline_item_has_no_line_box_extent(item: &InlineLineItem) -> bool {
    match item {
        InlineLineItem::Fragment(fragment) => {
            matches!(fragment.source(), InlineTextSource::BlockEllipsis)
                || fragment.style().font_size == 0.0
                || fragment.style().line_height == 0.0
        }
        InlineLineItem::Atom(atom) => {
            matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
            ) && (atom.style().font_size == 0.0 || atom.style().line_height == 0.0)
        }
        InlineLineItem::Float(_) => false,
    }
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
            && !inline_box_edge_fragment_breaks_shaping(atom.style(), *edge)
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}

/// Return whether a generated inline-box edge separates adjacent typographic
/// character units for shaping.
///
/// A nonzero inline-axis margin, border, or padding interrupts cursive
/// joining. It has the same joining effect as a zero-width non-joiner while
/// remaining transparent to the Unicode bidi algorithm:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::layout) fn inline_atom_is_logical_shaping_boundary(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
    ) && !inline_atom_is_transparent_to_logical_shaping(atom)
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
    pub(in crate::layout) baseline_offset: f32,
    pub(in crate::layout) descent: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedTextCombineSquareGeometry {
    pub(in crate::layout) baseline_extents: InlineBaselineExtents,
    pub(in crate::layout) paint_placement_baseline_offset: f32,
}

/// The parent text box's over/under baselines measured from its alphabetic
/// line baseline.
///
/// A tate-chu-yoko composition aligns the center of its one-em square to the
/// parent inline box's central baseline, which CSS Writing Modes defines as
/// centered between these two baselines. Keeping this pair distinct from a
/// line box's leading makes it impossible to accidentally center the square
/// around the alphabetic baseline instead.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
impl TextCombineSquareGeometry {
    /// Return a composition square's signed line extents after aligning its
    /// central baseline to the parent text's central baseline.
    pub(in crate::layout) fn resolve(
        self,
        parent_metrics: InlineTextBoxMetrics,
        baseline_shift: f32,
    ) -> ResolvedTextCombineSquareGeometry {
        let square_block_size = self.points();
        debug_assert!(square_block_size >= 0.0);
        let parent_over = parent_metrics.content_baseline_offset;
        let parent_under = parent_metrics.content_block_size - parent_over;
        let parent_central_baseline_from_alphabetic = (parent_over - parent_under) / 2.0;
        let composition_baseline_offset =
            parent_central_baseline_from_alphabetic + square_block_size / 2.0;
        let baseline_extents = InlineBaselineExtents::from_shifted_baseline_and_block_size(
            composition_baseline_offset,
            square_block_size,
            baseline_shift,
        );
        ResolvedTextCombineSquareGeometry {
            paint_placement_baseline_offset: baseline_extents.baseline_offset,
            baseline_extents,
        }
    }
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
    /// Retain logical source shaping before UAX #9 splits it into visual
    /// fragments.
    ///
    /// CSS Text shapes a typographic character unit in logical order, then
    /// the bidi algorithm selects visual line fragments. Re-shaping those
    /// fragments independently changes contextual forms at transparent inline
    /// boundaries. A boundary can also divide a non-joining OpenType cluster
    /// such as lam-alef, so the complete source shape is retained even when
    /// no individual fragment can own a strict glyph slice.
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
            let starts_after_shaping_boundary = start
                .checked_sub(1)
                .and_then(|previous| items.get(previous))
                .is_some_and(|item| matches!(&item.item, InlineLineItem::Atom(atom) if inline_atom_is_logical_shaping_boundary(atom)));
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

            let ends_before_shaping_boundary = items
                .get(index)
                .is_some_and(|item| matches!(&item.item, InlineLineItem::Atom(atom) if inline_atom_is_logical_shaping_boundary(atom)));

            if !starts_after_shaping_boundary
                && !ends_before_shaping_boundary
                && fragment_indices.len() < 2
            {
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
            if starts_after_shaping_boundary {
                text.push('\u{200c}');
                spans.push(StyledTextSpan {
                    text: "\u{200c}",
                    style: source_style,
                });
            }
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
                    style: fragment.style(),
                });
                unshaped_width += item.base_advance().points();
            }
            if ends_before_shaping_boundary {
                let InlineLineItem::Fragment(last_fragment) =
                    &items[*fragment_indices.last().expect("fragment group is nonempty")].item
                else {
                    unreachable!("fragment grouping only records text fragments");
                };
                text.push('\u{200c}');
                spans.push(StyledTextSpan {
                    text: "\u{200c}",
                    style: last_fragment.style(),
                });
            }
            let line_height = line_height.expect("a fragment group always has a first style");
            let shaped = self.font_system.shape_untracked_styled_inline_fragments(
                &spans,
                text,
                unshaped_width,
                line_height,
                0.0,
                tab_metric_style,
            );
            let Some(shaped) = shaped else {
                output.extend_from_slice(&items[start..index]);
                continue;
            };
            let shaped = Rc::new(shaped);
            // A nonzero edge is a true shaping boundary. Its synthetic ZWNJ
            // belongs only to this logical shape, not to a reusable source
            // shared by its members.
            let boundary_source = (!starts_after_shaping_boundary && !ends_before_shaping_boundary)
                .then(|| {
                    Rc::new(BoundaryShapedSource {
                        shaped: Rc::clone(&shaped),
                    })
                });
            // The inserted ZWNJ is an implementation-only boundary marker,
            // never an authored source artifact.  A selection from such a
            // shape must take the ordinary re-shaping path at paint time.
            let selections = if starts_after_shaping_boundary || ends_before_shaping_boundary {
                vec![None; ranges.len()]
            } else {
                ranges
                    .iter()
                    .cloned()
                    .map(|range| SourceShapedSelection::from_source(Rc::clone(&shaped), range))
                    .collect::<Vec<_>>()
            };
            let original_fragment_width = fragment_indices
                .iter()
                .map(|&fragment_index| items[fragment_index].base_advance().points())
                .sum::<f32>();
            // An explicit join control can make the backend report one
            // cluster spanning several lexical fragments. Those fragments
            // still need the full source's aggregate advance for bidi
            // alignment; distribute it proportionally until paint consumes
            // the shared glyph stream once.
            let fallback_width_scale = if selections.iter().any(Option::is_none) {
                shaped.advance_width() / original_fragment_width.max(f32::EPSILON)
            } else {
                1.0
            };
            let mut fragment_ranges = ranges.into_iter();
            let mut selections = selections.into_iter();
            for item in &items[start..index] {
                if let InlineLineItem::Fragment(fragment) = &item.item {
                    let range = fragment_ranges
                        .next()
                        .expect("every logical shaping fragment has one source range");
                    let mut fragment = fragment.clone();
                    if let Some(boundary_source) = &boundary_source {
                        fragment
                            .set_boundary_shaped_source(Rc::clone(boundary_source), range.clone());
                    }
                    let selection = selections
                        .next()
                        .expect("every logical shaping fragment has one source selection");
                    let width = selection
                        .as_ref()
                        .map(|selection| selection.selected().advance_width())
                        .unwrap_or(item.base_advance().points() * fallback_width_scale);
                    let selected = selection.as_ref().map(SourceShapedSelection::selected_rc);
                    fragment.set_source_shaped_selection(selection);
                    let mut measured = item.clone();
                    measured.item = InlineLineItem::Fragment(fragment);
                    measured.advance.replace_base_points(width);
                    measured.shaped = selected.or_else(|| item.shaped.clone());
                    output.push(measured);
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
        line_direction: Direction,
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
            || !bidi_scope_continuations.suffix.is_empty()
            || !bidi_scope_continuations
                .trailing_line_edge_context
                .is_empty();
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
                .visual_ranges_for_unwrapped_text(&text, line_direction),
            match line_direction {
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
                visual_range.direction,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.end,
                true,
                visual_range.direction,
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
                visual_range.direction,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_transparent_edges_at_visual_boundary(
                &ranged_items,
                visual_range.range.end,
                false,
                visual_range.direction,
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
        // UAX #9 removes formatting controls and can consequently leave a
        // zero-width positioned marker inside an empty isolate without a
        // visual range boundary. Retain that structural marker so provisional
        // containing-block measurement and final descendant replay use the
        // same selected line, without introducing a neutral object into bidi
        // resolution or changing its advance. A non-marker atom is retained
        // here only when all of its escaped paint already uses page coordinates;
        // atom-relative replay still requires an exact visual boundary.
        // <https://www.unicode.org/reports/tr9/#X9>
        // <https://drafts.csswg.org/css-position-3/#def-cb>
        for (index, ranged) in ranged_items.iter().enumerate() {
            if emitted[index] {
                continue;
            }
            let InlineLineItem::Atom(atom) = &ranged.item.item else {
                continue;
            };
            let is_positioning_marker = matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
                    if edge.is_positioning_marker()
            );
            let retains_page_coordinates = atom.escaped_positioned_layers().is_some_and(|layers| {
                !layers.is_empty()
                    && layers.iter().all(|layer| {
                        matches!(
                            layer.escaped_atom_replay,
                            EscapedAtomReplay::RetainPageCoordinates
                        )
                    })
            });
            if is_positioning_marker || retains_page_coordinates {
                output.push(ranged.item.clone());
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
                // A boundary source shaped before this full line's UBA pass
                // still carries the fragment's CSS embedding/override. Once
                // a line contains bidi scope controls it cannot be reused for
                // a visual slice: that would run the scope a second time and
                // erase an explicit LTR override of intrinsically RTL text.
                let complete_boundary_source = (!line_has_bidi_scope_controls)
                    .then(|| fragment.boundary_shaped_source_and_range())
                    .flatten();
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
                // A cached source shape may already include a fragment-local
                // `rtlm` substitution for punctuation before this full line
                // has established its final UAX #9 level. Re-shape mirrored
                // characters under that final level; all other source slices
                // retain their contextual glyph forms and joining behavior.
                let mut source_selection = (!line_has_bidi_scope_controls
                    && !text
                        .chars()
                        .any(|character| bidi_mirroring_glyph(character).is_some()))
                .then(|| {
                    fragment
                        .source_shaped_selection()
                        .and_then(|selection| selection.subselection(relative_start..relative_end))
                })
                .flatten();
                // Keep the logical paint text available to the shaped-source
                // selection after transferring its owned copy to the
                // fragment below.
                let bidi_context_text = Rc::<str>::from(&*text);
                fragment.set_text(text);
                // A visual slice preserves its precise authored range in the
                // shared logical shape. A paint group may put those slices
                // in a different order, but it can reuse the full source
                // only after its ranges collectively cover it once.
                // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
                if let Some((source, range)) = complete_boundary_source {
                    let selected_range =
                        (range.start + relative_start)..(range.start + relative_end);
                    fragment.set_boundary_shaped_source(source, selected_range);
                }
                fragment.set_resolved_bidi_direction(Some(resolved_direction));
                // The source line has already been shaped and resolved by
                // UAX #9. Preserve its selected glyph slice in every visual
                // direction: re-shaping an RTL slice independently can
                // resolve edge neutrals differently and loses the source
                // glyph form even when no cursive context is involved.
                // <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>
                if let Some(selection) = &mut source_selection {
                    // The graph source shape predates visual-level selection.
                    // Apply UAX #9 L4 while handing it to this visual slice,
                    // before its advance becomes part of line layout.
                    self.font_system.apply_resolved_bidi_glyph_mirroring(
                        selection.selected_mut(),
                        resolved_direction,
                    );
                    selection.resolve_bidi_context(resolved_direction, bidi_context_text);
                }
                if source_selection.as_ref().is_some_and(|selection| {
                    selection.is_reusable_for(fragment.text(), Some(resolved_direction))
                }) {
                    // `set_text()` resets source-dependent shaping, but this
                    // reusable graph slice is already tracking-free.
                    fragment.set_source_shaped_selection(source_selection);
                } else {
                    fragment.set_source_shaped_selection(None);
                }
                let shaped = fragment
                    .source_shaped_selection()
                    .map(SourceShapedSelection::selected_rc)
                    .map(|selection| selection.as_ref().clone())
                    .or_else(|| {
                        // UAX #9 resolved this slice at the enclosing line's
                        // paragraph level. Keep that outer context while the
                        // visual-order shaper installs its own unscoped guard;
                        // using the inline's `direction:ltr` as a new paragraph
                        // changes the final glyph positioning of an LTR override
                        // embedded in RTL text.
                        let mut visual_context_style = fragment.style().clone();
                        visual_context_style.direction = block_style.used_direction();
                        self.font_system.shape_untracked_visual_ordered_line(
                            fragment.text(),
                            &visual_context_style,
                            visual_context_style.line_height,
                            resolved_direction,
                        )
                    });
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                // Both a reusable graph selection and the visual-order
                // fallback above are explicitly untracked. Do not let the
                // resolver interpret this final visual artifact as a legacy
                // backend-owned terminal advance.
                let shaped = shaped.map(Rc::new);
                Some(MeasuredInlineItem::new(
                    InlineLineItem::Fragment(fragment),
                    width,
                    shaped,
                ))
            }
            InlineLineItem::Atom(atom)
                if (mixed_inline_atom_participates_in_bidi_ordering(atom)
                    || inline_atom_is_logical_shaping_boundary(atom))
                    && start == ranged.range.start
                    && end == ranged.range.end =>
            {
                // A nonzero inline edge is transparent to UAX #9 itself,
                // but its virtual U+200C owns a real visual range so Arabic
                // joining stops at the same position as the box-model
                // advance. Emit the original atom at that resolved position;
                // otherwise the control reorders correctly while the padding,
                // border, or margin vanishes from the visual line.
                // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
                // and <https://www.unicode.org/reports/tr9/#X9>
                Some(MeasuredInlineItem::new(
                    InlineLineItem::Atom(atom.clone()),
                    inline_atom_logical_inline_size(atom, block_style),
                    None,
                ))
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
        let shaped = self.font_system.shape_untracked_inline_line(
            fragment.text(),
            fragment.style(),
            fragment.style().line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        let shaped = shaped.map(Rc::new);
        Some(MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            width,
            shaped,
        ))
    }

    pub(in crate::layout) fn push_mixed_inline_transparent_edges_at_visual_boundary(
        &self,
        ranged_items: &[RangedMeasuredMixedInlineLineItem],
        boundary: usize,
        precedes_visual_content: bool,
        visual_direction: ResolvedBidiDirection,
        emitted: &mut [bool],
        output: &mut Vec<MeasuredInlineItem>,
    ) {
        for (edge_index, ranged) in ranged_items.iter().enumerate() {
            if emitted[edge_index]
                || ranged.range.start != boundary
                || !measured_item_is_transparent_mixed_inline_edge(&ranged.item)
                || transparent_inline_edge_precedes_visual_content(&ranged.item, visual_direction)
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
                if fragment.style().font_size == 0.0 || fragment.style().line_height == 0.0 {
                    return InlineBaselineExtents::new(0.0, 0.0);
                }
                let metrics =
                    self.inline_text_box_metrics(fragment.style(), fragment.baseline_shift);
                let strut_extents = if inline_fragment_uses_text_edge_layout(fragment) {
                    self.inline_text_edge_baseline_extents(
                        fragment.style(),
                        fragment.baseline_shift,
                        metrics,
                    )
                } else {
                    let block_size = metrics.line_block_size;
                    if block_size <= 0.0 {
                        return InlineBaselineExtents::new(0.0, 0.0);
                    }
                    InlineBaselineExtents::from_shifted_baseline_and_block_size(
                        metrics.line_baseline_offset,
                        block_size,
                        fragment.baseline_shift,
                    )
                };
                if fragment.style().line_height_is_normal()
                    && !block_style.writing_mode.has_vertical_lines()
                    && let Some(selected_font_extents) =
                        self.inline_fragment_selected_run_baseline_extents(item, fragment)
                {
                    // The element's own normal-line strut always participates.
                    // Selected fallback faces may enlarge it, but cannot replace
                    // it with a different baseline origin.
                    return strut_extents.union(selected_font_extents);
                }
                strut_extents
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
                let metrics = self.inline_text_box_metrics(atom.style(), atom.baseline_shift());
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
                        atom.baseline_shift(),
                        metrics,
                    )
                } else {
                    self.inline_style_line_extents(atom.style(), atom.baseline_shift())
                }
            }
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::TextCombineUpright { .. })
                    && block_style.writing_mode.has_vertical_lines() =>
            {
                let InlineAtomContent::TextCombineUpright { composition } = atom.content() else {
                    unreachable!("text-combine atom matched above")
                };
                composition
                    .square
                    .resolve(
                        self.inline_text_box_metrics(block_style, 0.0),
                        atom.baseline_shift(),
                    )
                    .baseline_extents
            }
            InlineLineItem::Atom(atom) => {
                Self::inline_atom_line_baseline_extents(atom, block_style)
            }
            InlineLineItem::Float(_) => InlineBaselineExtents::new(0.0, 0.0),
        }
    }

    /// Return the baseline extents contributed by fallback fonts selected for
    /// one shaped horizontal fragment with `line-height: normal`.
    ///
    /// CSS Inline permits fallback glyphs to affect a line box only for a
    /// normal line height. An explicit line height keeps the first available
    /// font's fixed layout box even if fallback glyph ink overflows it. Vertical
    /// line boxes instead use their owner's text-over/text-under baseline
    /// table, so that table remains the single axis-aware baseline authority.
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
    /// <https://drafts.csswg.org/css-inline-3/#inline-sizing>
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>
    fn inline_fragment_selected_run_baseline_extents(
        &mut self,
        item: &MeasuredInlineItem,
        fragment: &InlineFragment,
    ) -> Option<InlineBaselineExtents> {
        let shaped = item.shaped.as_deref()?;
        let mut extents = None;
        for run in &shaped.runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            if !run.paints || !run.glyphs.iter().any(|glyph| glyph.paints) {
                continue;
            }
            let resolved = self
                .font_system
                .resolved_inline_text_metrics_for_selected_font(
                    fragment.style(),
                    font_id,
                    run.font_size,
                )?;
            let metrics = InlineTextBoxMetrics {
                content_block_size: layout_points(resolved.content_block_size()).max(0.0),
                content_baseline_offset: layout_points(resolved.content.above_baseline),
                line_block_size: layout_points(resolved.line_block_size()).max(0.0),
                block_start_leading: layout_points(resolved.block_start_leading()),
                block_end_leading: layout_points(resolved.block_end_leading()),
                line_baseline_offset: layout_points(resolved.block_start_leading())
                    + layout_points(resolved.content.above_baseline),
            };
            let run_extents = if inline_fragment_uses_text_edge_layout(fragment) {
                self.inline_text_edge_baseline_extents(
                    fragment.style(),
                    fragment.baseline_shift,
                    metrics,
                )
            } else if metrics.line_block_size <= 0.0 {
                InlineBaselineExtents::new(0.0, 0.0)
            } else {
                InlineBaselineExtents::from_shifted_baseline_and_block_size(
                    metrics.line_baseline_offset,
                    metrics.line_block_size,
                    fragment.baseline_shift,
                )
            };
            extents = Some(
                extents.map_or(run_extents, |current: InlineBaselineExtents| {
                    current.union(run_extents)
                }),
            );
        }
        extents
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
        let unshifted_baseline = match atom.content() {
            // Replaced inline content participates in vertical line layout
            // with the central baseline.  Non-replaced atomic boxes instead
            // preserve their exported logical block-axis baseline in every
            // writing mode.
            // <https://www.w3.org/TR/css-writing-modes-4/#intro-baselines>
            InlineAtomContent::Image(_)
            | InlineAtomContent::Gradient { .. }
            | InlineAtomContent::Svg { asset: Some(_) }
            | InlineAtomContent::Canvas
            | InlineAtomContent::Iframe(_)
                if containing_style.writing_mode.has_vertical_lines() =>
            {
                block_size * 0.5
            }
            _ => inline_atom_logical_margin_box_baseline_offset(atom, containing_style).points(),
        };
        InlineBaselineExtents::from_baseline_and_block_size(
            unshifted_baseline + atom.baseline_shift(),
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
        match item {
            InlineLineItem::Fragment(fragment) => fragment.line_relative_alignment().is_some(),
            InlineLineItem::Atom(atom) => atom.line_relative_alignment().is_some(),
            InlineLineItem::Float(_) => false,
        }
    }

    fn inline_line_item_parent_content_edge_extents(
        &mut self,
        item: &MeasuredInlineItem,
        block_style: &ComputedStyle,
    ) -> Option<InlineBaselineExtents> {
        let (vertical_align, block_size) = match &item.item {
            InlineLineItem::Fragment(fragment) => {
                let metrics = self.inline_text_box_metrics(fragment.style(), 0.0);
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
        let parent_metrics = self.inline_text_box_metrics(block_style, 0.0);
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
        let metrics = self.inline_text_box_metrics(style, baseline_shift);
        InlineBaselineExtents::from_shifted_baseline_and_block_size(
            metrics.line_baseline_offset,
            metrics.line_block_size,
            baseline_shift,
        )
    }

    pub(in crate::layout) fn inline_text_box_metrics(
        &mut self,
        style: &ComputedStyle,
        _baseline_shift: f32,
    ) -> InlineTextBoxMetrics {
        let resolved = self.font_system.resolved_inline_text_metrics(style);
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
            if inline_item_has_no_line_box_extent(&item.item) {
                continue;
            }
            // Regular-inline edges carry lexical structure and decoration
            // geometry, but do not independently establish a baseline.
            // Counting their originating style here can make a `top` or
            // `bottom` inline shift the very baseline against which its
            // aligned subtree is placed. Its text and atomic descendants are
            // accounted for through their scoped line-relative alignment.
            // <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
            if matches!(
                item.as_ref(),
                InlineLineItem::Atom(atom)
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    )
            ) {
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
                // A regular inline's `top`/`bottom` alignment owns its
                // aligned subtree rather than only the style copied to a
                // descendant text fragment. Include both the scope's strut
                // and the selected participant so empty scopes, smaller
                // children, and atomic descendants retain their required
                // line-box extent.
                // <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>
                let participant_height = match item {
                    InlineLineItem::Fragment(fragment) => {
                        self.inline_text_box_metrics(fragment.style(), 0.0)
                            .line_block_size
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                        inline_line_item_logical_block_size(item, block_style)
                    }
                };
                let scope_height = match item {
                    InlineLineItem::Fragment(fragment) => fragment.line_relative_scope(),
                    InlineLineItem::Atom(atom) => atom.line_relative_scope(),
                    InlineLineItem::Float(_) => None,
                }
                .map(|scope| {
                    self.inline_text_box_metrics(scope.line_relative_style(), 0.0)
                        .line_block_size
                })
                .unwrap_or(0.0);
                height = height.max(participant_height.max(scope_height));
                continue;
            }
            // Inline-edge atoms are lexical/paint markers for a regular
            // inline box, not independent block-axis participants. Their
            // text fragments already contribute the box's line-height; using
            // an edge again for `vertical-align: top|center|bottom` can make
            // a normal inline enlarge its own line through a separate raw
            // `line-height: normal` resolution.
            // <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
            if matches!(
                item,
                InlineLineItem::Atom(atom)
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    )
            ) {
                continue;
            }
            if Self::inline_line_item_has_line_relative_baseline_shift(item) {
                height = height.max(match item {
                    // Use the same resolved line-box extent that baseline
                    // participants use. `line-height: normal` is finalized
                    // by `inline_text_box_metrics`; consulting the font
                    // system separately can select a larger raw font metric
                    // and make a `vertical-align: bottom` inline enlarge the
                    // line that it is meant to align to.
                    // <https://www.w3.org/TR/css-inline-3/#line-height-property>
                    InlineLineItem::Fragment(fragment) => {
                        self.inline_text_box_metrics(fragment.style(), 0.0)
                            .line_block_size
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
    for item in items.iter_mut() {
        item.advance.clear_boundary_before();
    }

    let mut previous_unit = None::<VisualTrackingUnit>;
    let mut sequence_is_open = true;
    for item_index in 0..items.len() {
        let current = match &items[item_index].item {
            // Formatting controls remain in the source stream for UAX #9 and
            // OpenType context, but are absent when CSS Text counts
            // typographic character units. In particular, `A<cf>B` owns the
            // same single boundary as `AB`.
            InlineLineItem::Fragment(fragment)
                if crate::text::text_is_inter_character_control_only(fragment.text()) =>
            {
                continue;
            }
            InlineLineItem::Fragment(fragment)
                if inline_fragment_is_inter_character_unit(fragment) =>
            {
                fragment
                    .tracking_scope()
                    .cloned()
                    .map(|_scope| VisualTrackingUnit {
                        kind: VisualTrackingUnitKind::Text,
                        first_item: item_index,
                        first_used_spacing: UsedLetterSpacing::new(
                            fragment
                                .tracking_scope()
                                .expect("scope was cloned above")
                                .letter_spacing(),
                        ),
                        last_used_spacing: UsedLetterSpacing::new(
                            fragment
                                .tracking_scope()
                                .expect("scope was cloned above")
                                .letter_spacing(),
                        ),
                        // A preserved tab's used advance already reaches a
                        // stop whose numeric period includes letter spacing.
                        // It therefore cannot also receive a deferred
                        // tracking boundary before or after the tab.
                        // <https://www.w3.org/TR/css-text-3/#tab-size-property>
                        text_allows_gap_before: crate::text::text_allows_inter_character_gap_before(
                            fragment.text(),
                        ) && !fragment.text().starts_with('\t'),
                        text_allows_gap_after: crate::text::text_allows_inter_character_gap_after(
                            fragment.text(),
                        ) && !fragment.text().ends_with('\t'),
                        starts_visual_fragment: fragment.starts_visual_fragment(),
                    })
            }
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_unit(atom) => atom
                .tracking_scope()
                .cloned()
                .map(|_scope| VisualTrackingUnit {
                    // Ruby columns retain the typographic units of their base
                    // text. Unlike replaced/atomic siblings, adjacent columns
                    // therefore do not collapse into one atomic run for
                    // tracking. CSS Ruby aligns annotations over that tracked
                    // base geometry.
                    // <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
                    kind: if matches!(atom.content(), InlineAtomContent::Ruby { .. }) {
                        VisualTrackingUnitKind::RubyColumn
                    } else {
                        VisualTrackingUnitKind::AtomicRun
                    },
                    first_item: item_index,
                    first_used_spacing: if atom.content().ignores_boundary_letter_spacing() {
                        UsedLetterSpacing::new(layout_pt(0.0))
                    } else {
                        UsedLetterSpacing::new(
                            atom.tracking_scope()
                                .expect("scope was cloned above")
                                .letter_spacing(),
                        )
                    },
                    last_used_spacing: if atom.content().ignores_boundary_letter_spacing() {
                        UsedLetterSpacing::new(layout_pt(0.0))
                    } else {
                        UsedLetterSpacing::new(
                            atom.tracking_scope()
                                .expect("scope was cloned above")
                                .letter_spacing(),
                        )
                    },
                    text_allows_gap_before: false,
                    text_allows_gap_after: false,
                    starts_visual_fragment: false,
                }),
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_transparent(atom) => {
                continue;
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                sequence_is_open = false;
                continue;
            }
            InlineLineItem::Fragment(_) => continue,
        };
        let Some(current) = current else {
            sequence_is_open = false;
            continue;
        };

        if current.kind == VisualTrackingUnitKind::AtomicRun
            && sequence_is_open
            && matches!(
                previous_unit.as_ref().map(|unit| unit.kind),
                Some(VisualTrackingUnitKind::AtomicRun)
            )
        {
            previous_unit
                .as_mut()
                .expect("checked atomic run")
                .last_used_spacing = current.last_used_spacing;
        } else {
            if let Some(previous) = &previous_unit {
                apply_visual_tracking_boundary(items, previous, &current);
            }
            previous_unit = Some(current);
        }
        sequence_is_open = true;
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
        MeasuredInlineItem::new(InlineLineItem::Fragment(fragment), width, None)
    }

    fn tracked_canvas_atom(style: ComputedStyle, scope: Rc<InlineTrackingScope>) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::Canvas,
            style,
            None,
            InlineSize::new(20.0, 20.0),
            20.0,
            0.0,
            None,
            None,
        )
        .with_synthesized_margin_box_block_end_baseline()
        .with_tracking_scope(scope)
    }

    fn tracked_text_combine_atom(
        style: ComputedStyle,
        scope: Rc<InlineTrackingScope>,
    ) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::TextCombineUpright {
                composition: Box::new(TextCombineComposition {
                    source: TextCombineSource {
                        boundary_text: "12".into(),
                        style: Box::new(style.clone()),
                    },
                    layout: TextCombineLayout {
                        sequence: InlineLineSequence::default(),
                        horizontal_style: Box::new(style.clone()),
                        inline_scale: 1.0,
                    },
                    square: TextCombineSquareGeometry::new(layout_pt(20.0)),
                }),
            },
            style,
            None,
            InlineSize::new(20.0, 20.0),
            0.0,
            0.0,
            None,
            None,
        )
        .with_text_combine_parent_central_baseline()
        .with_tracking_scope(scope)
    }

    #[test]
    fn zero_tracking_keeps_shared_fragment_and_atom_data() {
        let style = ComputedStyle::initial();
        let scope = InlineTrackingScope::root(&style);
        let source_fragment = tracked_fragment("A", style.clone(), Rc::clone(&scope));
        let source_atom = tracked_canvas_atom(style, scope);
        let mut items = vec![
            measured_fragment(source_fragment.clone(), 10.0),
            MeasuredInlineItem::new(InlineLineItem::Atom(source_atom.clone()), 20.0, None),
        ];

        apply_visual_tracking_boundaries(&mut items);

        let InlineLineItem::Fragment(fragment) = &items[0].item else {
            panic!("test setup creates text")
        };
        let InlineLineItem::Atom(atom) = &items[1].item else {
            panic!("test setup creates an atom")
        };
        assert!(Rc::ptr_eq(&source_fragment.data, &fragment.data));
        assert!(Rc::ptr_eq(&source_atom.data, &atom.data));
        assert_eq!(items[0].advance.boundary_before().points(), 0.0);
        assert_eq!(items[1].advance.boundary_before().points(), 0.0);
    }

    #[test]
    fn applying_visual_tracking_twice_is_idempotent() {
        let style = tracked_style(11.0);
        let scope = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(
                tracked_fragment("a", style.clone(), Rc::clone(&scope)),
                10.0,
            ),
            measured_fragment(tracked_fragment("b", style, scope), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);
        let first_widths: Vec<_> = items
            .iter()
            .map(|item| item.used_advance().points())
            .collect();
        let first_tracking: Vec<_> = items
            .iter()
            .map(|item| item.advance.boundary_before().points())
            .collect();

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(
            items
                .iter()
                .map(|item| item.used_advance().points())
                .collect::<Vec<_>>(),
            first_widths
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.advance.boundary_before().points())
                .collect::<Vec<_>>(),
            first_tracking
        );
    }

    #[test]
    fn text_combine_keeps_lexical_scope_but_uses_zero_outgoing_tracking() {
        let style = tracked_style(10.0);
        let scope = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(
                tracked_fragment("A", style.clone(), Rc::clone(&scope)),
                10.0,
            ),
            MeasuredInlineItem::new(
                InlineLineItem::Atom(tracked_text_combine_atom(style.clone(), Rc::clone(&scope))),
                20.0,
                None,
            ),
            measured_fragment(tracked_fragment("B", style, scope), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 10.0);
        assert_eq!(items[2].advance.boundary_before().points(), 0.0);
    }

    #[test]
    fn positive_baseline_shift_raises_text_extents() {
        let extents = InlineBaselineExtents::from_shifted_baseline_and_block_size(12.0, 20.0, 5.0);

        assert_eq!(extents.baseline_offset, 17.0);
        assert_eq!(extents.descent, 3.0);
    }

    #[test]
    fn tcy_square_uses_the_parent_text_central_baseline() {
        let metrics = InlineTextBoxMetrics {
            content_block_size: 60.0,
            content_baseline_offset: 48.0,
            line_block_size: 60.0,
            block_start_leading: 0.0,
            block_end_leading: 0.0,
            line_baseline_offset: 48.0,
        };

        let resolved = TextCombineSquareGeometry::new(layout_pt(60.0)).resolve(metrics, 0.0);
        let extents = resolved.baseline_extents;

        // The parent central baseline lies 18pt above its alphabetic
        // baseline. Aligning a 60pt composition square to it preserves the
        // parent's 48pt-over / 12pt-under text extent, rather than expanding
        // the line with a symmetric 30pt / 30pt extent.
        assert_eq!(extents.baseline_offset, 48.0);
        assert_eq!(extents.descent, 12.0);
        assert_eq!(resolved.paint_placement_baseline_offset, 48.0);
    }

    #[test]
    fn tcy_square_applies_baseline_shift_after_central_alignment() {
        let metrics = InlineTextBoxMetrics {
            content_block_size: 60.0,
            content_baseline_offset: 48.0,
            line_block_size: 60.0,
            block_start_leading: 0.0,
            block_end_leading: 0.0,
            line_baseline_offset: 48.0,
        };

        let extents = TextCombineSquareGeometry::new(layout_pt(60.0))
            .resolve(metrics, -7.0)
            .baseline_extents;

        assert_eq!(extents.baseline_offset, 41.0);
        assert_eq!(extents.descent, 19.0);
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

    fn containing_style_with_block_margins(
        writing_mode: WritingMode,
        block_start: f32,
        block_end: f32,
    ) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.writing_mode = writing_mode;
        match writing_mode {
            WritingMode::HorizontalTb => {
                style.margin.top = block_start;
                style.margin.bottom = block_end;
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                style.margin.right = block_start;
                style.margin.left = block_end;
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                style.margin.left = block_start;
                style.margin.right = block_end;
            }
        }
        style
    }

    #[test]
    fn exported_atomic_baselines_include_logical_block_start_margin() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let style = containing_style_with_block_margins(writing_mode, 3.0, 4.0);
            let size = match writing_mode {
                WritingMode::HorizontalTb => InlineSize::new(10.0, 27.0),
                WritingMode::VerticalRl | WritingMode::VerticalLr => InlineSize::new(27.0, 10.0),
                WritingMode::SidewaysRl | WritingMode::SidewaysLr => unreachable!(),
            };
            let atom = InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(
                    InlineTextBoundarySpacing::new(layout_pt(size.width)),
                )),
                style.clone(),
                None,
                size,
                7.0,
                0.0,
                None,
                None,
            );

            assert_eq!(
                inline_atom_logical_margin_box_baseline_offset(&atom, &style).points(),
                10.0,
                "{writing_mode:?} projects its block-start margin"
            );
            let extents = LayoutBuilder::inline_atom_line_baseline_extents(&atom, &style);
            assert_eq!(extents.baseline_offset, 10.0);
            assert_eq!(extents.descent, 17.0);
        }
    }

    #[test]
    fn inline_table_line_metrics_include_wrapper_margin_but_replay_uses_table_box() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for (block_start, expected_margin_box_baseline, expected_descent) in
                [(3.0, 10.0, 17.0), (-3.0, 4.0, 23.0)]
            {
                let style = containing_style_with_block_margins(writing_mode, block_start, 4.0);
                let size = match writing_mode {
                    WritingMode::HorizontalTb => InlineSize::new(10.0, 27.0),
                    WritingMode::VerticalRl | WritingMode::VerticalLr => {
                        InlineSize::new(27.0, 10.0)
                    }
                    WritingMode::SidewaysRl | WritingMode::SidewaysLr => unreachable!(),
                };
                let atom = InlineAtom::new(
                    InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(
                        InlineTextBoundarySpacing::new(layout_pt(size.width)),
                    )),
                    style.clone(),
                    None,
                    size,
                    7.0,
                    0.0,
                    None,
                    None,
                )
                .with_exported_table_box_baseline();

                assert_eq!(
                    inline_atom_logical_margin_box_baseline_offset(&atom, &style).points(),
                    expected_margin_box_baseline,
                    "{writing_mode:?} must preserve the signed inline-table wrapper block-start margin in line metrics"
                );
                assert_eq!(
                    inline_atom_logical_content_placement_baseline_offset(&atom, &style).points(),
                    7.0,
                    "{writing_mode:?} must replay the inline table from its table-box baseline"
                );
                let extents = LayoutBuilder::inline_atom_line_baseline_extents(&atom, &style);
                assert_eq!(extents.baseline_offset, expected_margin_box_baseline);
                assert_eq!(extents.descent, expected_descent);
            }
        }
    }

    #[test]
    fn synthesized_atomic_baseline_uses_logical_border_box_block_end() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let style = containing_style_with_block_margins(writing_mode, 3.0, 4.0);
            let size = match writing_mode {
                WritingMode::HorizontalTb => InlineSize::new(10.0, 27.0),
                WritingMode::VerticalRl | WritingMode::VerticalLr => InlineSize::new(27.0, 10.0),
                WritingMode::SidewaysRl | WritingMode::SidewaysLr => unreachable!(),
            };
            let atom = InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(
                    InlineTextBoundarySpacing::new(layout_pt(size.width)),
                )),
                style.clone(),
                None,
                size,
                7.0,
                0.0,
                None,
                None,
            )
            .with_synthesized_border_box_block_end_baseline();

            assert_eq!(
                inline_atom_logical_margin_box_baseline_offset(&atom, &style).points(),
                23.0,
                "{writing_mode:?} projects its synthesized block-end baseline"
            );
            let extents = LayoutBuilder::inline_atom_line_baseline_extents(&atom, &style);
            assert_eq!(extents.baseline_offset, 23.0);
            assert_eq!(extents.descent, 4.0);
        }
    }

    #[test]
    fn tracking_uses_the_preceding_unit_and_is_leading_only() {
        let root_style = tracked_style(11.0);
        let child_style = tracked_style(3.0);
        let root = InlineTrackingScope::root(&root_style);
        let child = InlineTrackingScope::child(Rc::clone(&root), &child_style);
        let mut items = vec![
            measured_fragment(tracked_fragment("a", child_style, child), 10.0),
            measured_fragment(tracked_fragment("b", root_style, Rc::clone(&root)), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[0].advance.boundary_before().points(), 0.0);
        assert_eq!(items[1].advance.boundary_before().points(), 3.0);
        assert_eq!(items[0].base_advance().points(), 10.0);
        assert_eq!(items[1].base_advance().points(), 10.0);
        assert_eq!(items[1].used_advance().points(), 13.0);
    }

    #[test]
    fn following_tracking_does_not_affect_a_preceding_boundary() {
        let zero_style = tracked_style(0.0);
        let tracked_style = tracked_style(12.0);
        let zero_scope = InlineTrackingScope::root(&zero_style);
        let tracked_scope = InlineTrackingScope::root(&tracked_style);
        let mut items = vec![
            measured_fragment(tracked_fragment("a", zero_style, zero_scope), 10.0),
            measured_fragment(tracked_fragment("b", tracked_style, tracked_scope), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 0.0);
        assert_eq!(items[1].used_advance().points(), 10.0);
    }

    #[test]
    fn negative_tracking_remains_signed_and_never_clamps_the_base() {
        let style = tracked_style(-12.0);
        let scope = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(
                tracked_fragment("a", style.clone(), Rc::clone(&scope)),
                10.0,
            ),
            measured_fragment(tracked_fragment("b", style, scope), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), -12.0);
        assert_eq!(items[1].base_advance().points(), 10.0);
        assert_eq!(items[1].used_advance().points(), -2.0);
    }

    #[test]
    fn replacing_a_reshaped_base_preserves_boundary_spacing() {
        let mut advance = MeasuredInlineAdvance::from_base_points(10.0);
        advance.set_boundary_before(InlineBoundaryAdvance::between(
            UsedLetterSpacing::new(layout_pt(8.0)),
            UsedLetterSpacing::new(layout_pt(12.0)),
        ));

        advance.replace_base_points(15.0);

        assert_eq!(advance.base().points(), 15.0);
        assert_eq!(advance.boundary_before().points(), 8.0);
        assert_eq!(advance.used().points(), 23.0);
    }

    #[test]
    fn terminal_tracking_is_removed_before_a_nested_inline_boundary() {
        let style = tracked_style(11.0);
        let root = InlineTrackingScope::root(&style);
        let child = InlineTrackingScope::child(Rc::clone(&root), &style);
        let mut items = vec![
            measured_fragment(tracked_fragment("a", style.clone(), child), 10.0),
            measured_fragment(tracked_fragment("b", style, root), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[0].base_advance().points(), 10.0);
        assert_eq!(items[1].base_advance().points(), 10.0);
        assert_eq!(items[1].used_advance().points(), 21.0);
    }

    #[test]
    fn tracking_uses_the_final_visual_sequence() {
        let style = tracked_style(11.0);
        let root = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(tracked_fragment("b", style.clone(), Rc::clone(&root)), 10.0),
            measured_fragment(tracked_fragment("a", style, root), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[0].advance.boundary_before().points(), 0.0);
        assert_eq!(items[1].advance.boundary_before().points(), 11.0);
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
            measured_fragment(tracked_fragment("A", style.clone(), Rc::clone(&root)), 10.0),
            MeasuredInlineItem::new(InlineLineItem::Atom(atom()), 20.0, None),
            MeasuredInlineItem::new(InlineLineItem::Atom(atom()), 20.0, None),
            measured_fragment(tracked_fragment("D", style, root), 10.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 11.0);
        assert_eq!(items[2].advance.boundary_before().points(), 0.0);
        assert_eq!(items[3].advance.boundary_before().points(), 11.0);
        assert_eq!(
            items
                .iter()
                .map(|item| item.used_advance().points())
                .sum::<f32>(),
            82.0
        );
    }

    #[test]
    fn tracking_does_not_cross_a_joining_text_boundary() {
        let style = tracked_style(11.0);
        let scope = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(
                tracked_fragment("س", style.clone(), Rc::clone(&scope)),
                21.0,
            ),
            measured_fragment(tracked_fragment("ل", style, scope), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 0.0);
    }

    #[test]
    fn formatting_controls_are_skipped_between_visible_tracking_units() {
        let style = tracked_style(11.0);
        let scope = InlineTrackingScope::root(&style);
        let mut items = vec![
            measured_fragment(
                tracked_fragment("a", style.clone(), Rc::clone(&scope)),
                21.0,
            ),
            measured_fragment(
                tracked_fragment("\u{200e}", style.clone(), Rc::clone(&scope)),
                0.0,
            ),
            measured_fragment(tracked_fragment("b", style, scope), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 0.0);
        assert_eq!(items[2].advance.boundary_before().points(), 11.0);
    }

    #[test]
    fn tracking_does_not_reconnect_a_new_visual_fragment() {
        let style = tracked_style(11.0);
        let scope = InlineTrackingScope::root(&style);
        let mut second = tracked_fragment("b", style.clone(), Rc::clone(&scope));
        second.mark_starts_visual_fragment();
        let mut items = vec![
            measured_fragment(tracked_fragment("a", style, scope), 21.0),
            measured_fragment(second, 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[1].advance.boundary_before().points(), 0.0);
    }

    #[test]
    fn transparent_atoms_preserve_the_text_tracking_sequence() {
        let style = tracked_style(11.0);
        let scope = InlineTrackingScope::root(&style);
        let transparent = InlineAtom::new(
            InlineAtomContent::StaticPositionHypothetical {
                source: InlineStaticPositionSourceId::Block,
                boundary: StaticPositionHypotheticalBoundary::Transparent,
            },
            style.clone(),
            None,
            InlineSize::new(0.0, 0.0),
            0.0,
            0.0,
            None,
            None,
        );
        let mut items = vec![
            measured_fragment(
                tracked_fragment("a", style.clone(), Rc::clone(&scope)),
                21.0,
            ),
            MeasuredInlineItem::new(InlineLineItem::Atom(transparent), 0.0, None),
            measured_fragment(tracked_fragment("b", style, scope), 21.0),
        ];

        apply_visual_tracking_boundaries(&mut items);

        assert_eq!(items[2].advance.boundary_before().points(), 11.0);
    }
}
