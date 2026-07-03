use super::*;
use crate::css::{Edges, LineFitEdge, TextBoxTrim};
use std::rc::Rc;

fn inline_fragment_uses_text_edge_layout(fragment: &InlineFragment) -> bool {
    !matches!(fragment.style().text_box_trim, TextBoxTrim::None)
        || !matches!(fragment.style().line_fit_edge, LineFitEdge::Leading)
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
/// policy and centers that content area inside the line-height box with
/// half-leading:
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineTextBoxMetrics {
    pub(in crate::layout) content_block_size: f32,
    pub(in crate::layout) content_baseline_offset: f32,
    pub(in crate::layout) line_block_size: f32,
    pub(in crate::layout) half_leading: f32,
    pub(in crate::layout) line_baseline_offset: f32,
}

impl<'a> LayoutBuilder<'a> {
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
    ) -> Vec<MeasuredInlineItem> {
        if items
            .iter()
            .all(|item| matches!(item.as_ref(), InlineLineItem::Fragment(_)))
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
            return items
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
                .collect();
        }
        if !mixed_inline_line_needs_bidi_ordering(items, block_style) {
            return items.to_vec();
        }
        let (text, ranged_items) = mixed_measured_inline_line_bidi_text(items);
        let visual_ranges = split_mixed_inline_visual_ranges_at_box_edges(
            normalize_mixed_inline_visual_ranges(
                &text,
                self.font_system
                    .visual_ranges_for_unwrapped_text(&text, block_style),
            ),
            &ranged_items,
            &text,
        );
        let mut output = Vec::new();
        let mut emitted = vec![false; ranged_items.len()];
        for visual_range in visual_ranges {
            self.push_mixed_inline_box_edges_at_visual_boundary(
                &ranged_items,
                visual_range.start,
                true,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_box_edges_at_visual_boundary(
                &ranged_items,
                visual_range.end,
                true,
                &mut emitted,
                &mut output,
            );
            for (index, ranged) in ranged_items.iter().enumerate() {
                let start = ranged.range.start.max(visual_range.start);
                let end = ranged.range.end.min(visual_range.end);
                if start >= end {
                    continue;
                }
                if let Some(item) = self.measured_visual_item_slice(ranged, start, end, block_style)
                {
                    output.push(item);
                    emitted[index] = true;
                }
            }
            self.push_mixed_inline_box_edges_at_visual_boundary(
                &ranged_items,
                visual_range.start,
                false,
                &mut emitted,
                &mut output,
            );
            self.push_mixed_inline_box_edges_at_visual_boundary(
                &ranged_items,
                visual_range.end,
                false,
                &mut emitted,
                &mut output,
            );
        }
        if output.is_empty() {
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
        }
    }

    pub(in crate::layout) fn measured_visual_item_slice(
        &mut self,
        ranged: &RangedMeasuredMixedInlineLineItem,
        start: usize,
        end: usize,
        block_style: &ComputedStyle,
    ) -> Option<MeasuredInlineItem> {
        match &ranged.item.item {
            InlineLineItem::Fragment(fragment) => {
                let relative_start = start - ranged.range.start;
                let relative_end = end - ranged.range.start;
                let mut text = char_boundary_slice(fragment.text(), relative_start..relative_end)?;
                text = text_without_bidi_format_controls(&text).into_owned();
                if text.is_empty() {
                    return None;
                }
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                fragment.set_text(text);
                hanging_edges.blocks_start = hanging_edges.blocks_start && relative_start == 0;
                hanging_edges.blocks_end =
                    hanging_edges.blocks_end && relative_end == ranged.range.len();
                fragment = fragment.with_hanging_edges(hanging_edges);
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

    pub(in crate::layout) fn push_mixed_inline_box_edges_at_visual_boundary(
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
                || !measured_item_is_transparent_mixed_inline_box_edge(&ranged.item)
                || mixed_inline_box_edge_precedes_visual_content(&ranged.item)
                    != Some(precedes_visual_content)
            {
                continue;
            }
            emitted[edge_index] = true;
            output.push(ranged.item.clone());
        }
    }

    /// Return a mixed inline item ascent/descent pair around its baseline.
    ///
    /// CSS Inline Layout defines line box height from the logical extents of
    /// inline-level boxes placed around the shared line baseline. Text
    /// fragments keep the CSS `line-height` logical box even when selected font
    /// ink metrics are taller; CSS 2.2 permits negative leading, so glyph ink
    /// can overflow without increasing the line box:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(in crate::layout) fn inline_line_item_baseline_extents(
        &mut self,
        item: &MeasuredInlineItem,
        block_style: &ComputedStyle,
    ) -> (f32, f32) {
        match &item.item {
            InlineLineItem::Fragment(fragment) => {
                let metrics = self.inline_text_box_metrics(
                    fragment.style(),
                    item.shaped.as_deref(),
                    fragment.baseline_shift,
                );
                if inline_fragment_uses_text_edge_layout(fragment) {
                    return self.inline_fragment_text_edge_baseline_extents(fragment, metrics);
                }
                let block_size = metrics.line_block_size;
                if block_size <= 0.0 {
                    return (0.0, 0.0);
                }
                let baseline = metrics.line_baseline_offset.max(0.0);
                let descent = (block_size - metrics.line_baseline_offset).max(0.0);
                (baseline, descent)
            }
            InlineLineItem::Atom(atom) => {
                Self::inline_atom_line_baseline_extents(atom, block_style)
            }
            InlineLineItem::Float(_) => (0.0, 0.0),
        }
    }

    fn inline_fragment_text_edge_baseline_extents(
        &mut self,
        fragment: &InlineFragment,
        metrics: InlineTextBoxMetrics,
    ) -> (f32, f32) {
        let use_line_fit_edge = !matches!(fragment.style().line_fit_edge, LineFitEdge::Leading);
        let pair = fragment.style().line_fit_edge.text_box_pair();
        let mut over_edge = if use_line_fit_edge {
            self.text_edge_over_position(fragment.style(), metrics, pair.over)
        } else {
            metrics.half_leading
        };
        let mut under_edge = if use_line_fit_edge {
            self.text_edge_under_position(fragment.style(), metrics, pair.under)
        } else {
            metrics.half_leading + metrics.content_block_size
        };
        let trim = self.inline_text_box_content_trim_for_style(fragment.style(), metrics);
        over_edge += trim.block_start;
        under_edge -= trim.block_end;
        if under_edge < over_edge {
            under_edge = over_edge;
        }
        let (block_start_extra, block_end_extra) =
            inline_fragment_block_axis_outer_extras(fragment.style(), use_line_fit_edge);
        let layout_over_edge = over_edge - block_start_extra;
        let layout_under_edge = under_edge + block_end_extra;
        (
            (metrics.line_baseline_offset - layout_over_edge).max(0.0),
            (layout_under_edge - metrics.line_baseline_offset).max(0.0),
        )
    }

    /// Return an atomic inline margin box's shifted extents around the line baseline.
    ///
    /// CSS 2.2 `vertical-align` shifts the whole inline-level box relative to
    /// the parent baseline. Line metrics must enclose the shifted margin-box
    /// top and bottom, instead of reusing the shifted baseline offset as the
    /// box's unshifted ascent; otherwise `vertical-align: middle` lowers an
    /// inline-block and incorrectly inflates the row advance:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn inline_atom_line_baseline_extents(
        atom: &InlineAtom,
        containing_style: &ComputedStyle,
    ) -> (f32, f32) {
        let block_size = inline_atom_logical_block_size(atom, containing_style);
        let unshifted_baseline = match containing_style.writing_mode {
            WritingMode::HorizontalTb => atom.style().margin.top + atom.baseline_offset,
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                inline_atom_logical_block_start_margin(atom, containing_style)
                    + inline_atom_logical_border_block_size(atom, containing_style)
            }
        };
        let baseline = (unshifted_baseline + atom.baseline_shift).max(0.0);
        let descent = (block_size - baseline).max(0.0);
        (baseline, descent)
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
            InlineLineItem::Fragment(fragment) => fragment.style().vertical_align,
            InlineLineItem::Atom(atom) => atom.style().vertical_align,
            InlineLineItem::Float(_) => VerticalAlign::BASELINE,
        };
        vertical_align.has_line_relative_baseline_shift()
    }

    fn inline_fragment_blocks_text_only_height_shortcut(fragment: &InlineFragment) -> bool {
        if inline_fragment_uses_text_edge_layout(fragment) {
            return true;
        }
        let vertical_align = fragment.style().vertical_align;
        !vertical_align.has_line_relative_baseline_shift()
            && vertical_align != VerticalAlign::BASELINE
    }

    fn inline_line_item_parent_content_edge_extents(
        &mut self,
        item: &MeasuredInlineItem,
        block_style: &ComputedStyle,
    ) -> Option<(f32, f32)> {
        let (vertical_align, block_size) = match &item.item {
            InlineLineItem::Fragment(fragment) => {
                let metrics =
                    self.inline_text_box_metrics(fragment.style(), item.shaped.as_deref(), 0.0);
                (fragment.style().vertical_align, metrics.line_block_size)
            }
            InlineLineItem::Atom(atom) => (
                atom.style().vertical_align,
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
        Some((baseline_offset, descent))
    }

    /// Return the parent line strut ascent/descent pair around its baseline.
    ///
    /// The strut participates in every inline formatting context line. Text
    /// painting in this renderer uses the selected-font ascent as the line
    /// baseline coordinate, while `line-height` remains the used block-axis
    /// line advance:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-inline-3/#line-height-property>.
    pub(in crate::layout) fn inline_style_line_extents(
        &mut self,
        style: &ComputedStyle,
        baseline_shift: f32,
    ) -> (f32, f32) {
        let metrics = self.inline_text_box_metrics(style, None, baseline_shift);
        let baseline = metrics.line_baseline_offset.max(0.0);
        let descent = metrics.line_block_size - metrics.line_baseline_offset;
        (baseline, descent)
    }

    pub(in crate::layout) fn inline_text_box_metrics(
        &mut self,
        style: &ComputedStyle,
        _shaped: Option<&ShapedInlineLine>,
        baseline_shift: f32,
    ) -> InlineTextBoxMetrics {
        let content_block_size = self
            .font_system
            .rendered_font_size_for_style(style)
            .max(0.0);
        let content_baseline_offset = self.inline_text_content_baseline_offset(style);
        let line_block_size = self.font_system.used_line_height(style).max(0.0);
        let half_leading = (line_block_size - content_block_size) / 2.0;
        let line_baseline_offset = half_leading + content_baseline_offset - baseline_shift;
        InlineTextBoxMetrics {
            content_block_size,
            content_baseline_offset,
            line_block_size,
            half_leading,
            line_baseline_offset,
        }
    }

    /// Return a text box baseline offset from the style's first available font.
    ///
    /// CSS 2.2 makes `line-height` establish the inline box used for baseline
    /// alignment, with glyph ink allowed to overflow that box. The baseline
    /// anchor therefore comes from the selected font for the style, not from a
    /// later fallback run that happened to shape one glyph:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn inline_text_content_baseline_offset(
        &mut self,
        style: &ComputedStyle,
    ) -> f32 {
        let font_id = self.font_system.resolve_style(style);
        let line_height = self.font_system.line_height_for_font(font_id, style);
        let adjustment =
            self.font_system
                .font_ascent_baseline_adjustment(font_id, style, line_height);
        style.font_size - adjustment
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
        let (baseline_offset, descent) =
            self.mixed_inline_line_baseline_extents(items, block_style);
        let non_baseline_aligned_height =
            self.mixed_inline_line_non_baseline_aligned_height(items, block_style);
        let text_only_height = self.mixed_inline_text_only_height(items, block_style);
        InlineLineMetrics {
            width,
            height: text_only_height
                .unwrap_or(baseline_offset + descent)
                .max(non_baseline_aligned_height),
            baseline_offset,
        }
    }

    fn mixed_inline_text_only_height(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
    ) -> Option<f32> {
        if !items.iter().all(|item| {
            matches!(
                item.as_ref(),
                InlineLineItem::Fragment(_) | InlineLineItem::Float(_)
            )
        }) {
            return None;
        }
        if items.iter().any(|item| match item.as_ref() {
            InlineLineItem::Fragment(fragment) => {
                Self::inline_fragment_blocks_text_only_height_shortcut(fragment)
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
        }) {
            return None;
        }
        Some(
            items
                .iter()
                .filter_map(|item| match item.as_ref() {
                    InlineLineItem::Fragment(fragment) => {
                        Some(self.font_system.used_line_height(fragment.style()))
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                })
                .fold(block_style.line_height, f32::max),
        )
    }

    pub(in crate::layout) fn mixed_inline_line_baseline_extents(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
    ) -> (f32, f32) {
        let (mut baseline_offset, mut descent) = self.inline_style_line_extents(block_style, 0.0);
        for item in items {
            if Self::inline_line_item_has_line_relative_baseline_shift(&item.item) {
                continue;
            }
            if let Some((item_baseline_offset, item_descent)) =
                self.inline_line_item_parent_content_edge_extents(item, block_style)
            {
                baseline_offset = baseline_offset.max(item_baseline_offset);
                descent = descent.max(item_descent);
                continue;
            }
            let (item_baseline_offset, item_descent) =
                self.inline_line_item_baseline_extents(item, block_style);
            baseline_offset = baseline_offset.max(item_baseline_offset);
            descent = descent.max(item_descent);
        }
        (baseline_offset, descent)
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
            if Self::inline_line_item_has_line_relative_baseline_shift(item) {
                height = height.max(match item {
                    InlineLineItem::Fragment(fragment) => {
                        self.font_system.used_line_height(fragment.style())
                    }
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                        inline_line_item_logical_block_size(item, block_style)
                    }
                });
            }
        }
        height
    }
}
