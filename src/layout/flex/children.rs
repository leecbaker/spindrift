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
    trim_flex_item_margins_at_container_inline_edges(container_style, &mut in_flow);
    (in_flow, positioned)
}

/// Apply a flex container's `margin-trim: inline` to its edge flex items.
///
/// `margin-trim` is defined against the containing block's logical edges, not
/// against an item's own writing mode.  Applying the zeroed physical margin to
/// the item style before both intrinsic estimation and the Taffy adapter keeps
/// every flex sizing and placement phase in agreement:
/// <https://drafts.csswg.org/css-box-4/#margin-trim> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
fn trim_flex_item_margins_at_container_inline_edges(
    container_style: &ComputedStyle,
    children: &mut [StyledChild<'_>],
) {
    if children.is_empty() {
        return;
    }
    let Some((main_start, main_end)) = flex_main_logical_edges(container_style) else {
        return;
    };
    let axes = WritingModeAxes::new(
        container_style.writing_mode,
        container_style.used_direction(),
    );
    let inline_start = axes.physical_side(LogicalSide::InlineStart);
    let inline_end = axes.physical_side(LogicalSide::InlineEnd);

    if container_style.margin_trim.inline_start {
        if axes.physical_side(main_start) == inline_start {
            trim_physical_item_margin(&mut children[0].style, inline_start);
        }
        if axes.physical_side(main_end) == inline_start {
            trim_physical_item_margin(&mut children[children.len() - 1].style, inline_start);
        }
    }
    if container_style.margin_trim.inline_end {
        if axes.physical_side(main_start) == inline_end {
            trim_physical_item_margin(&mut children[0].style, inline_end);
        }
        if axes.physical_side(main_end) == inline_end {
            trim_physical_item_margin(&mut children[children.len() - 1].style, inline_end);
        }
    }
}

/// Return the logical edges occupied by the first and last flex items.
///
/// CSS Flexbox defines `row` from the container inline axis and `column` from
/// its block axis; the reverse variants exchange those edges:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
fn flex_main_logical_edges(style: &ComputedStyle) -> Option<(LogicalSide, LogicalSide)> {
    match style.flex_direction {
        FlexDirection::Row => Some((LogicalSide::InlineStart, LogicalSide::InlineEnd)),
        FlexDirection::RowReverse => Some((LogicalSide::InlineEnd, LogicalSide::InlineStart)),
        FlexDirection::Column => Some((LogicalSide::BlockStart, LogicalSide::BlockEnd)),
        FlexDirection::ColumnReverse => Some((LogicalSide::BlockEnd, LogicalSide::BlockStart)),
    }
}

/// Set a physical margin to its specified zero used value.
///
/// Both the eagerly resolved edge and the computed source value must change:
/// Flexbox uses the former during its post-Taffy corrections and the latter at
/// the Taffy sizing boundary.  CSS Box defines a trimmed margin as zero:
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
fn trim_physical_item_margin(style: &mut ComputedStyle, side: PhysicalSide) {
    let zero = css::ComputedLengthPercentageOrAuto::ZERO;
    match side {
        PhysicalSide::Top => {
            style.margin.top = 0.0;
            style.box_values.margin.top = zero;
        }
        PhysicalSide::Right => {
            style.margin.right = 0.0;
            style.box_values.margin.right = zero;
        }
        PhysicalSide::Bottom => {
            style.margin.bottom = 0.0;
            style.box_values.margin.bottom = zero;
        }
        PhysicalSide::Left => {
            style.margin.left = 0.0;
            style.box_values.margin.left = zero;
        }
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

        trim_flex_item_margins_at_container_inline_edges(&container, &mut children);

        assert_eq!(children[0].style.margin.left, 0.0);
        assert_eq!(children[0].style.margin.right, 0.0);
    }
}
