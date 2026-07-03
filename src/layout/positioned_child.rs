use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedChildStaticRect {
    left: f32,
    right: f32,
    top: f32,
}

impl PositionedChildStaticRect {
    pub(in crate::layout) fn new(left: f32, right: f32, top: f32) -> Self {
        Self { left, right, top }
    }

    fn layout_right(self) -> f32 {
        self.left + (self.right - self.left).max(1.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum PositionedFormattingChildReplayMode {
    AbsoluteStaticRect,
    InlineStaticPosition,
}

impl<'a> LayoutBuilder<'a> {
    /// Replay an absolutely positioned flex/grid child from a precomputed
    /// static-position rectangle.
    ///
    /// CSS Flexbox and CSS Grid compute different hypothetical positions for
    /// out-of-flow children, but both replay the same child under that temporary
    /// static-position geometry:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items> and
    /// <https://www.w3.org/TR/css-grid-1/#abspos-items>.
    pub(in crate::layout) fn layout_positioned_formatting_context_child(
        &mut self,
        child: &FormattingContextChild<'_>,
        stylesheets: &[Stylesheet],
        static_rect: PositionedChildStaticRect,
        mode: PositionedFormattingChildReplayMode,
    ) {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_absolute_static_position = self.absolute_static_position;

        self.content_left = static_rect.left;
        self.content_right = static_rect.layout_right();
        self.cursor_y = static_rect.top;

        if matches!(
            mode,
            PositionedFormattingChildReplayMode::AbsoluteStaticRect
        ) {
            self.absolute_static_position = Some(AbsoluteStaticPosition::from_page_rect(
                static_rect.left,
                static_rect.right,
                static_rect.top,
            ));
        }

        let mut positioned_style = child.style.clone();
        if positioned_style.display.is_inline_level() {
            positioned_style.display = positioned_style.display.blockified();
        }
        if matches!(
            mode,
            PositionedFormattingChildReplayMode::InlineStaticPosition
        ) {
            positioned_style.abspos_static_source_was_inline_level = true;
            positioned_style.abspos_static_source_was_atomic_inline =
                child.style.display.is_atomic_inline();
        }

        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            match mode {
                PositionedFormattingChildReplayMode::AbsoluteStaticRect => {
                    self.layout_element_with_child_boxes(
                        child_element,
                        &positioned_style,
                        stylesheets,
                        child_boxes,
                    );
                }
                PositionedFormattingChildReplayMode::InlineStaticPosition => {
                    self.layout_positioned_block_with_inline_static_position(
                        child_element,
                        &positioned_style,
                        stylesheets,
                        child_boxes,
                        None,
                        InlineStaticPosition {
                            start_x: self.content_left,
                            end_x: self.content_right,
                            top_y: self.cursor_y,
                            baseline_y: self.cursor_y,
                            use_margin_box_top: false,
                        },
                    );
                }
            }
            self.ancestors.pop();
        }

        self.absolute_static_position = previous_absolute_static_position;
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }
}
