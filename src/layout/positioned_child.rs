use super::*;

/// Selects which positioned-descendant containing-block stacks a box establishes.
///
/// CSS Positioned Layout defines the absolute-position containing block for
/// positioned ancestors, while layout/paint containment and transforms also
/// establish the containing block for fixed-position descendants:
/// <https://www.w3.org/TR/css-position-3/#def-cb> and
/// <https://drafts.csswg.org/css-transforms-1/#transform-rendering>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PositionedContainingBlockMode {
    AbsoluteOnly,
    FixedAndAbsolute,
}

impl PositionedContainingBlockMode {
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Option<Self> {
        if style.contain.layout || style.contain.paint || style.has_transform() {
            Some(Self::FixedAndAbsolute)
        } else if matches!(style.position, Position::Relative | Position::Sticky) {
            Some(Self::AbsoluteOnly)
        } else {
            None
        }
    }

    fn establishes_fixed_containing_block(self) -> bool {
        matches!(self, Self::FixedAndAbsolute)
    }
}

/// Records the stack depths before a positioned-containing-block scope.
///
/// The token intentionally does not borrow the builder, so a caller can retain
/// it while replaying fragments in a separate temporary layout context.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedContainingBlockScope {
    containing_blocks_depth: usize,
    fixed_containing_blocks_depth: usize,
    mode: PositionedContainingBlockMode,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedChildStaticRect {
    left: f32,
    right: f32,
    top: f32,
    containing_block: Option<ContainingBlock>,
    grid_alignment: Option<GridAbsposStaticAlignment>,
}

impl PositionedChildStaticRect {
    pub(in crate::layout) fn new(left: f32, right: f32, top: f32) -> Self {
        Self {
            left,
            right,
            top,
            containing_block: None,
            grid_alignment: None,
        }
    }

    pub(in crate::layout) fn with_containing_block(
        left: f32,
        right: f32,
        top: f32,
        containing_block: ContainingBlock,
    ) -> Self {
        Self {
            left,
            right,
            top,
            containing_block: Some(containing_block),
            grid_alignment: None,
        }
    }

    pub(in crate::layout) fn with_grid_alignment(
        mut self,
        grid_alignment: GridAbsposStaticAlignment,
    ) -> Self {
        self.grid_alignment = Some(grid_alignment);
        self
    }

    fn layout_right(self) -> f32 {
        self.left + (self.right - self.left).max(1.0)
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Push the positioned-containing-block stacks established by one box.
    ///
    /// Callers retain ownership of the box geometry; this only centralizes the
    /// paired stack lifecycle required by CSS Positioned Layout and Transforms.
    /// <https://www.w3.org/TR/css-position-3/#def-cb> and
    /// <https://drafts.csswg.org/css-transforms-1/#transform-rendering>.
    pub(in crate::layout) fn push_positioned_containing_block(
        &mut self,
        mode: PositionedContainingBlockMode,
        containing_block: ContainingBlock,
    ) -> PositionedContainingBlockScope {
        let scope = PositionedContainingBlockScope {
            containing_blocks_depth: self.containing_blocks.len(),
            fixed_containing_blocks_depth: self.fixed_containing_blocks.len(),
            mode,
        };
        self.containing_blocks.push(containing_block);
        if mode.establishes_fixed_containing_block() {
            self.fixed_containing_blocks.push(containing_block);
        }
        scope
    }

    /// Restore the positioned-containing-block stacks recorded by `scope`.
    ///
    /// The depth assertions make leaked nested scopes visible at their owning
    /// layout boundary rather than silently popping an ancestor's geometry.
    pub(in crate::layout) fn pop_positioned_containing_block(
        &mut self,
        scope: PositionedContainingBlockScope,
    ) {
        debug_assert_eq!(
            self.containing_blocks.len(),
            scope.containing_blocks_depth + 1,
            "positioned containing-block scopes must be popped in nesting order",
        );
        debug_assert_eq!(
            self.fixed_containing_blocks.len(),
            scope.fixed_containing_blocks_depth
                + usize::from(scope.mode.establishes_fixed_containing_block()),
            "fixed containing-block scopes must be popped in nesting order",
        );
        if scope.mode.establishes_fixed_containing_block() {
            self.fixed_containing_blocks.pop();
        }
        self.containing_blocks.pop();
        debug_assert_eq!(self.containing_blocks.len(), scope.containing_blocks_depth,);
        debug_assert_eq!(
            self.fixed_containing_blocks.len(),
            scope.fixed_containing_blocks_depth,
        );
    }

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
    ) {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_absolute_static_position = self.absolute_static_position;
        let pushed_containing_block = if let Some(containing_block) = static_rect.containing_block {
            self.containing_blocks.push(containing_block);
            true
        } else {
            false
        };

        self.content_left = static_rect.left;
        self.content_right = static_rect.layout_right();
        self.cursor_y = static_rect.top;
        let static_position = AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
            static_rect.left,
            static_rect.right,
            static_rect.top,
            true,
        );
        self.absolute_static_position = Some(match static_rect.grid_alignment {
            Some(grid_alignment) => static_position.with_grid_alignment(grid_alignment),
            None => static_position,
        });

        let mut positioned_style = child.style.clone();
        if positioned_style.display.is_inline_level() {
            positioned_style.display = positioned_style.display.blockified();
        }

        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.layout_element_with_child_boxes(
                child_element,
                &positioned_style,
                stylesheets,
                child_boxes,
            );
            self.ancestors.pop();
        }

        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.absolute_static_position = previous_absolute_static_position;
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn positioned_containing_block_mode_follows_positioning_and_effects() {
        let mut style = ComputedStyle::initial();
        assert_eq!(PositionedContainingBlockMode::for_style(&style), None);

        style.position = Position::Relative;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::AbsoluteOnly),
        );

        style.position = Position::Static;
        style.contain.layout = true;
        assert_eq!(
            PositionedContainingBlockMode::for_style(&style),
            Some(PositionedContainingBlockMode::FixedAndAbsolute),
        );
    }

    #[test]
    fn positioned_containing_block_scope_restores_both_stack_variants() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 20.0, 30.0, 40.0));

        let initial_containing_blocks = builder.containing_blocks.len();
        let initial_fixed_containing_blocks = builder.fixed_containing_blocks.len();
        let absolute_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            containing_block,
        );
        assert_eq!(
            builder.containing_blocks.len(),
            initial_containing_blocks + 1
        );
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );
        builder.pop_positioned_containing_block(absolute_scope);
        assert_eq!(builder.containing_blocks.len(), initial_containing_blocks);
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );

        let fixed_scope = builder.push_positioned_containing_block(
            PositionedContainingBlockMode::FixedAndAbsolute,
            containing_block,
        );
        assert_eq!(
            builder.containing_blocks.len(),
            initial_containing_blocks + 1
        );
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks + 1
        );
        builder.pop_positioned_containing_block(fixed_scope);
        assert_eq!(builder.containing_blocks.len(), initial_containing_blocks);
        assert_eq!(
            builder.fixed_containing_blocks.len(),
            initial_fixed_containing_blocks
        );
    }
}
