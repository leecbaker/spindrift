use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum PageStartMarginPolicy {
    Preserve,
    Suppress,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FormattingContextItemPlacement {
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) page_start_margin_policy: PageStartMarginPolicy,
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
    pub(in crate::layout) fn with_formatting_context_item_placement<R>(
        &mut self,
        placement: FormattingContextItemPlacement,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_truncate_page_start_margins = self.truncate_page_start_margins;

        self.content_left = placement.content_left;
        self.content_right = placement.content_left + placement.content_width.max(0.0);
        self.cursor_y = placement.cursor_y;
        if matches!(
            placement.page_start_margin_policy,
            PageStartMarginPolicy::Suppress
        ) {
            self.truncate_page_start_margins = false;
        }

        let result = layout(self);

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.truncate_page_start_margins = previous_truncate_page_start_margins;

        result
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
        stylesheets: &[Stylesheet],
    ) {
        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.push_page_name_element_scope_suppression();
            self.layout_element_with_child_boxes(
                child_element,
                placed_style,
                stylesheets,
                child_boxes,
            );
            self.pop_page_name_element_scope_suppression();
            self.ancestors.pop();
        } else if let Some(children) = child.anonymous_content() {
            self.layout_anonymous_block(placed_style, children, stylesheets, None);
        }
    }
}
