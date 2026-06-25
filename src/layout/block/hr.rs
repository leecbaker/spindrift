use super::super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_hr(&mut self, style: &ComputedStyle) {
        self.apply_forced_break(style.break_before);

        let mut geometry = self.hr_layout_geometry(style);
        if geometry.consumed_height > 0.0
            && self.cursor_y - geometry.applied_start_margin - geometry.consumed_height
                < self.page_bottom()
        {
            self.push_page();
            geometry = self.hr_layout_geometry(style);
        }
        self.cursor_y -= geometry.applied_start_margin;

        let style = &geometry.style;
        let x = self.content_left + style.margin.left;
        if style.visibility == Visibility::Visible {
            if let Some(fill) = style.background_color
                && geometry.consumed_height > 0.0
            {
                self.push_rect(RenderedRect {
                    x,
                    y: self.cursor_y - geometry.consumed_height,
                    width: geometry.width,
                    height: geometry.consumed_height,
                    fill: Some(fill),
                    stroke: None,
                    stroke_width: 0.0,
                });
            }
            if geometry.line_height > 0.0 && geometry.width > 0.0 {
                let y = self.cursor_y - ((geometry.consumed_height + geometry.line_height) / 2.0);
                self.push_rect(RenderedRect {
                    x,
                    y,
                    width: geometry.width,
                    height: geometry.line_height,
                    fill: Some(style.border_color),
                    stroke: None,
                    stroke_width: 0.0,
                });
            }
        }

        self.cursor_y -= geometry.consumed_height + style.margin.bottom;
        self.apply_forced_break(style.break_after);
    }

    fn hr_layout_geometry(&self, style: &ComputedStyle) -> HrLayoutGeometry {
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let used_edges = used_box_edges(&used_style, containing_inline_size);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        let available_width = self.content_right
            - self.content_left
            - used_style.margin.left
            - used_style.margin.right;
        let width = used_length_percentage_or_auto(used_style.box_values.width, available_width)
            .unwrap_or(available_width)
            .max(0.0);
        let line_height = used_border_width(&used_style);
        let consumed_height =
            used_length_percentage_or_auto(used_style.box_values.height, line_height)
                .unwrap_or(line_height)
                .max(line_height);
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        let applied_start_margin = page_start_margin(used_style.margin.top, starts_at_page_top);

        HrLayoutGeometry {
            style: used_style,
            width,
            line_height,
            consumed_height,
            applied_start_margin,
        }
    }
}

struct HrLayoutGeometry {
    style: ComputedStyle,
    width: f32,
    line_height: f32,
    consumed_height: f32,
    applied_start_margin: f32,
}
