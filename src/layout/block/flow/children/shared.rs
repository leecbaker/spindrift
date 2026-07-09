use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn split_inline_static_position_y_offset_before_child(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        child_box_index: usize,
        block_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Option<f32> {
        let previous = (0..child_box_index).rev().find_map(|index| {
            let previous = child_boxes.get(index)?;
            (!formatting_box_is_out_of_flow_positioned(previous)).then_some(previous)
        })?;
        if !matches!(previous, box_tree::FormattingBox::Inline(_)) {
            return None;
        }

        let mut items = Vec::new();
        self.collect_intrinsic_inline_box_items(
            std::slice::from_ref(previous),
            stylesheets,
            None,
            IntrinsicInlineCollectionContext {
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                block_style,
                propagated_decoration: block_style.text_decoration.clone(),
            },
            &mut items,
        );
        let has_inline_content = items.iter().any(|item| match item {
            InlineItem::Word(_) => !inline_item_is_collapsible_space(item),
            InlineItem::Atom(_) => true,
            InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => false,
        });
        has_inline_content.then(|| {
            self.block_static_position_y_offset_from_split_inline_items(items, block_style)
        })
    }

    pub(in crate::layout) fn block_static_position_y_offset_from_split_inline_items(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
    ) -> f32 {
        if !matches!(items.last(), Some(InlineItem::Break(_))) {
            items.push(InlineItem::Break(InlineBreak::default()));
        }
        items.push(InlineItem::Atom(Box::new(
            self.block_static_position_placeholder_atom(block_style),
        )));
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            block_style,
            self.current_content_logical_inline_size().max(1.0),
            0.0,
            0.0,
        );
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let mut offset = 0.0;
        for record in &records {
            if record.fragment.as_ref().is_some_and(|fragment| {
                fragment.items().iter().any(|item| {
                    matches!(
                        &item.item,
                        InlineLineItem::Atom(atom)
                            if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                    )
                })
            }) {
                return offset;
            }
            offset += record.height();
        }
        0.0
    }

    pub(in crate::layout) fn text_box_trim_dom_child_node_index(
        &mut self,
        element: &Element,
        sibling_tags: &ElementSiblingSignatureList,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        block_start: bool,
        find_last: bool,
    ) -> Option<usize> {
        let mut element_index = 0usize;
        let mut child_styles = Vec::new();
        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                continue;
            };
            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(parent_style),
            );
            child_styles.push((child_node_index, child_element, child_style));
        }
        if find_last {
            child_styles.reverse();
        }
        for (child_node_index, child_element, child_style) in child_styles {
            let Some(accepts) =
                dom_element_text_box_trim_reach(child_element, &child_style, block_start)
            else {
                continue;
            };
            return accepts.then_some(child_node_index);
        }
        None
    }
}

/// Consume pending normal-flow spacing when the next generated box is a float.
///
/// A float is out of flow, so its margins do not collapse with the preceding
/// block's adjoining margin. The pending block margin still contributes to the
/// float's hypothetical block-start position and is therefore added to the
/// float's own block-start margin.
/// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
pub(in crate::layout) fn apply_pending_normal_flow_margin_before_float(
    style: &mut ComputedStyle,
    pending_margin: Option<f32>,
) {
    if style.float != Float::None
        && let Some(pending_margin) = pending_margin
    {
        style.margin.top += pending_margin;
    }
}

/// Preserve block-axis margins that this flow pass has already resolved.
///
/// The child pass adjusts the used margins for collapsing, trimming, and
/// fragmentainer replay. Re-entering block layout resolves `box_values` again,
/// so retain the adjusted values there as fixed lengths as well.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn preserve_adjusted_block_margins(style: &mut ComputedStyle) {
    style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(style.margin.top),
    );
    style.box_values.margin.bottom = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(style.margin.bottom),
    );
}

pub(in crate::layout) fn formatting_box_is_out_of_flow_positioned(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    box_.element_parts().is_some_and(|(_, _, style, _)| {
        matches!(style.position, Position::Absolute | Position::Fixed)
    })
}

pub(in crate::layout) fn block_avoid_break_flow_child(
    parent_element: &Element,
    child_element: &Element,
    child_style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(child_element, child_style)
        || is_document_canvas_element(parent_element)
        || is_replaced_element(child_element)
}

pub(in crate::layout) fn next_formatting_box_flow_child_break_before(
    parent_element: &Element,
    child_boxes: &[box_tree::FormattingBox<'_>],
    current_index: usize,
) -> Option<PageBreak> {
    for child in child_boxes.iter().skip(current_index + 1) {
        if matches!(child, box_tree::FormattingBox::AnonymousBlock(_)) {
            return Some(PageBreak::Auto);
        }
        let Some((child_element, _, child_style, _)) = child.element_parts() else {
            continue;
        };
        if block_avoid_break_flow_child(parent_element, child_element, child_style) {
            return Some(child_style.break_before);
        }
        if !style_is_in_normal_flow(child_style) {
            // Floats and positioned boxes do not participate in the class A
            // boundary between the surrounding in-flow block siblings. Look
            // through them so later `break-before: avoid` pressure can arm a
            // rollback candidate before the preceding in-flow sibling.
            // <https://www.w3.org/TR/css-break-3/#possible-breaks>
            continue;
        }
        return None;
    }
    Some(PageBreak::Auto)
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn next_dom_flow_child_break_before(
        &mut self,
        parent_element: &Element,
        current_node_index: usize,
        next_element_index: usize,
        sibling_tags: &ElementSiblingSignatureList,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Option<PageBreak> {
        for child in parent_element.children.iter().skip(current_node_index + 1) {
            let NodeKind::Element(child_element) = &child.kind else {
                continue;
            };
            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                next_element_index,
                sibling_tags.clone(),
            );
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(parent_style),
            );
            if block_avoid_break_flow_child(parent_element, child_element, &child_style) {
                return Some(child_style.break_before);
            }
            if !style_is_in_normal_flow(&child_style) {
                continue;
            }
            return None;
        }
        Some(PageBreak::Auto)
    }

    /// Return whether a following BFC root separates an adjoining float replay.
    ///
    /// CSS 2.2 makes block formatting context roots avoid earlier float margin
    /// boxes. If an adjoining-margin replay would put the next BFC root beside
    /// floats that leave no fitting band, the BFC root is separated in the
    /// same spirit as clearance and the collapsed margin must not drag those
    /// floats downward:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
    pub(in crate::layout) fn adjoining_float_replay_separated_by_bfc_root(
        &mut self,
        replay: &AdjoiningFloatReplayCandidate,
        child_element: &Element,
        child_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        replay_origin_y: f32,
    ) -> bool {
        let establishes_independent_bfc =
            child_style.display.establishes_block_formatting_context()
                || self.element_used_overflow_clips(child_element, child_style)
                || block_align_content_establishes_independent_formatting_context(
                    child_style.align_content,
                );
        if !establishes_independent_bfc
            || child_style.float != Float::None
            || matches!(child_style.position, Position::Absolute | Position::Fixed)
        {
            return false;
        }

        let snapshot = replay.snapshot();
        if snapshot.pages.len() != self.pages.len()
            || snapshot.float_contexts.len() != self.float_contexts.len()
        {
            return true;
        }

        let Some(snapshot_context) = snapshot.float_contexts.last() else {
            return true;
        };
        let Some(current_context) = self.float_contexts.last() else {
            return true;
        };
        if current_context.shapes.len() <= snapshot_context.shapes.len() {
            return false;
        }

        if snapshot.containing_block_writing_mode != WritingMode::HorizontalTb
            || child_style.writing_mode != WritingMode::HorizontalTb
        {
            return true;
        }

        let delta_y = replay_origin_y - snapshot.cursor_y;
        let mut replayed_context = snapshot_context.clone();
        replayed_context.shapes.extend(
            current_context.shapes[snapshot_context.shapes.len()..]
                .iter()
                .cloned()
                .map(|shape| shape.translated_block(delta_y)),
        );

        let containing_left = snapshot.content_left;
        let containing_right = snapshot.content_right;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        let page_index = snapshot.pages.len();
        let placement = replayed_context.avoiding_bfc_root_position(
            page_index,
            replay_origin_y,
            child_style.clear,
            child_style.writing_mode,
            child_style.direction,
            containing_left,
            containing_right,
            |band_left, band_width, _candidate_top| {
                let band_right = band_left + band_width;
                let avoidance_left = if band_left > containing_left + FLOAT_EPSILON {
                    band_left
                } else {
                    containing_left
                };
                let avoidance_right = if band_right < containing_right - FLOAT_EPSILON {
                    band_right
                } else {
                    containing_right
                };
                let candidate_geometry = self.block_layout_geometry_in_inline_span(
                    child_element,
                    child_style,
                    stylesheets,
                    child_boxes,
                    BlockLayoutInlineConstraint {
                        containing_left: avoidance_left,
                        containing_right: avoidance_right,
                        percentage_basis: PercentageBasis::definite(LogicalInlineContentSize::new(
                            content_box_pt(containing_inline_size),
                        )),
                        physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                            containing_inline_size,
                        )),
                        auto_border_box_width: (band_width
                            < containing_inline_size - FLOAT_EPSILON)
                            .then_some(border_box_pt(
                                (band_width - child_style.margin.left - child_style.margin.right)
                                    .max(0.0),
                            )),
                    },
                );
                let candidate_style = &candidate_geometry.style;
                let estimated_outer_height = self
                    .estimate_element_height(
                        child_element,
                        candidate_style,
                        stylesheets,
                        candidate_geometry.outer_width(),
                        child_boxes,
                    )
                    .unwrap_or(
                        candidate_style.margin.top
                            + candidate_style.line_height
                            + candidate_style.margin.bottom,
                    );
                let border_box_height = (estimated_outer_height
                    - candidate_style.margin.top
                    - candidate_style.margin.bottom)
                    .max(0.0);
                FloatAvoidingBfcMeasurement {
                    border_box_left: candidate_geometry.outer_inline().start()
                        - candidate_geometry.relative_offset.x(),
                    border_box_width: candidate_geometry.outer_width(),
                    border_box_height,
                }
            },
        );

        placement.top < replay_origin_y - FLOAT_EPSILON
    }
}

pub(in crate::layout) fn text_box_trim_formatting_box_child_index(
    child_boxes: &[box_tree::FormattingBox<'_>],
    block_start: bool,
    find_last: bool,
) -> Option<usize> {
    let candidate = if find_last {
        child_boxes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, child)| {
                text_box_trim_formatting_box_reach(child, block_start)
                    .map(|accepts| (index, accepts))
            })
    } else {
        child_boxes.iter().enumerate().find_map(|(index, child)| {
            text_box_trim_formatting_box_reach(child, block_start).map(|accepts| (index, accepts))
        })
    };
    candidate.and_then(|(index, accepts)| accepts.then_some(index))
}

pub(in crate::layout) fn text_box_trim_formatting_box_reach(
    box_: &box_tree::FormattingBox<'_>,
    block_start: bool,
) -> Option<bool> {
    if !formatting_box_is_in_normal_flow(box_) || formatting_box_is_zero_height_page_boundary(box_)
    {
        return None;
    }
    match box_ {
        box_tree::FormattingBox::AnonymousBlock(_) => Some(true),
        box_tree::FormattingBox::InlineSplitBlockContext(context)
            if context.children.len() == 1 =>
        {
            text_box_trim_formatting_box_reach(&context.children[0], block_start)
        }
        box_tree::FormattingBox::Block(box_) => Some(
            matches!(
                element_layout_kind(box_.element, &box_.style),
                ElementLayoutKind::BlockFlow
            ) && style_allows_text_box_trim_propagation(&box_.style, block_start),
        ),
        box_tree::FormattingBox::Inline(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Text(_) => None,
        box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => Some(false),
    }
}

pub(in crate::layout) fn dom_element_text_box_trim_reach(
    element: &Element,
    style: &ComputedStyle,
    block_start: bool,
) -> Option<bool> {
    if !style_is_in_normal_flow(style) {
        return None;
    }
    let layout_kind = element_layout_kind(element, style);
    if matches!(layout_kind, ElementLayoutKind::BlockFlow) {
        return Some(style_allows_text_box_trim_propagation(style, block_start));
    }
    (is_replaced_element(element) || style.display.is_block_level()).then_some(false)
}

pub(in crate::layout) fn style_allows_text_box_trim_propagation(
    style: &ComputedStyle,
    block_start: bool,
) -> bool {
    let side = if block_start {
        block_start_side(style.writing_mode)
    } else {
        block_end_side(style.writing_mode)
    };
    physical_edge_value(style.padding, side) <= 0.0
        && physical_edge_value(used_border_widths(style), side) <= 0.0
}

pub(in crate::layout) fn physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}
