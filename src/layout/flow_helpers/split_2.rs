use super::*;
use crate::layout::block::ParentStartClearanceHypothesis;

pub(in crate::layout) fn has_later_normal_block_flow_child_with_resolver(
    element: &Element,
    start_element_index: usize,
    sibling_tags: &ElementSiblingSignatureList,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let mut element_index = 0usize;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let current_index = element_index;
        element_index += 1;
        if current_index < start_element_index {
            continue;
        }
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            current_index,
            sibling_tags.clone(),
        );
        let style = resolver.structural_style_for_element(
            child_element,
            signature,
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        if is_normal_block_flow_child(child_element, &style) || is_replaced_element(child_element) {
            return true;
        }
    }
    false
}

pub(in crate::layout) fn has_later_normal_block_flow_box_child(
    child_boxes: &[box_tree::FormattingBox<'_>],
    start_index: usize,
    parent: &Element,
    document_canvas: DocumentCanvasResolution,
) -> bool {
    child_boxes
        .iter()
        .skip(start_index)
        .any(|child| formatting_box_is_normal_block_flow_sibling(child, parent, document_canvas))
}

/// Return whether a formatting box is a later in-flow block sibling.
///
/// CSS 2.2 mixed-flow normalization creates anonymous block boxes around runs
/// of inline content. Those anonymous boxes are real block-level siblings for
/// normal-flow ordering and margin-adjacency decisions; a preceding block must
/// not collapse its end margin through the parent as if the anonymous block did
/// not exist:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
pub(in crate::layout) fn formatting_box_is_normal_block_flow_sibling(
    child: &box_tree::FormattingBox<'_>,
    parent: &Element,
    document_canvas: DocumentCanvasResolution,
) -> bool {
    match child {
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            style_is_in_normal_flow(&box_.style)
                && !formatting_box_can_only_create_phantom_line_boxes(child)
        }
        _ => child
            .element_parts()
            .is_some_and(|(child_element, _, child_style, _)| {
                // The document canvas can make an in-flow direct child a
                // block-flow sibling, but it cannot pull an absolute or
                // fixed child back into normal flow. In particular, root/body
                // endpoint selection, margin adjacency, and fragmentation
                // must look through positioned children.
                // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                style_is_in_normal_flow(child_style)
                    && (is_normal_block_flow_child(child_element, child_style)
                        || document_canvas.is_document_canvas_flow_element(parent)
                        || is_replaced_element(child_element))
            }),
    }
}

/// Collapse two adjoining signed layout margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn collapse_margins(
    first: LayoutLength,
    second: LayoutLength,
) -> LayoutLength {
    let first = first.points();
    let second = second.points();
    layout_pt(if first >= 0.0 && second >= 0.0 {
        first.max(second)
    } else if first <= 0.0 && second <= 0.0 {
        first.min(second)
    } else {
        first + second
    })
}

/// Collapses an adjoining set of vertical margins.
///
/// CSS 2.2 collapses an adjoining margin set to the maximum positive margin
/// plus the minimum negative margin. This differs from pairwise collapsing
/// when a set contains more than two mixed-sign margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
pub(in crate::layout) fn collapse_margin_set(
    margins: impl IntoIterator<Item = LayoutLength>,
) -> LayoutLength {
    let mut max_positive = 0.0f32;
    let mut min_negative = 0.0f32;
    for margin in margins {
        let margin = margin.points();
        if margin > max_positive {
            max_positive = margin;
        }
        if margin < min_negative {
            min_negative = margin;
        }
    }
    layout_pt(max_positive + min_negative)
}

pub(in crate::layout) fn page_start_margin(
    margin: LayoutLength,
    starts_at_page_top: bool,
) -> LayoutLength {
    if starts_at_page_top && margin.points() > 0.0 {
        layout_pt(0.0)
    } else {
        margin
    }
}

pub(in crate::layout) fn collapsed_start_margin_delta(
    previous_applied: LayoutLength,
    next: LayoutLength,
    starts_at_page_top: bool,
) -> LayoutLength {
    let collapsed = collapse_margins(previous_applied, next);
    layout_pt(page_start_margin(collapsed, starts_at_page_top).points() - previous_applied.points())
}

pub(in crate::layout) fn collapsed_margin_delta(
    previous_applied: LayoutLength,
    next: LayoutLength,
) -> LayoutLength {
    layout_pt(collapse_margins(previous_applied, next).points() - previous_applied.points())
}

/// The collapsed block-start margin set owned by one in-flow child.
///
/// An auto-height block and its first in-flow child's adjoining block-start
/// margins form one set. The child therefore carries only the additional local
/// margin needed after its parent or preceding sibling has already consumed a
/// portion of that set; it must not subtract its first descendant's margin a
/// second time.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct AdjoiningBlockStartMargin {
    collapsed: LayoutLength,
    descendant_deferred_to_child: LayoutLength,
}

/// The complete start-margin set a parent has provisionally adjoined with its
/// first in-flow child.  A cleared child also needs the parent border edge as
/// its `clear:none` counterfactual position.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug)]
pub(in crate::layout) struct InheritedAdjoiningStartMargin {
    complete_margin: LayoutLength,
    parent_start_clearance_hypothesis: ParentStartClearanceHypothesis,
}

impl InheritedAdjoiningStartMargin {
    pub(in crate::layout) fn new(
        complete_margin: LayoutLength,
        parent_border_edge: PageTopBlockPosition,
    ) -> Self {
        Self {
            complete_margin,
            parent_start_clearance_hypothesis: ParentStartClearanceHypothesis::new(
                parent_border_edge,
            ),
        }
    }

    pub(in crate::layout) fn complete_margin(self) -> LayoutLength {
        self.complete_margin
    }

    pub(in crate::layout) fn parent_start_clearance_hypothesis(
        self,
    ) -> ParentStartClearanceHypothesis {
        self.parent_start_clearance_hypothesis
    }
}

impl AdjoiningBlockStartMargin {
    /// Construct the margin set from a child's own margin and its adjoining
    /// first descendant's margin.
    ///
    /// CSS 2.2 collapses all adjoining positive margins to their maximum;
    /// negative margins follow the corresponding signed rule. The resulting
    /// value belongs to the whole adjoining set, not separately to each box.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_child_and_descendant(
        child_margin: LayoutLength,
        descendant_margin: Option<LayoutLength>,
    ) -> Self {
        let collapsed = descendant_margin
            .map(|descendant| collapse_margins(child_margin, descendant))
            .unwrap_or(child_margin);
        Self {
            collapsed,
            // When the descendant is itself the collapsed result, leave that
            // contribution to the child's own start-margin pass. The sibling
            // delta must then cancel the preceding margin without consuming
            // the descendant twice. Mixed-sign margin sets remain whole at
            // this boundary because neither individual margin represents the
            // collapsed result.
            descendant_deferred_to_child: descendant_margin
                .filter(|descendant| *descendant == collapsed)
                .unwrap_or_else(|| layout_pt(0.0)),
        }
    }

    /// Construct an already-collapsed set for a self-collapsing child.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_collapsed(collapsed: LayoutLength) -> Self {
        Self {
            collapsed,
            descendant_deferred_to_child: layout_pt(0.0),
        }
    }

    /// Return the collapsed value for bookkeeping that continues the same
    /// adjoining margin set through later transparent boxes.
    pub(in crate::layout) fn value(self) -> LayoutLength {
        self.collapsed
    }

    /// Return the child-local delta after the parent has consumed its
    /// block-start margin set.
    ///
    /// The parent collapses directly with the complete adjoining set, so
    /// page-start trimming applies before the local delta is computed.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn child_delta_at_parent_start(
        self,
        parent_applied: LayoutLength,
        starts_at_page_top: bool,
    ) -> LayoutLength {
        collapsed_start_margin_delta(parent_applied, self.collapsed, starts_at_page_top)
    }

    /// Return the child-local delta after a preceding sibling's adjoining
    /// block-end margin. When a first descendant itself supplies the
    /// collapsed value, defer that part to the child's own layout pass.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn child_delta_after_sibling(
        self,
        previous_sibling_margin: LayoutLength,
    ) -> LayoutLength {
        layout_pt(
            collapsed_margin_delta(previous_sibling_margin, self.collapsed).points()
                - self.descendant_deferred_to_child.points(),
        )
    }
}

/// Applies CSS Box Model Level 4 `margin-trim: block-start` to the first
/// in-flow child adjoining a block container's block-start edge.
///
/// The margin to trim is the collapsed adjoining margin set at the parent's
/// block-start edge, not just the child's authored `margin-top`. Cancelling
/// that collapsed contribution is observable when CSS 2.2 block-in-inline
/// splitting exposes a self-collapsing block as the first in-flow child.
///
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn trim_adjoining_block_start_margin(
    parent_style: &ComputedStyle,
    child_style: &mut ComputedStyle,
    is_first_flow_child: bool,
    descendant_start_margin: Option<f32>,
) -> bool {
    if !parent_style.margin_trim.block_start || !is_first_flow_child {
        return false;
    }
    let adjoining_start_margin = descendant_start_margin
        .map(|descendant| {
            collapse_margins(layout_pt(child_style.margin.top), layout_pt(descendant)).points()
        })
        .unwrap_or(child_style.margin.top);
    child_style.margin.top -= adjoining_start_margin;
    true
}

/// Returns whether a block's own top and bottom margins may adjoin.
///
/// CSS 2.2 allows a block's own margins to be adjoining when it has no border,
/// padding, line boxes, min-height, or in-flow content separating the edges,
/// and its height is either `auto` or zero.
/// Formatting-context roots, including `flow-root`, and layout/paint-contained
/// boxes cannot be self-collapsing through contained descendants.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(in crate::layout) fn can_collapse_own_block_margins(
    element: &Element,
    style: &ComputedStyle,
    border_widths: css::Edges,
    has_direct_inline_content: bool,
    used_overflow: css::Overflow,
) -> bool {
    style.display.is_flow()
        && style.float == Float::None
        && !style_establishes_multicol_formatting_context(style)
        && !used_property_containment(element, style).establishes_independent_formatting_context()
        && !has_direct_inline_content
        && used_overflow == css::Overflow::Visible
        && style.padding.top == 0.0
        && style.padding.bottom == 0.0
        && border_widths.top == 0.0
        && border_widths.bottom == 0.0
        && height_is_auto_or_zero(style)
        && style.box_values.min_height.is_auto()
}

pub(in crate::layout) fn height_is_auto_or_zero(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
        || style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height == 0.0)
}

/// Returns whether a block's own start and end margins are adjoining.
///
/// CSS 2.2 defines the margins of a box with no border, padding, min-height,
/// fixed height, line box, or in-flow non-self-collapsing children as
/// adjoining. Such a child keeps its parent's block-start collapsed margin set
/// open, which matters for `margin-trim: block-start`.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
pub(in crate::layout) fn is_self_collapsing_block_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    overflow_context: DocumentCanvasResolution,
) -> bool {
    // Generated content participates in the generated pseudo-element's box
    // just like an inline child, even though it is collected from `content`
    // during inline layout rather than stored in this formatting-box slice.
    // It therefore prevents the pseudo-element from being self-collapsing.
    // <https://drafts.csswg.org/css-pseudo-4/#generated-content>
    let has_line_box_content = has_direct_inline_content_box(child_boxes)
        || has_atomic_inline_formatting_box(child_boxes)
        || generated_content_has_non_phantom_inline_content(style)
        || style_has_non_phantom_generated_pseudo_content(style)
        || style_has_in_flow_marker_line(style);
    // Clearance changes the placement of an otherwise empty box without
    // creating an own block-size. Its margins therefore remain adjoining for
    // self-collapsing classification; the block-flow cursor independently
    // retains clearance for following in-flow content.
    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            element,
            style,
            used_border_widths(style),
            has_line_box_content,
            overflow_context.used_overflow(element, style),
        )
        && !(style.display.establishes_block_formatting_context()
            && child_boxes.iter().any(|child| {
                formatting_box_is_in_normal_flow(child)
                    && !formatting_box_can_only_create_phantom_line_boxes(child)
            }))
        && child_boxes
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context))
}

pub(in crate::layout) fn self_collapsing_block_margin_set_for_box(
    style: &ComputedStyle,
    descendant_start_margin: Option<f32>,
) -> f32 {
    let margins = [
        Some(style.margin.top),
        descendant_start_margin,
        Some(style.margin.bottom),
    ];
    collapse_margin_set(margins.into_iter().flatten().map(layout_pt)).points()
}

pub(in crate::layout) fn formatting_box_keeps_self_collapsing_parent(
    box_: &box_tree::FormattingBox<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    if box_tree::is_out_of_flow_box(box_) || box_tree::is_floated_box(box_) {
        return true;
    }
    if formatting_box_can_only_create_phantom_line_boxes(box_) {
        return true;
    }
    // CSS 2.2 block-in-inline splitting materializes this transparent
    // context solely to retain the originating inline box's painting and
    // positioning state. Its block child still participates directly in the
    // enclosing block formatting context, including adjoining margin sets.
    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    if let box_tree::FormattingBox::InlineSplitBlockContext(context) = box_ {
        return context
            .core
            .children
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context));
    }
    if let box_tree::FormattingBox::AnonymousBlock(box_) = box_ {
        return box_
            .children
            .iter()
            .all(|child| formatting_box_keeps_self_collapsing_parent(child, overflow_context));
    }
    let Some((element, _, style, children)) = box_.element_parts() else {
        return false;
    };
    is_normal_block_flow_child(element, style)
        && is_self_collapsing_block_box(element, style, children, overflow_context)
}

pub(in crate::layout) fn collapsible_first_child_start_margin_from_boxes(
    child_boxes: &[box_tree::FormattingBox<'_>],
    parent: &Element,
    parent_style: &ComputedStyle,
    overflow_context: DocumentCanvasResolution,
) -> Option<f32> {
    let mut has_preceding_css_float = false;
    for child_box in child_boxes {
        // An inline split context is transparent to the parent block's
        // margin-collapse chain. Recurse into its generated block segment
        // instead of treating the originating inline wrapper as a separator.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if let box_tree::FormattingBox::InlineSplitBlockContext(context) = child_box {
            if let Some(margin) = collapsible_first_child_start_margin_from_boxes(
                &context.core.children,
                parent,
                parent_style,
                overflow_context,
            ) {
                return Some(margin);
            }
            continue;
        }
        let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
        else {
            if matches!(
                child_box,
                box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Text(_)
            ) && !formatting_box_can_only_create_phantom_line_boxes(child_box)
            {
                return None;
            }
            continue;
        };
        // Out-of-flow descendants do not participate in the parent's
        // adjoining-margin chain. In particular, the document-canvas
        // exception below must not make a floated first child look like the
        // first in-flow child and collapse its margin through the body.
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, child_style)
            || overflow_context.is_document_canvas_flow_element(parent)
            || is_replaced_element(child_element);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        // A cleared first in-flow child after an adjoining float needs the
        // parent-start `clear:none` hypothesis rather than a pre-collapsed
        // used start margin. Other cleared descendants retain the ordinary
        // margin-collapse probe; block layout resolves whether they actually
        // introduce clearance in their containing float context.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if child_style.clear != Clear::None && has_preceding_css_float {
            return None;
        }
        if parent_style.margin_trim.block_start {
            return Some(0.0);
        }
        return Some(collapsible_start_margin_for_box(
            child_element,
            child_style,
            child_children,
            overflow_context,
        ));
    }
    None
}

pub(in crate::layout) fn collapsible_start_margin_for_box(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    overflow_context: DocumentCanvasResolution,
) -> f32 {
    if can_collapse_block_start_margin(
        element,
        style,
        UsedEdges::from_css_edges(used_border_widths(style)),
        has_direct_inline_content_box(child_boxes),
        overflow_context.used_overflow(element, style),
    ) && let Some(descendant_margin) = collapsible_first_child_start_margin_from_boxes(
        child_boxes,
        element,
        style,
        overflow_context,
    ) {
        if is_self_collapsing_block_box(element, style, child_boxes, overflow_context) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        collapse_margins(layout_pt(style.margin.top), layout_pt(descendant_margin)).points()
    } else if is_self_collapsing_block_box(element, style, child_boxes, overflow_context) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        style.margin.top
    }
}

/// Return whether a block container has direct inline content that prevents
/// its own block-start/end edges from adjoining child margins.
///
/// Anonymous blocks are block-level siblings, even though they contain inline
/// content, so they do not make the enclosing block container have direct
/// inline content: <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn has_direct_inline_content_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| {
        matches!(
            child,
            box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Text(_)
        ) && !formatting_box_can_only_create_phantom_line_boxes(child)
    })
}

pub(in crate::layout) fn has_non_inline_formatting_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| {
        if box_tree::is_out_of_flow_box(child) {
            return false;
        }
        match child {
            box_tree::FormattingBox::AnonymousBlock(_) => {
                !formatting_box_can_only_create_phantom_line_boxes(child)
            }
            box_tree::FormattingBox::Table(_) => child.style().display.is_block_level(),
            box_tree::FormattingBox::InlineSplitBlockContext(_)
            | box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::Flex(_) => true,
            // Replaced elements retain their own durable formatting-box
            // variant, but their outer display decides whether they form a
            // block-flow child or one atomic participant in an inline line.
            // <https://drafts.csswg.org/css-display-3/#replaced-elements>
            box_tree::FormattingBox::Replaced(_) => child.style().display.is_block_level(),
            _ => false,
        }
    })
}

pub(in crate::layout) fn formatting_box_can_only_create_phantom_line_boxes(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    match box_ {
        box_tree::FormattingBox::Text(text) => {
            text.text.is_empty()
                || (text.style.white_space.collapses_spaces()
                    && text.text.chars().all(is_css_collapsible_whitespace))
        }
        box_tree::FormattingBox::Inline(box_) => {
            inline_box_has_no_nonzero_inline_axis_component(&box_.core.style)
                && !box_.core.style.content.is_generated()
                && !box_
                    .core
                    .style
                    .before_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                && !box_
                    .core
                    .style
                    .after_style
                    .as_deref()
                    .is_some_and(|style| style.content.is_generated())
                && box_
                    .core
                    .children
                    .iter()
                    .all(formatting_box_can_only_create_phantom_line_boxes)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .all(formatting_box_can_only_create_phantom_line_boxes),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .core
            .children
            .iter()
            .all(formatting_box_can_only_create_phantom_line_boxes),
        box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    }
}

/// Whether generated `content` contributes a non-phantom inline line box.
///
/// An empty (or collapsible-whitespace-only) generated string still creates a
/// pseudo-element box, but it does not make a line box non-adjoining for CSS
/// margin collapse. Other generated content can resolve to visible text or an
/// atomic inline, so it remains a line-box contribution.
/// <https://drafts.csswg.org/css-inline/#invisible-line-boxes>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn generated_content_has_non_phantom_inline_content(
    style: &ComputedStyle,
) -> bool {
    style.content.generated_parts().is_some_and(|parts| {
        parts.iter().any(|part| match part {
            css::GeneratedContentPart::Text(text) => {
                !(text.is_empty()
                    || style.white_space.collapses_spaces()
                        && text.chars().all(is_css_collapsible_whitespace))
            }
            _ => true,
        })
    })
}

fn inline_box_has_no_nonzero_inline_axis_component(style: &ComputedStyle) -> bool {
    let borders = used_border_widths(style);
    [
        inline_start_side(style.writing_mode, style.used_direction()),
        inline_end_side(style.writing_mode, style.used_direction()),
    ]
    .into_iter()
    .all(|side| {
        edge_value(style.margin, side).abs() <= 0.001
            && edge_value(borders, side).abs() <= 0.001
            && edge_value(style.padding, side).abs() <= 0.001
    })
}

fn edge_value(edges: css::Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}

pub(in crate::layout) fn has_atomic_inline_formatting_box(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child| match child {
        box_tree::FormattingBox::AtomicInline(_) => true,
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_atomic_inline_formatting_box(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_atomic_inline_formatting_box(&box_.core.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            has_atomic_inline_formatting_box(&box_.core.children)
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Text(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}

pub(in crate::layout) fn collapsible_first_child_start_margin_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
    overflow_context: DocumentCanvasResolution,
) -> Option<f32> {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    collapsible_first_child_start_margin_dom_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
        overflow_context,
    )
}

pub(in crate::layout) fn collapsible_first_child_start_margin_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> Option<f32> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_preceding_css_float = false;
    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            if let NodeKind::Text(text) = &child.kind
                && !collapse_whitespace(text).is_empty()
            {
                return None;
            }
            continue;
        };
        let signature = ElementSignature::with_sibling_list(
            child_element.tag.clone(),
            child_element.attrs.clone(),
            element_index,
            sibling_tags.clone(),
        );
        element_index += 1;
        let child_style = resolver.style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        // Floats and positioned descendants are out of normal flow, even
        // below the document canvas; they cannot supply an adjoining margin.
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if child_style.float != Float::None {
            has_preceding_css_float = true;
            continue;
        }
        if matches!(child_style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let is_flow_child = is_normal_block_flow_child(child_element, &child_style);
        if !is_flow_child {
            if !inline_text(child_element).is_empty() {
                return None;
            }
            continue;
        }
        // A cleared first in-flow child after an adjoining float needs the
        // parent-start `clear:none` hypothesis rather than a pre-collapsed
        // used start margin. Other cleared descendants retain the ordinary
        // margin-collapse probe; block layout resolves whether they actually
        // introduce clearance in their containing float context.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if child_style.clear != Clear::None && has_preceding_css_float {
            return None;
        }
        if parent_style.margin_trim.block_start {
            return Some(0.0);
        }
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(signature);
        return Some(collapsible_start_margin_dom_with_resolver(
            child_element,
            &child_style,
            stylesheets,
            &child_ancestors,
            resolver,
            overflow_context,
        ));
    }
    None
}

pub(in crate::layout) fn collapsible_start_margin_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> f32 {
    if can_collapse_block_start_margin(
        element,
        style,
        UsedEdges::from_css_edges(used_border_widths(style)),
        has_direct_inline_content_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
        ),
        overflow_context.used_overflow(element, style),
    ) && let Some(descendant_margin) = collapsible_first_child_start_margin_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
        overflow_context,
    ) {
        if is_self_collapsing_block_dom_with_resolver(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
            overflow_context,
        ) {
            return self_collapsing_block_margin_set_for_box(style, Some(descendant_margin));
        }
        collapse_margins(layout_pt(style.margin.top), layout_pt(descendant_margin)).points()
    } else if is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
        overflow_context,
    ) {
        self_collapsing_block_margin_set_for_box(style, None)
    } else {
        style.margin.top
    }
}

/// Returns whether a DOM-backed block's own start and end margins are adjoining.
///
/// This is the DOM traversal equivalent of `is_self_collapsing_block_box` for
/// code paths that have not yet received normalized formatting boxes. It keeps
/// CSS 2.2 margin-collapse and CSS Box 4 `margin-trim` behavior consistent
/// across both layout entrypoints.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
pub(in crate::layout) fn is_self_collapsing_block_dom_with_font_metrics(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    is_self_collapsing_block_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        &mut resolver,
        overflow_context,
    )
}

pub(in crate::layout) fn is_self_collapsing_block_dom_with_resolver(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let has_direct_inline_content = has_direct_inline_content_dom_with_resolver(
        element,
        style,
        stylesheets,
        ancestors,
        resolver,
    ) || style_has_non_phantom_generated_pseudo_content(style)
        || style_has_in_flow_marker_line(style);
    is_collapsible_block_child(element, style)
        && can_collapse_own_block_margins(
            element,
            style,
            used_border_widths(style),
            has_direct_inline_content,
            overflow_context.used_overflow(element, style),
        )
        && dom_children_keep_self_collapsing_parent(
            element,
            style,
            stylesheets,
            ancestors,
            resolver,
            overflow_context,
        )
}

/// Generated `::before` and `::after` boxes participate in their originating
/// block's principal inline flow.  They are stored separately from DOM and
/// formatting-box children, so self-collapsing classification must include
/// them explicitly.
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
fn style_has_non_phantom_generated_pseudo_content(style: &ComputedStyle) -> bool {
    style
        .before_style
        .as_deref()
        .is_some_and(generated_content_has_non_phantom_inline_content)
        || style
            .after_style
            .as_deref()
            .is_some_and(generated_content_has_non_phantom_inline_content)
}

/// Whether a computed list item necessarily supplies an inside marker line.
///
/// Margin-collapse classification happens before the inline formatter creates
/// the actual marker.  This mirrors its empty-marker test so a marker-only
/// list item, and every ancestor whose size depends on it, cannot be treated
/// as self-collapsing.  Counter values are allowed to vary, but every
/// non-`none` counter style has the decimal fallback representation required
/// by CSS Counter Styles.
///
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
fn style_has_in_flow_marker_line(style: &ComputedStyle) -> bool {
    if !style.display.is_list_item() {
        return false;
    }
    let inside = if style.display.is_inline_level() && !style.display.is_atomic_inline() {
        true
    } else {
        style.list_style_position == ListStylePosition::Inside
    };
    if !inside {
        return false;
    }

    let marker_style = style.marker_style.as_deref().unwrap_or(style);
    match &marker_style.marker_content {
        MarkerContent::None => false,
        MarkerContent::Auto => {
            style.list_style_image.is_image()
                || list_style_type_has_nonempty_representation(&style.list_style_type)
        }
        MarkerContent::Parts(parts) => parts
            .iter()
            .any(|part| marker_content_part_has_nonempty_representation(part, marker_style)),
    }
}

fn list_style_type_has_nonempty_representation(style: &ListStyleType) -> bool {
    match style {
        ListStyleType::None => false,
        ListStyleType::String(text) => {
            !crate::text::trim_css_collapsible_whitespace(text).is_empty()
        }
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed
        | ListStyleType::Decimal
        | ListStyleType::Anonymous(_)
        | ListStyleType::Named(_) => true,
    }
}

fn marker_content_part_has_nonempty_representation(
    part: &MarkerContentPart,
    marker_style: &ComputedStyle,
) -> bool {
    match part {
        MarkerContentPart::Text(text) => {
            !crate::text::trim_css_collapsible_whitespace(text).is_empty()
        }
        MarkerContentPart::Counter { style, .. } | MarkerContentPart::Counters { style, .. } => {
            style
                .as_ref()
                .is_none_or(list_style_type_has_nonempty_representation)
        }
        MarkerContentPart::Quote(GeneratedQuote::NoOpen | GeneratedQuote::NoClose) => false,
        MarkerContentPart::Quote(GeneratedQuote::Open | GeneratedQuote::Close) => {
            match &marker_style.quotes {
                Quotes::None => false,
                Quotes::Auto(_) => true,
                Quotes::Pairs(pairs) => pairs.iter().any(|(open, close)| {
                    !crate::text::trim_css_collapsible_whitespace(open).is_empty()
                        || !crate::text::trim_css_collapsible_whitespace(close).is_empty()
                }),
            }
        }
    }
}

pub(in crate::layout) fn dom_children_keep_self_collapsing_parent(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
    overflow_context: DocumentCanvasResolution,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().all(|child| match &child.kind {
        NodeKind::Text(text) => dom_text_can_only_create_phantom_line_box(text, parent_style),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = resolver.style_for_element(
                child,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                ancestors,
            );
            if matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.float != Float::None
            {
                return true;
            }
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            if is_normal_block_flow_child(child, &child_style) {
                is_self_collapsing_block_dom_with_resolver(
                    child,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                    overflow_context,
                )
            } else {
                dom_inline_element_can_only_create_phantom_line_boxes(
                    child,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    resolver,
                )
            }
        }
    })
}

/// Whether an inline DOM subtree can only create an ignorable line box.
///
/// The box-tree margin-collapse path already recognizes this state. Mirror it
/// while block children are still represented by DOM nodes so whitespace and
/// empty inline wrappers do not cause the two paths to disagree.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
fn dom_inline_element_can_only_create_phantom_line_boxes(
    element: &Element,
    style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    // Keep the DOM fallback in lockstep with
    // `formatting_box_can_only_create_phantom_line_boxes`: an atomic inline
    // (including a replaced element) is an actual participant in its
    // enclosing line box, even when it owns no text descendants.  Treating
    // it as phantom lets an ancestor self-collapse after the inline formatter
    // has painted and advanced that line, so a following block can overlap
    // the atom.
    //
    // <https://www.w3.org/TR/css-display-3/#atomic-inline>
    // <https://www.w3.org/TR/css-inline-3/#line-box>
    if style.display.is_atomic_inline() || is_replaced_element(element) {
        return false;
    }
    style.display.is_inline_level()
        && style.display.is_flow()
        && inline_box_has_no_nonzero_inline_axis_component(style)
        && !style.content.is_generated()
        && !style
            .before_style
            .as_deref()
            .is_some_and(|style| style.content.is_generated())
        && !style
            .after_style
            .as_deref()
            .is_some_and(|style| style.content.is_generated())
        && {
            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            element.children.iter().all(|child| match &child.kind {
                NodeKind::Text(text) => dom_text_can_only_create_phantom_line_box(text, style),
                NodeKind::Element(child) => {
                    let signature = ElementSignature::with_sibling_list(
                        child.tag.clone(),
                        child.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let child_style = resolver.style_for_element(
                        child,
                        signature.clone(),
                        stylesheets,
                        Some(style),
                        ancestors,
                    );
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    dom_inline_element_can_only_create_phantom_line_boxes(
                        child,
                        &child_style,
                        stylesheets,
                        &child_ancestors,
                        resolver,
                    )
                }
            })
        }
}

fn dom_text_can_only_create_phantom_line_box(text: &str, style: &ComputedStyle) -> bool {
    style.white_space.collapses_spaces() && collapse_whitespace(text).is_empty()
}

/// Return whether direct inline content precedes the first normal-flow block
/// child in DOM order.
///
/// Only inline content before that child prevents the parent's block-start
/// margin from adjoining the child's margin. Inline content after a first
/// block belongs to a later anonymous block box and cannot retroactively
/// separate the parent's block-start edge:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) if !collapse_whitespace(text).is_empty() => return true,
            NodeKind::Text(_) => {}
            NodeKind::Element(child) => {
                let signature = ElementSignature::with_sibling_list(
                    child.tag.clone(),
                    child.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = resolver.style_for_element(
                    child,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if is_normal_block_flow_child(child, &child_style) {
                    return false;
                }
                let mut child_ancestors = ancestors.to_vec();
                child_ancestors.push(signature);
                if is_line_break_element(child)
                    || (child_style.display.is_inline_level()
                        && !dom_inline_element_can_only_create_phantom_line_boxes(
                            child,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                            &mut resolver,
                        ))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Return whether a raw DOM block container has any non-phantom direct inline
/// contribution.
///
/// This is used for self-collapsing and parent/child-collapse checks, where an
/// inline run after a block child still gives the parent non-zero content.
/// A sized atomic inline box contributes a line box even when it has no text.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn has_direct_inline_content_dom_with_resolver(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> bool {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    element.children.iter().any(|child| match &child.kind {
        NodeKind::Text(text) => !collapse_whitespace(text).is_empty(),
        NodeKind::Element(child) => {
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = resolver.style_for_element(
                child,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                ancestors,
            );
            if !child_style.display.is_inline_level() {
                return false;
            }
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature);
            !dom_inline_element_can_only_create_phantom_line_boxes(
                child,
                &child_style,
                stylesheets,
                &child_ancestors,
                resolver,
            )
        }
    })
}

/// Whether a raw DOM fallback contains only direct in-flow inline content.
///
/// The document canvas deliberately has no own text run, so this identifies
/// the narrow case where its direct DOM children must be collected as one
/// inline formatting context.  Block, float, replaced, and out-of-flow
/// children retain their dedicated layout paths.
///
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
pub(in crate::layout) fn has_only_direct_in_flow_inline_dom_content_with_font_metrics(
    element: &Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> bool {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut has_content = false;

    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                has_content |= !collapse_whitespace(text).is_empty();
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = resolver.style_for_element(
                    child_element,
                    signature,
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                if child_style.display.is_none() {
                    continue;
                }
                if !child_style.display.is_inline_level()
                    || child_style.float != Float::None
                    || matches!(child_style.position, Position::Absolute | Position::Fixed)
                    || is_replaced_element(child_element)
                {
                    return false;
                }
                has_content |= !inline_text(child_element).is_empty();
            }
        }
    }
    has_content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_snapshots_are_shared_after_dom_preparation() {
        let root = dom::parse(
            "<html><body><section id=\"hit\"><span lang=\"fr\">texte</span></section></body></html>",
        );
        prime_selector_snapshots(&root, None, None);
        let section = first_element_by_tag(&root, "section").expect("expected section");

        let first = element_selector_signature(section);
        let replay = element_selector_signature(section);

        assert!(first.shares_snapshot_with(&replay));
        assert_eq!(first.children.len(), 1);
        assert_eq!(first.children[0].attrs.get("lang"), Some(&"fr".to_string()));
    }

    #[test]
    fn selector_preparation_marks_only_document_local_links_visited() {
        let document_url = url::Url::parse("https://example.test/document.html#source")
            .expect("valid document URL");

        let self_link = dom::parse("<a href=\"#section\"></a>");
        prime_selector_snapshots(&self_link, Some(&document_url), Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&self_link, "a").unwrap()).link_state,
            css::LinkState::Visited
        );

        let external_link = dom::parse("<a href=\"other.html\"></a>");
        prime_selector_snapshots(&external_link, Some(&document_url), Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&external_link, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );

        let base_changed_link = dom::parse("<a href=\"\"></a>");
        let base_url = url::Url::parse("https://assets.example.test/").expect("valid base URL");
        prime_selector_snapshots(&base_changed_link, Some(&document_url), Some(&base_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&base_changed_link, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );

        let string_document = dom::parse("<a href=\"\"></a>");
        prime_selector_snapshots(&string_document, None, Some(&document_url));
        assert_eq!(
            element_selector_signature(first_element_by_tag(&string_document, "a").unwrap())
                .link_state,
            css::LinkState::Unvisited
        );
    }

    fn test_parent_style() -> ComputedStyle {
        ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: CssColor::BLACK,
            ..ComputedStyle::initial()
        }
    }

    fn first_element_by_tag<'a>(node: &'a Node, tag: &str) -> Option<&'a Element> {
        match &node.kind {
            NodeKind::Text(_) => None,
            NodeKind::Element(element) => {
                if element.tag == tag {
                    return Some(element);
                }
                element
                    .children
                    .iter()
                    .find_map(|child| first_element_by_tag(child, tag))
            }
        }
    }

    #[tokio::test]
    async fn structural_style_keeps_principal_cascade_and_omits_generated_pseudos() {
        let root = dom::parse("<span></span>");
        let span = first_element_by_tag(&root, "span").expect("expected span element");
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            "span { display: block; font-size: 3ch } \
             span::before { content: var(--missing, 'generated') }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = ComputedStyle {
            font_size: 20.0,
            line_height: 20.0,
            ..ComputedStyle::initial()
        };
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;
        let signature = element_signature(span);
        let mut resolver = DomStyleResolver::with_font_system(&mut font_system);

        let full = resolver.style_for_element(
            span,
            signature.clone(),
            &stylesheets,
            Some(&parent_style),
            &[],
        );
        let structural = resolver.structural_style_for_element(
            span,
            signature,
            &stylesheets,
            Some(&parent_style),
            &[],
        );

        assert_eq!(structural.display, full.display);
        assert_eq!(structural.font_size, full.font_size);
        assert!(full.before_style.is_some());
        assert!(structural.before_style.is_none());
        assert!(structural.after_style.is_none());
        assert!(structural.marker_style.is_none());
    }

    #[tokio::test]
    async fn structural_probes_ignore_generated_pseudo_variable_rules() {
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            ".flow { display: block } \
             .run-in { display: run-in } \
             .row { display: table-row } \
             .inline { display: inline } \
             .ruby { display: ruby } \
             .probe::before { content: var(--missing, 'generated') }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = test_parent_style();
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let direct_flow = dom::parse("<div><span class=\"probe flow\"></span></div>");
        assert!(has_direct_flow_child_with_font_metrics(
            first_element_by_tag(&direct_flow, "div").expect("expected direct-flow parent"),
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let run_in = dom::parse("<div><span class=\"probe run-in\"></span></div>");
        assert!(has_direct_run_in_child_with_font_metrics(
            first_element_by_tag(&run_in, "div").expect("expected run-in parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let table = dom::parse("<div><span class=\"probe row\"></span></div>");
        assert!(has_unwrapped_table_internal_descendant_with_font_metrics(
            first_element_by_tag(&table, "div").expect("expected table parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let block_in_inline = dom::parse(
            "<div><span class=\"probe inline\"><span class=\"probe flow\"></span></span></div>",
        );
        assert!(has_block_in_inline_split_boundary_with_font_metrics(
            first_element_by_tag(&block_in_inline, "div").expect("expected block-in-inline parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let ruby = dom::parse("<div><span class=\"probe ruby\"></span></div>");
        assert!(has_ruby_formatting_descendant_with_font_metrics(
            first_element_by_tag(&ruby, "div").expect("expected ruby parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
            &mut HashMap::new(),
        ));

        let later_block = dom::parse(
            "<div><span class=\"probe inline\"></span><span class=\"probe flow\"></span></div>",
        );
        let later_parent =
            first_element_by_tag(&later_block, "div").expect("expected later-block parent");
        let siblings = element_sibling_signature_list(later_parent);
        assert!(has_later_normal_block_flow_child_with_font_metrics(
            later_parent,
            1,
            &siblings,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
    }

    #[tokio::test]
    async fn direct_float_uses_ordinary_child_traversal_without_reclassifying_normal_flow() {
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            ".float { display: inline-block; float: left } \
             .inline { display: inline-block }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = test_parent_style();
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let floated = dom::parse("<div><span class=\"float\">float</span></div>");
        let floated_parent = first_element_by_tag(&floated, "div").expect("expected float parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
        assert!(!has_direct_flow_child_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));
        assert!(has_direct_float_only_source_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let mixed = dom::parse("<div>prefix<span class=\"float\">float</span></div>");
        let mixed_parent = first_element_by_tag(&mixed, "div").expect("expected mixed parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            mixed_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
        assert!(!has_direct_float_only_source_with_font_metrics(
            mixed_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let inline = dom::parse("<div><span class=\"inline\">inline</span></div>");
        let inline_parent = first_element_by_tag(&inline, "div").expect("expected inline parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            inline_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
    }

    #[test]
    fn margin_collapse_keeps_signed_layout_lengths_typed() {
        let mixed: LayoutLength = collapse_margins(layout_pt(12.0), layout_pt(-4.0));
        let negative: LayoutLength = collapse_margins(layout_pt(-3.0), layout_pt(-9.0));
        let set: LayoutLength =
            collapse_margin_set([layout_pt(8.0), layout_pt(-3.0), layout_pt(5.0)]);

        assert_eq!(mixed, layout_pt(8.0));
        assert_eq!(negative, layout_pt(-9.0));
        assert_eq!(set, layout_pt(5.0));
        assert_eq!(page_start_margin(layout_pt(7.0), true), layout_pt(0.0));
    }

    #[test]
    fn independent_formatting_contexts_keep_outer_margins_adjoining_siblings() {
        let root = dom::parse("<html><body><div></div><img></body></html>");
        let block = first_element_by_tag(&root, "div").expect("expected block element");
        let image = first_element_by_tag(&root, "img").expect("expected image element");

        for (name, display) in [
            ("grid", Display::GRID),
            ("flex", Display::FLEX),
            (
                "flow-root",
                Display::BLOCK.with_inner(DisplayInner::FlowRoot),
            ),
        ] {
            let mut style = ComputedStyle::initial();
            style.display = display;
            assert!(
                outer_margins_adjoin_block_siblings(block, &style),
                "a block-level {name} container's outer margins should adjoin a normal-flow sibling"
            );
        }

        let mut absolute = ComputedStyle::initial();
        absolute.position = Position::Absolute;
        assert!(!outer_margins_adjoin_block_siblings(block, &absolute));

        let mut fixed = ComputedStyle::initial();
        fixed.position = Position::Fixed;
        assert!(!outer_margins_adjoin_block_siblings(block, &fixed));

        let mut floated = ComputedStyle::initial();
        floated.float = Float::Left;
        assert!(!outer_margins_adjoin_block_siblings(block, &floated));

        let mut atomic_inline = ComputedStyle::initial();
        atomic_inline.display = Display::INLINE_BLOCK;
        assert!(!outer_margins_adjoin_block_siblings(block, &atomic_inline));

        assert!(!outer_margins_adjoin_block_siblings(
            image,
            &ComputedStyle::initial()
        ));
    }

    #[test]
    fn flow_root_does_not_self_collapse_margins() {
        let root = dom::parse("<div></div>");
        let element = first_element_by_tag(&root, "div").expect("expected block element");
        let mut style = ComputedStyle::initial();

        assert!(can_collapse_own_block_margins(
            element,
            &style,
            css::Edges::ZERO,
            false,
            css::Overflow::Visible,
        ));

        style.display = Display::BLOCK.with_inner(DisplayInner::FlowRoot);
        assert!(!can_collapse_own_block_margins(
            element,
            &style,
            css::Edges::ZERO,
            false,
            css::Overflow::Visible,
        ));
    }

    #[tokio::test]
    async fn sized_atomic_inline_blocks_start_margin_collapse_but_empty_inline_does_not() {
        let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let atomic_root = dom::parse(
            "<html><body><div><span style=\"display:inline-block; width:100px; height:100px\"></span><div></div></div></body></html>",
        );
        let atomic_parent = first_element_by_tag(&atomic_root, "div")
            .expect("expected the atomic inline's parent block");
        assert!(
            has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                atomic_parent,
                &test_parent_style(),
                &stylesheets,
                &[],
                &mut font_system,
            )
        );

        let phantom_root =
            dom::parse("<html><body><div><span></span><div></div></div></body></html>");
        let phantom_parent = first_element_by_tag(&phantom_root, "div")
            .expect("expected the phantom inline's parent block");
        assert!(
            !has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                phantom_parent,
                &test_parent_style(),
                &stylesheets,
                &[],
                &mut font_system,
            )
        );
    }

    #[test]
    fn atomic_inline_page_values_do_not_escape_its_atomic_formatting_context() {
        let root = dom::parse(
            "<html><body>\
             <div style=\"page:c; display:inline-block\">\
               <div style=\"page:a\">A</div>\
               <div style=\"page:b\">B</div>\
             </div>\
             <div style=\"page:c\">C</div>\
             </body></html>",
        );
        let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
        let page = box_tree::freeze_page_box(box_tree::build_page_box(
            &root,
            &stylesheets,
            &test_parent_style(),
        ));
        let body = &page.children[0].children()[0];
        let anonymous_inline_run = &body.children()[0];

        assert_eq!(
            formatting_box_page_values(anonymous_inline_run),
            (None, None)
        );
        assert_eq!(
            formatting_box_page_values(&body.children()[1]),
            (Some("c".to_string()), Some("c".to_string()))
        );
    }

    #[test]
    fn deeply_nested_inline_page_values_do_not_repeat_single_child_traversal() {
        // The formatting tree itself is recursive, while the regression
        // verifies its named-page summary no longer branches exponentially.
        std::thread::Builder::new()
            .name("deep-page-value-regression".to_string())
            .spawn(|| {
                // WPT: css/css-zoom/crashtests/zoom-deeply-nested.html. The
                // CSS declaration is incidental: the regression is a
                // sole-child chain in named-page summary traversal.
                let spans = "<span>".repeat(40);
                let closing_spans = "</span>".repeat(40);
                let root = dom::parse(&format!(
                    "<html><style>span {{ zoom: .1%; }}</style><body>{spans}text{closing_spans}</body></html>"
                ));
                let stylesheets =
                    Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
                let page = box_tree::freeze_page_box(box_tree::build_page_box(
                    &root,
                    &stylesheets,
                    &test_parent_style(),
                ));
                let body = &page.children[0].children()[0];

                assert_eq!(
                    formatting_box_page_value_sources(body),
                    PageBoundaryValues {
                        start: PageBoundaryValue::Inherited,
                        end: PageBoundaryValue::Inherited,
                    }
                );
            })
            .expect("deep page-value regression thread should start")
            .join()
            .expect("deep page-value regression thread should complete");
    }

    #[tokio::test]
    async fn definition_list_dom_grouping_uses_measured_parent_ch_for_child_font_size() {
        let root =
            dom::parse("<html><body><dl><dt style=\"font-size: 2ch\">Term</dt></dl></body></html>");
        let dl = first_element_by_tag(&root, "dl").expect("expected dl element");
        let stylesheet = css::parse_stylesheet(
            &css::Css::from_string(
                r#"@font-face {
                    font-family: MetricProbe;
                    src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
                }"#,
            )
            .with_base_path(".")
            .expect("current directory should be a valid file URL"),
        );
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let mut parent_style = ComputedStyle {
            font_family: css::FontFamily::Names(vec!["MetricProbe".to_string()]),
            font_size: 40.0,
            line_height: 40.0,
            ..ComputedStyle::initial()
        };
        parent_style.line_height_value = css::ComputedLineHeight::from_points(40.0);
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;
        let parent_ch_advance = font_system.ch_advance(&parent_style);
        assert!(
            (parent_ch_advance.points() - parent_style.font_size * 0.5).abs() > 0.01,
            "fixture must differ from the generic 0.5em ch fallback"
        );

        let groups = definition_list_column_groups_with_font_metrics(
            dl,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        );

        assert_eq!(groups.len(), 1);
        assert!((groups[0][0].style.font_size - parent_ch_advance.points() * 2.0).abs() < 0.01);
    }
}
