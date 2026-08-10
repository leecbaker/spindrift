use super::*;

/// Class-A break values exposed by the first and last in-flow flex items.
///
/// Flexbox propagates these item-side values to the flex container's outer
/// fragmentation boundaries.  The record keeps the order-modified flex-item
/// sequence separate from the container style, so a parent formatting context
/// does not need to reconstruct itemization to honor an avoided boundary.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct FlexContainerFragmentBoundaryBreaks {
    pub(in crate::layout) before: PageBreak,
    pub(in crate::layout) after: PageBreak,
}

impl FlexContainerFragmentBoundaryBreaks {
    fn from_itemized_children(children: &[StyledChild<'_>]) -> Self {
        Self {
            before: children
                .first()
                .map(|child| child.style.break_before)
                .unwrap_or(PageBreak::Auto),
            after: children
                .last()
                .map(|child| child.style.break_after)
                .unwrap_or(PageBreak::Auto),
        }
    }

    /// Combines propagated item values with the container's authored
    /// boundaries for the active fragmentation context.
    pub(in crate::layout) fn combined_with_container(
        self,
        fragmentainer_kind: FragmentainerKind,
        container_style: &ComputedStyle,
    ) -> Self {
        Self {
            before: fragmentainer_kind.combine_break(container_style.break_before, self.before),
            after: fragmentainer_kind.combine_break(container_style.break_after, self.after),
        }
    }
}

/// Returns the outer class-A break values of a flex container.
///
/// The first/last item are selected after Flexbox itemization and `order`
/// sorting. Out-of-flow positioned descendants do not participate.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
pub(in crate::layout) fn flex_container_fragment_boundary_breaks(
    container_element: &Element,
    container_signature: &ElementSignature,
    container_style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    fragmentainer_kind: FragmentainerKind,
) -> FlexContainerFragmentBoundaryBreaks {
    let children = flex_children_from_boxes(
        container_element,
        container_signature,
        container_style,
        child_boxes,
    );
    FlexContainerFragmentBoundaryBreaks::from_itemized_children(&children)
        .combined_with_container(fragmentainer_kind, container_style)
}

pub(super) fn flex_children_from_boxes<'a>(
    container_element: &'a Element,
    container_signature: &ElementSignature,
    container_style: &ComputedStyle,
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Vec<StyledChild<'a>> {
    flex_child_lists_from_boxes(
        container_element,
        container_signature,
        container_style,
        child_boxes,
    )
    .0
}

/// Splits normalized child boxes into flex items and out-of-flow positioned boxes.
///
/// CSS Positioned Layout makes absolutely positioned boxes out-of-flow, and
/// CSS Flexbox says they do not participate in flex item layout:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(super) fn flex_child_lists_from_boxes<'a>(
    container_element: &'a Element,
    container_signature: &ElementSignature,
    container_style: &ComputedStyle,
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<StyledChild<'a>>, Vec<StyledChild<'a>>) {
    // `content` replaces an element's ordinary children. Generated pseudos
    // keep that content on their own computed style rather than materializing
    // it as a frozen child box, so a flex formatting context must manufacture
    // its anonymous flex item here. Replaying the item through the normal
    // formatting-box entry point then evaluates the generated parts in the
    // pseudo counter scope already established by the outer box.
    //
    // The anonymous item inherits the generated container's text properties,
    // but not its box geometry: those margins, dimensions, and decoration
    // belong to the flex container itself.
    // <https://www.w3.org/TR/css-content-3/#content-property>
    // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
    if container_style.content.is_generated() {
        let mut generated_item_style = container_style.clone();
        generated_item_style.display = Display::BLOCK;
        suppress_replayed_item_margins(&mut generated_item_style);
        generated_item_style = independent_formatting_context_item_style(generated_item_style);
        return (
            vec![StyledChild {
                kind: FormattingContextChildKind::Element {
                    element: container_element,
                    signature: Box::new(container_signature.clone()),
                    generated_pseudo: None,
                    children: Some(std::borrow::Cow::Borrowed(&[])),
                    table_fragment: None,
                },
                style: generated_item_style,
            }],
            Vec::new(),
        );
    }
    let (mut in_flow, positioned) = itemize_blockified_children(
        child_boxes,
        ItemizationOptions {
            anonymous_item_tag: "__quire_anonymous_flex_item",
            strip_blockified_inline_text_paint: true,
            establish_independent_formatting_context: true,
        },
    );
    if !container_style.flex_wrap.wraps() {
        apply_single_line_flex_margin_trim(container_style, &mut in_flow);
    }
    (in_flow, positioned)
}

/// Apply the single-line subset of a flex container's `margin-trim`.
///
/// Multi-line containers refine this plan once flex-line topology is known.
/// This eager application is nevertheless the exact final plan for the
/// overwhelmingly common single-line case, and ensures intrinsic estimation
/// and the Taffy adapter receive the same used margins.
/// <https://drafts.csswg.org/css-box-4/#margin-trim-flex>.
fn apply_single_line_flex_margin_trim(
    container_style: &ComputedStyle,
    children: &mut [StyledChild<'_>],
) {
    if children.is_empty() {
        return;
    }
    let axes = WritingModeAxes::new(
        container_style.writing_mode,
        container_style.used_direction(),
    );
    let (main_start, main_end) = flex_main_logical_edges(container_style);
    let main_start = axes.physical_side(main_start);
    let main_end = axes.physical_side(main_end);
    let mut plan = MarginTrimPlan::for_item_count(children.len());

    for (trimmed, side) in [
        (
            container_style.margin_trim.block_start,
            LogicalSide::BlockStart,
        ),
        (container_style.margin_trim.block_end, LogicalSide::BlockEnd),
        (
            container_style.margin_trim.inline_start,
            LogicalSide::InlineStart,
        ),
        (
            container_style.margin_trim.inline_end,
            LogicalSide::InlineEnd,
        ),
    ] {
        if !trimmed {
            continue;
        }
        let physical_side = axes.physical_side(side);
        if physical_side.axis() != main_start.axis() {
            // The container edge is parallel to the main axis, so every item
            // on the only flex line adjoins it.
            for index in 0..children.len() {
                plan.trim(index, physical_side);
            }
        } else if physical_side == main_start {
            plan.trim(0, physical_side);
        } else if physical_side == main_end {
            plan.trim(children.len() - 1, physical_side);
        }
    }
    for (index, child) in children.iter_mut().enumerate() {
        plan.apply_to_style(index, &mut child.style);
    }
}

/// Return the logical edges occupied by the first and last flex items.
///
/// CSS Flexbox defines `row` from the container inline axis and `column` from
/// its block axis; the reverse variants exchange those edges:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
fn flex_main_logical_edges(style: &ComputedStyle) -> (LogicalSide, LogicalSide) {
    match style.flex_direction {
        FlexDirection::Row => (LogicalSide::InlineStart, LogicalSide::InlineEnd),
        FlexDirection::RowReverse => (LogicalSide::InlineEnd, LogicalSide::InlineStart),
        FlexDirection::Column => (LogicalSide::BlockStart, LogicalSide::BlockEnd),
        FlexDirection::ColumnReverse => (LogicalSide::BlockEnd, LogicalSide::BlockStart),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_boundary_breaks_follow_order_modified_first_and_last_items() {
        let mut first = ComputedStyle::initial();
        first.break_before = PageBreak::AvoidColumn;
        let mut last = ComputedStyle::initial();
        last.break_after = PageBreak::AvoidColumn;
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
                style: first,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
                style: last,
            },
        ];

        assert_eq!(
            FlexContainerFragmentBoundaryBreaks::from_itemized_children(&children),
            FlexContainerFragmentBoundaryBreaks {
                before: PageBreak::AvoidColumn,
                after: PageBreak::AvoidColumn,
            }
        );
    }

    #[test]
    fn row_margin_trim_uses_the_container_inline_edge_for_orthogonal_items() {
        let mut container = ComputedStyle::initial();
        container.margin_trim.inline_start = true;
        container.margin_trim.inline_end = true;
        let mut item = ComputedStyle::initial();
        item.margin.left = 10.0;
        item.margin.right = 20.0;
        item.writing_mode = WritingMode::VerticalRl;
        let mut children = vec![StyledChild {
            kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
            style: item,
        }];

        apply_single_line_flex_margin_trim(&container, &mut children);

        assert_eq!(children[0].style.margin.left, 0.0);
        assert_eq!(children[0].style.margin.right, 0.0);
    }
}
