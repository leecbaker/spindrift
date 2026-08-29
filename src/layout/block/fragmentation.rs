use super::super::*;

fn has_table_or_replaced_descendant(element: &Element) -> bool {
    element.children.iter().any(|child| {
        let NodeKind::Element(child_element) = &child.kind else {
            return false;
        };
        is_table_or_replaced_element(child_element)
            || has_table_or_replaced_descendant(child_element)
    })
}

fn has_table_or_replaced_descendant_box(child_boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    child_boxes.iter().any(|child| match child {
        box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Replaced(_) => true,
        box_tree::FormattingBox::AtomicInline(box_) => {
            is_table_or_replaced_element(box_.core.element)
                || has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::Block(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::Inline(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            has_table_or_replaced_descendant_box(&box_.children)
        }
        box_tree::FormattingBox::Flex(box_) => {
            has_table_or_replaced_descendant_box(&box_.core.children)
        }
        box_tree::FormattingBox::Text(_) => false,
    })
}
impl<'a> LayoutBuilder<'a> {
    /// Measure the final outer extent that an avoid-constrained child occupies
    /// in its fragmentation context's block direction.
    ///
    /// A vertical flow fragments across physical X, so its logical block-size
    /// (including a winning `min-block-size`) is the used physical width, not
    /// the physical-height estimate used by horizontal block layout. This
    /// post-constraint measurement does not export a descendant percentage
    /// basis.
    /// <https://drafts.csswg.org/css-logical/#logical-dimension-properties>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn avoid_break_fragmentation_extent(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_outer_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        fragmentation_writing_mode: WritingMode,
    ) -> Option<LayoutLength> {
        if !WritingModeAxes::new(fragmentation_writing_mode, self.containing_block_direction)
            .swaps_physical_axes()
        {
            return self
                .estimate_element_height(
                    element,
                    style,
                    stylesheets,
                    available_outer_width,
                    child_boxes,
                )
                .map(layout_pt);
        }

        let geometry = self.block_layout_geometry(element, style, stylesheets, child_boxes);
        let used_style = &geometry.style;
        Some(layout_pt(
            geometry.outer_inline().width().points()
                + used_style.margin.left
                + used_style.margin.right,
        ))
    }

    /// Return the active fragmentainer's capacity in the root block-flow
    /// direction used by an avoid decision.
    ///
    /// Temporary multicolumn pages use their physical width when that flow is
    /// vertical, even though their legacy cursor is represented in physical Y.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn avoid_break_current_fragmentainer(
        &self,
        writing_mode: WritingMode,
        content_block_start: PageTopBlockPosition,
    ) -> Fragmentainer {
        if WritingModeAxes::new(writing_mode, self.containing_block_direction).swaps_physical_axes()
        {
            let extent = layout_pt(self.current_page_context.area_width());
            Fragmentainer::new(extent, extent)
        } else {
            self.fragmentainer_from_page_cursor(content_block_start)
        }
    }

    /// Return the next empty fragmentainer in the active root flow's block
    /// direction for avoid-run planning.
    ///
    /// The destination can use a different page or anonymous-column context,
    /// so this must project that context as well as the current one.
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    pub(in crate::layout) fn next_empty_avoid_break_fragmentainer(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        writing_mode: WritingMode,
    ) -> Fragmentainer {
        if !WritingModeAxes::new(writing_mode, self.containing_block_direction)
            .swaps_physical_axes()
        {
            return self.next_empty_fragmentainer(fragmentainer_kind);
        }

        let context = match fragmentainer_kind {
            FragmentainerKind::Page => self.resolved_page_context(
                self.destination_document_page_number(self.pages.len() + 2),
                false,
            ),
            FragmentainerKind::Column => self
                .fragmentainer_override
                .map(|override_| override_.context_for_fragmentainer(self.pages.len() + 1))
                .unwrap_or(self.current_page_context),
        };
        let extent = layout_pt(context.area_width());
        Fragmentainer::new(extent, extent)
    }

    /// Return whether this block should first be laid out speculatively for
    /// `break-inside: avoid`.
    ///
    /// CSS Fragmentation treats avoid breaks as a preference that should be
    /// honored when the box can be moved to the next fragmentainer without
    /// producing worse overflow. A forced break can leave the current page
    /// empty but still below the fragmentainer top because ancestor fragment
    /// offsets are preserved; that state must still protect descendant breaks:
    /// <https://www.w3.org/TR/css-break-3/#break-within>.
    pub(in crate::layout) fn should_try_avoid_break_inside(
        &self,
        style: &ComputedStyle,
        fragmentainer_kind: FragmentainerKind,
    ) -> bool {
        let break_context = FragmentBreakContext::for_standalone_box(style);
        fragmentainer_kind.avoids_break_inside(style)
            && self.avoid_inside_retry_depth == 0
            && break_context
                .forced_break_before_in(fragmentainer_kind)
                .is_none()
            && break_context
                .forced_break_after_in(fragmentainer_kind)
                .is_none()
            && !style.display.is_none()
            && !style.display.is_inline_level()
            && !matches!(style.position, Position::Absolute | Position::Fixed)
            && self.current_page_has_content()
            && !self.cursor_is_at_page_top()
    }

    /// Return whether an avoid-break descendant should move before layout.
    ///
    /// CSS Fragmentation permits an unforced break before a box to satisfy
    /// `break-inside: avoid` on an ancestor when the kept subtree fits in the
    /// next fragmentainer:
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules> and
    /// <https://www.w3.org/TR/css-break-3/#break-within>.
    pub(in crate::layout) fn should_prebreak_avoid_inside(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        fragmentainer_kind: FragmentainerKind,
    ) -> bool {
        let break_context = FragmentBreakContext::for_standalone_box(style);
        if !fragmentainer_kind.avoids_break_inside(style)
            || self.avoid_inside_retry_depth > 0
            || break_context
                .forced_break_before_in(fragmentainer_kind)
                .is_some()
            || break_context
                .forced_break_after_in(fragmentainer_kind)
                .is_some()
            || style.display.is_none()
            || style.display.is_inline_level()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || !child_boxes
                .map(has_table_or_replaced_descendant_box)
                .unwrap_or_else(|| has_table_or_replaced_descendant(element))
            || !self.current_page_has_content()
            || self.cursor_is_at_page_top()
        {
            return false;
        }

        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let Some(required_block_size) = self.avoid_break_fragmentation_extent(
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
            self.containing_block_writing_mode,
        ) else {
            return false;
        };
        let current_fragmentainer = self.avoid_break_current_fragmentainer(
            self.containing_block_writing_mode,
            PageTopBlockPosition::new(self.cursor_y),
        );
        let should_break = FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: true,
            current_fragmentainer,
            required_block_size,
            empty_fragmentainer: current_fragmentainer,
            empty_fit_block_size: required_block_size,
        })
        .should_break;
        if should_break {
            log::debug!(
                "moving break-inside: avoid <{}> to next page: required block extent {:.2}, remaining {:.2}",
                element.tag,
                required_block_size.points(),
                current_fragmentainer.available_block_size().points()
            );
        }
        should_break
    }

    /// Retry a block with `break-inside: avoid` from the next page when useful.
    ///
    /// The first pass detects whether normal layout fragmented the box. If the
    /// box's content fits an empty page, the retry suppresses recursive avoid
    /// decisions inside that same kept subtree so nested avoid boxes do not push
    /// each other onto additional pages:
    /// <https://www.w3.org/TR/css-break-3/#break-within>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_avoiding_break_inside(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        let snapshot = self.snapshot();
        let pages_before = snapshot.page_count();
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            principal_box_paint_mode,
            None,
        );
        if self.pages.len() <= pages_before {
            return;
        }
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let avoid_box_fits_empty_page = self
            .avoid_break_fragmentation_extent(
                element,
                style,
                stylesheets,
                available_width,
                child_boxes,
                self.containing_block_writing_mode,
            )
            .is_some_and(|required_block_size| {
                // The speculative pass may have ended partway through a
                // destination page. Test the retry against that page's
                // actual fragmentainer start, not the post-fragmentation
                // cursor, so an otherwise keepable nested box does not lose
                // its avoid constraint.
                self.avoid_break_current_fragmentainer(
                    self.containing_block_writing_mode,
                    PageTopBlockPosition::new(self.page_top()),
                )
                .block_size_fits_empty(required_block_size)
            });

        self.restore(snapshot);
        let continuation = self.block_page_break_continuation_context();
        self.push_page_if_nonempty();
        if !self.current_page_has_content()
            && self.active_fragmentainer_kind() == FragmentainerKind::Page
        {
            let page_number = self.destination_document_page_number(self.pages.len() + 1);
            let context = self.resolved_page_context(page_number, false);
            self.replay_fragment_continuation_on_page(&continuation, context);
        }
        let mut retry_style = style.clone();
        retry_style.break_inside = css::BreakInsideAvoidance::Auto;
        // CSS Fragmentation treats `break-inside: avoid` as a constraint to
        // keep a box unfragmented when possible. Once an ancestor avoid box has
        // been moved to a fresh fragmentainer for a retry, nested avoid boxes
        // must not recursively push the kept contents again:
        // <https://www.w3.org/TR/css-break-3/#break-within>.
        if avoid_box_fits_empty_page {
            self.avoid_inside_retry_depth += 1;
        }
        self.layout_element_inner_with_principal_effect_context(
            element,
            &retry_style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            principal_box_paint_mode,
            None,
        );
        if avoid_box_fits_empty_page {
            self.avoid_inside_retry_depth -= 1;
        }
        // Retain the retry even when it spans more fragmentainers. A higher
        // ancestor's `break-inside: avoid` may move before its first
        // descendant while a nested avoid group moves again at the next
        // class-A opportunity; restoring the shorter split layout would
        // discard both avoidance constraints merely to save a page.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
    }
}
