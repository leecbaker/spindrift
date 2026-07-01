use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Replay one laid-out grid item through Quire's existing child layout path.
    ///
    /// CSS Grid computes a grid area's geometry, then the grid item establishes
    /// its own formatting context inside that area:
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>.
    pub(super) fn replay_grid_item(
        &mut self,
        child: &GridChild<'_>,
        item: &GridItemLayout,
        stylesheets: &[Stylesheet],
        inner_x: f32,
        content_top: f32,
    ) {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let item_width = item.width.max(0.0);
        let item_height = item.height.max(0.0);

        self.content_left = inner_x + item.x;
        self.content_right = self.content_left + item_width;
        self.cursor_y = content_top - item.y;

        let mut placed_style = child.style.clone();
        placed_style.margin = css::Edges::ZERO;
        placed_style.page_name_specified = false;
        placed_style.page_name = None;
        suppress_grid_item_fragmentation_breaks(&mut placed_style);
        set_style_used_width(&mut placed_style, item_width);
        set_style_used_height(&mut placed_style, item_height);
        set_style_used_width_bounds(&mut placed_style, item_width);
        set_style_used_height_bounds(&mut placed_style, item_height);
        placed_style.box_sizing = BoxSizing::BorderBox;
        if placed_style.display.is_inline_level() {
            placed_style.display = placed_style.display.blockified();
        }

        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.push_page_name_element_scope_suppression();
            self.layout_element_with_child_boxes(
                child_element,
                &placed_style,
                stylesheets,
                child_boxes,
            );
            self.pop_page_name_element_scope_suppression();
            self.ancestors.pop();
        } else if let Some(children) = child.anonymous_content() {
            self.layout_anonymous_block(&placed_style, children, stylesheets, None);
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }
}

fn suppress_grid_item_fragmentation_breaks(style: &mut ComputedStyle) {
    style.break_before = PageBreak::Auto;
    style.break_after = PageBreak::Auto;
    style.break_inside_avoid = false;
}
