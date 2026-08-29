use super::*;
use crate::layout::block::{
    DefinitePhysicalContentHeight, child_available_space_for_formatting_context,
};

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum PageStartMarginPolicy {
    Preserve,
    Suppress,
}

/// The fragmentation rules for an item replayed by a layout algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum ReplayedItemFragmentationPolicy {
    Flex,
    Grid,
}

/// Identifies the code path responsible for a replayed formatting-context
/// root's principal-box decoration.
///
/// A parent layout algorithm can paint an item's decoration at its resolved
/// placement before replaying its contents. This belongs to that replay root
/// only; it must not be ambient builder state that a descendant can consume.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) enum PrincipalBoxPaintMode {
    #[default]
    RootPaints,
    ParentPaints,
}

impl PrincipalBoxPaintMode {
    pub(in crate::layout) const fn root_paints(self) -> bool {
        matches!(self, Self::RootPaints)
    }
}

/// Build the common base style for a flex or grid item replayed in a later
/// fragmentainer.
///
/// CSS Flexbox resets `break-before` and `break-after` on flex items during
/// pagination, while CSS Grid also disables break avoidance for a fragmented
/// grid item. See [Flexbox § 10](https://drafts.csswg.org/css-flexbox-1/#pagination)
/// and [Grid § 12](https://drafts.csswg.org/css-grid-1/#pagination).
pub(in crate::layout) fn replayed_item_fragmentation_base_style(
    source: &ComputedStyle,
    fragmentation_policy: ReplayedItemFragmentationPolicy,
) -> ComputedStyle {
    let mut style = independent_formatting_context_item_style(source.clone());
    suppress_replayed_item_margins(&mut style);
    style.page = css::PageAssignment::Unspecified;
    style.break_before = PageBreak::Auto;
    style.break_after = PageBreak::Auto;

    if matches!(fragmentation_policy, ReplayedItemFragmentationPolicy::Grid) {
        style.break_inside = css::BreakInsideAvoidance::Auto;
    }

    style
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) enum ReplayFloatScope {
    /// The replay participates in its parent's float formatting context.
    ///
    /// This is only appropriate when the replay is another view of the
    /// parent's own in-flow content, rather than an independently formatted
    /// child.
    #[default]
    InheritContainingBlock,
    /// The replay establishes an independent formatting context whose
    /// descendants must not observe or mutate ancestor float exclusions.
    ///
    /// Flex items, grid items, inline-blocks, table cells, and other
    /// flow-root-like replay roots use this variant. Keeping it explicit
    /// prevents a nested replay from applying an ancestor float band a second
    /// time.
    /// <https://www.w3.org/TR/CSS22/visuren.html#block-boxes>
    /// <https://www.w3.org/TR/css-display-3/#establishing-formatting-contexts>
    IsolatedFormattingContext,
}

/// Immutable placement inputs for replaying an independently formatted child.
///
/// A layout algorithm resolves this record before the child is replayed. The
/// child-layout boundary installs exactly these inputs and restores its caller
/// afterwards, rather than allowing the replay to reconstruct its geometry
/// from mutable [`LayoutBuilder`] state. Algorithm-specific sizing remains
/// outside this record; this is only the common formatting-context boundary.
/// <https://www.w3.org/TR/css-display-3/#independent-formatting-context>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PlacedFormattingContext {
    pub(in crate::layout) content_left: f32,
    /// The item's physical content-box width, independent of its writing
    /// mode. Its logical inline size may instead be its physical height.
    pub(in crate::layout) content_width: PhysicalContentWidth,
    /// A definite physical content-box height when one is available for
    /// descendant percentage resolution.
    pub(in crate::layout) content_height: Option<DefinitePhysicalContentHeight>,
    /// A flex/grid-assigned table wrapper border-box block size. CSS Tables
    /// distinguishes this used wrapper geometry from an authored `height`,
    /// which targets the table grid.
    pub(in crate::layout) table_wrapper_border_box_block_size: Option<BorderBoxLength>,
    /// The final logical inline content size to use while replaying this
    /// independently formatted item. This is separate from physical
    /// containing-block geometry because vertical writing maps its inline
    /// axis to physical height.
    pub(in crate::layout) replay_logical_inline_size: Option<LogicalInlineContentSize>,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) page_start_margin_policy: PageStartMarginPolicy,
    pub(in crate::layout) float_scope: ReplayFloatScope,
}

/// Sizing inputs supplied when an absolutely positioned table is replayed.
///
/// CSS Tables computes an auto table inline size against the containing block,
/// while CSS Positioned Layout resolves the final inset position separately.
/// A definite table block size must also reach table row distribution rather
/// than being lost in the generic positioned flow surrogate. Keeping these
/// values together prevents an inset-modified containing block from being
/// reused as the table's sizing basis, and prevents nested tables from
/// inheriting the outer table's inputs:
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>,
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>,
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning>, and
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PositionedTableSizing {
    pub(in crate::layout) available_inline_size: LogicalInlineContentSize,
    pub(in crate::layout) definite_block_content_size: Option<LogicalBlockContentSize>,
    pub(in crate::layout) writing_mode: WritingMode,
}

/// Return the table-wrapper size supplied by flex/grid alignment for an
/// auto-height table item.
///
/// An authored CSS `height` sizes the table grid, whereas flex/grid stretch
/// supplies the wrapper's used border-box size. Keeping that distinction at
/// the item-placement boundary prevents a final flex/grid size from being
/// mistaken for an authored table height:
/// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
pub(in crate::layout) fn auto_table_wrapper_block_size_override(
    style: &ComputedStyle,
    assigned_border_box_height: BorderBoxLength,
) -> Option<BorderBoxLength> {
    (style.display.is_table()
        && style.box_values.height.is_auto()
        && matches!(style.flex_basis, css::ComputedFlexBasis::Auto))
    .then_some(assigned_border_box_height)
}

impl<'a> LayoutBuilder<'a> {
    /// Temporarily set the containing block state for one placed flex/grid item.
    ///
    /// Flexbox and Grid compute an item's border-box geometry before replaying
    /// its independent formatting context. The replayed child layout must see
    /// the item's content box as its containing block, while the surrounding
    /// container layout resumes with its previous cursor and content bounds:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-items> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>.
    pub(in crate::layout) fn with_placed_formatting_context<R>(
        &mut self,
        placement: PlacedFormattingContext,
        style: &ComputedStyle,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_truncate_page_start_margins = self.truncate_page_start_margins;
        // The placed item is resolved against the flex/grid container's
        // writing mode. Using the child's writing mode here makes an
        // orthogonal item appear parallel to itself and loses a definite
        // physical-height inline-size basis during replay.
        // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-flows>
        let inherited_orthogonal_available_height = self
            .current_child_available_space()
            .orthogonal_available_height;
        let push_child_available_space = child_available_space_for_formatting_context(
            style,
            placement.content_width,
            placement.content_height,
            inherited_orthogonal_available_height,
            self.initial_containing_block_physical_height(),
        );
        let replay_logical_inline_size = placement.replay_logical_inline_size;
        let push_table_wrapper_block_size = placement.table_wrapper_border_box_block_size.is_some();
        self.content_left = placement.content_left;
        self.content_right = placement.content_left + placement.content_width.points();
        self.cursor_y = placement.cursor_y;
        self.child_available_space_stack
            .push(push_child_available_space);
        self.normal_flow_relative_containing_blocks
            .push(NormalFlowRelativeContainingBlock {
                physical_content_width: placement.content_width,
                physical_content_height: placement.content_height.map(|height| height.value()),
            });
        if push_table_wrapper_block_size {
            self.table_wrapper_block_size_overrides
                .push(placement.table_wrapper_border_box_block_size);
        }
        if let Some(inline_size) = replay_logical_inline_size {
            self.content_logical_inline_size_stack
                .push(inline_size.points());
        }
        if matches!(
            placement.page_start_margin_policy,
            PageStartMarginPolicy::Suppress
        ) {
            self.truncate_page_start_margins = false;
        }
        let result = self.with_replay_float_scope(placement.float_scope, layout);
        if replay_logical_inline_size.is_some() {
            self.content_logical_inline_size_stack.pop();
        }
        self.child_available_space_stack.pop();
        self.normal_flow_relative_containing_blocks.pop();
        if push_table_wrapper_block_size {
            self.table_wrapper_block_size_overrides.pop();
        }
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.truncate_page_start_margins = previous_truncate_page_start_margins;

        result
    }

    /// Run a nested replay with its declared float visibility.
    ///
    /// This is deliberately a closure-scoped operation: a nested formatting
    /// context cannot forget to restore the parent float stack after a
    /// measurement or paint replay.
    /// <https://www.w3.org/TR/CSS22/visuren.html#block-boxes>
    pub(in crate::layout) fn with_replay_float_scope<R>(
        &mut self,
        scope: ReplayFloatScope,
        replay: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let isolates = matches!(scope, ReplayFloatScope::IsolatedFormattingContext);
        if isolates {
            self.push_float_context();
        }
        let result = replay(self);
        if isolates {
            self.pop_float_context();
        }
        result
    }

    /// Consume the flex/grid table-wrapper size assigned to the root table in
    /// the current item-placement scope.
    ///
    /// Nested tables must use their own CSS table sizing, so the override is
    /// one-shot rather than inherited through table descendants:
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
    pub(in crate::layout) fn take_table_wrapper_block_size_override(
        &mut self,
    ) -> Option<BorderBoxLength> {
        self.table_wrapper_block_size_overrides
            .last_mut()
            .and_then(Option::take)
    }

    /// Install the one-shot sizing contract for the next positioned table
    /// formatting context. Nested table roots push their own contract before
    /// replay, so the outer record cannot leak into descendants.
    pub(in crate::layout) fn push_positioned_table_sizing(
        &mut self,
        sizing: PositionedTableSizing,
    ) {
        self.positioned_table_sizing.push(Some(sizing));
    }

    /// Consume the current positioned table's sizing contract.
    pub(in crate::layout) fn take_positioned_table_sizing(
        &mut self,
    ) -> Option<PositionedTableSizing> {
        self.positioned_table_sizing
            .last_mut()
            .and_then(Option::take)
    }

    /// Lay out the contents of a placed flex or grid item.
    ///
    /// CSS Flexbox and CSS Grid both make flex/grid items establish independent
    /// formatting contexts, but their algorithms compute item geometry before
    /// replaying the item's children through normal block layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-items> and
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>.
    pub(in crate::layout) fn layout_formatting_context_item_contents(
        &mut self,
        child: &FormattingContextChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            // A flex/grid item's own page value is selected by its container,
            // but block descendants still have class-A boundaries among
            // themselves. Suppress only element-entry scopes here; broad
            // named-page suppression would incorrectly erase those internal
            // boundaries.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            self.push_page_name_element_scope_suppression();
            if let Some(kind) = child.generated_pseudo_kind() {
                self.layout_generated_pseudo_box(
                    child_element,
                    placed_style,
                    kind.counter_event_source(),
                    stylesheets,
                    &[],
                    child_boxes,
                    None,
                    principal_box_paint_mode,
                );
            } else {
                self.layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
                    child_element,
                    placed_style,
                    stylesheets,
                    &[],
                    child_boxes,
                    child.table_fragment(),
                    false,
                    principal_box_paint_mode,
                );
            }
            self.pop_page_name_element_scope_suppression();
            self.ancestors.pop();
        } else if let Some(children) = child.anonymous_content() {
            // An anonymous flex item has no element dispatch through which to
            // consume the one-shot replay basis. Its anonymous block is the
            // root formatting context for the item, so consume the marker here
            // while the definite basis itself remains scoped by the caller.
            let _ = self.take_replayed_flex_item_percentage_height_basis();
            // Anonymous flex items do not have independently styleable
            // principal-box decoration, so parent ownership is a no-op.
            let _ = principal_box_paint_mode;
            self.layout_anonymous_block(placed_style, children, stylesheets, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_style_with_replay_state() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.margin = css::Edges {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        style.box_values.margin =
            css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(4.0),
            ));
        style.page = css::PageAssignment::Named(css::PageName::new("chapter".to_string()));
        style.break_before = PageBreak::Page;
        style.break_after = PageBreak::Page;
        style.break_inside = css::BreakInsideAvoidance::Avoid;
        style
    }

    fn assert_common_replay_state(style: &ComputedStyle) {
        assert_eq!(style.margin, css::Edges::ZERO);
        assert_eq!(
            style.box_values.margin,
            css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::ZERO)
        );
        assert!(!style.page.is_specified());
        assert_eq!(style.page, css::PageAssignment::Unspecified);
        assert_eq!(style.break_before, PageBreak::Auto);
        assert_eq!(style.break_after, PageBreak::Auto);
    }

    #[test]
    fn flex_replay_base_style_preserves_break_inside_avoidance() {
        let style = replayed_item_fragmentation_base_style(
            &source_style_with_replay_state(),
            ReplayedItemFragmentationPolicy::Flex,
        );

        assert_common_replay_state(&style);
        assert_eq!(style.break_inside, css::BreakInsideAvoidance::Avoid);
    }

    #[test]
    fn grid_replay_base_style_clears_break_inside_avoidance() {
        let style = replayed_item_fragmentation_base_style(
            &source_style_with_replay_state(),
            ReplayedItemFragmentationPolicy::Grid,
        );

        assert_common_replay_state(&style);
        assert_eq!(style.break_inside, css::BreakInsideAvoidance::Auto);
    }
}
