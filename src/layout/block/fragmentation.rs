use super::super::*;

impl<'a> LayoutBuilder<'a> {
    /// Return whether this block should first be laid out speculatively for
    /// `break-inside: avoid`.
    ///
    /// CSS Fragmentation treats avoid breaks as a preference that should be
    /// honored when the box can be moved to the next fragmentainer without
    /// producing worse overflow. A forced break can leave the current page
    /// empty but still below the fragmentainer top because ancestor fragment
    /// offsets are preserved; that state must still protect descendant breaks:
    /// <https://www.w3.org/TR/css-break-3/#break-within>.
    pub(in crate::layout) fn should_try_avoid_break_inside(&self, style: &ComputedStyle) -> bool {
        style.break_inside_avoid
            && self.avoid_inside_retry_depth == 0
            && !style.break_before.is_forced()
            && !style.break_after.is_forced()
            && !style.display.is_none()
            && !style.display.is_inline_level()
            && !matches!(style.position, Position::Absolute | Position::Fixed)
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> bool {
        if !style.break_inside_avoid
            || self.avoid_inside_retry_depth > 0
            || style.break_before.is_forced()
            || style.break_after.is_forced()
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
        let Some(estimated_height) =
            self.estimate_element_height(element, style, stylesheets, available_width, child_boxes)
        else {
            return false;
        };
        let usable_page_height = self.page_area_height();
        let remaining_height = self.cursor_y - self.page_bottom();
        let should_break =
            estimated_height <= usable_page_height + 0.01 && estimated_height > remaining_height;
        if should_break {
            log::debug!(
                "moving break-inside: avoid <{}> to next page: estimated height {:.2}, remaining {:.2}",
                element.tag,
                estimated_height,
                remaining_height
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
    pub(in crate::layout) fn layout_avoiding_break_inside(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let snapshot = self.snapshot();
        let pages_before = snapshot.pages.len();
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        if self.pages.len() <= pages_before {
            return;
        }
        let split_layout = self.snapshot();
        let split_page_count = split_layout.pages.len();
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let avoid_box_fits_empty_page = self
            .estimate_element_height(element, style, stylesheets, available_width, child_boxes)
            .is_some_and(|height| {
                let inner_height = (height - style.margin.top - style.margin.bottom).max(0.0);
                inner_height <= self.page_area_height() + 0.01
            });

        self.restore(snapshot);
        self.push_page_if_nonempty();
        let mut retry_style = style.clone();
        retry_style.break_inside_avoid = false;
        // CSS Fragmentation treats `break-inside: avoid` as a constraint to
        // keep a box unfragmented when possible. Once an ancestor avoid box has
        // been moved to a fresh fragmentainer for a retry, nested avoid boxes
        // must not recursively push the kept contents again:
        // <https://www.w3.org/TR/css-break-3/#break-within>.
        if avoid_box_fits_empty_page {
            self.avoid_inside_retry_depth += 1;
        }
        self.layout_element_inner(
            element,
            &retry_style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        if avoid_box_fits_empty_page {
            self.avoid_inside_retry_depth -= 1;
        }
        if self.pages.len() > split_page_count {
            self.restore(split_layout);
        }
    }
}
