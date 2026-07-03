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
        let item_width = item.width.max(0.0);
        let item_height = item.height.max(0.0);

        let mut placed_style = grid_item_layout_style(&child.style);
        placed_style.margin = css::Edges::ZERO;
        placed_style.page_name_specified = false;
        placed_style.page_name = None;
        suppress_grid_item_fragmentation_breaks(&mut placed_style);
        set_style_used_width(&mut placed_style, item_width);
        set_style_used_height(&mut placed_style, item_height);
        set_style_used_width_bounds(&mut placed_style, item_width);
        set_style_used_height_bounds(&mut placed_style, item_height);
        placed_style.box_sizing = BoxSizing::BorderBox;

        self.with_formatting_context_item_placement(
            FormattingContextItemPlacement {
                content_left: inner_x + item.x,
                content_width: item_width,
                cursor_y: content_top - item.y,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            |layout| {
                layout.layout_formatting_context_item_contents(child, &placed_style, stylesheets);
            },
        );
    }
}

fn suppress_grid_item_fragmentation_breaks(style: &mut ComputedStyle) {
    style.break_before = PageBreak::Auto;
    style.break_after = PageBreak::Auto;
    style.break_inside_avoid = false;
}
