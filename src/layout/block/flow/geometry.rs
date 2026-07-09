use super::*;

pub(in crate::layout) fn writing_modes_are_orthogonal(a: WritingMode, b: WritingMode) -> bool {
    WritingModeAxes::new(a, Direction::Ltr).swaps_physical_axes()
        != WritingModeAxes::new(b, Direction::Ltr).swaps_physical_axes()
}

pub(in crate::layout) fn child_available_space_for_block(
    style: &ComputedStyle,
    content_width: f32,
    definite_content_height: Option<f32>,
    inherited_orthogonal_available_height: OrthogonalAvailableHeight,
    initial_fallback_height: f32,
) -> ChildAvailableSpace {
    let local_orthogonal_constraint = orthogonal_fallback_physical_content_height(
        style,
        PercentageBasis::definite(layout_pt(content_width)),
    );
    let mut space = ChildAvailableSpace::new(
        style.writing_mode,
        PhysicalContentWidth::new(content_box_pt(content_width)),
        definite_content_height.map(|height| PhysicalContentHeight::new(content_box_pt(height))),
        PhysicalContentHeight::new(content_box_pt(
            local_orthogonal_constraint
                .unwrap_or_else(|| inherited_orthogonal_available_height.value.points()),
        )),
    );
    if style_clips_overflow(style) {
        let initial = initial_fallback_height.max(0.0);
        space = space.with_orthogonal_available_height(OrthogonalAvailableHeight {
            value: PhysicalContentHeight::new(content_box_pt(
                local_orthogonal_constraint
                    .unwrap_or(initial)
                    .min(initial)
                    .max(0.0),
            )),
            source: local_orthogonal_constraint.map_or(
                OrthogonalAvailableSizeSource::InitialContainingBlock,
                |_| OrthogonalAvailableSizeSource::NearestScrollContainer,
            ),
        });
    }
    space
}

/// Fallback physical height for orthogonal descendants of an auto-height block.
///
/// CSS Writing Modes uses the containing block's fixed max block-size, floored
/// by its fixed min block-size, before falling back to the initial containing
/// block. This intentionally does not call `constrain_height`, because for
/// this available-size fallback a larger min-height floors the max-height.
/// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>.
pub(in crate::layout) fn orthogonal_fallback_physical_content_height(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<LayoutLength>,
) -> Option<f32> {
    let min_height = used_min_height(style, percentage_basis).map(SemanticLengthExt::points);
    let max_height = used_max_height(style, percentage_basis).map(SemanticLengthExt::points);
    match (min_height, max_height) {
        (Some(min_height), Some(max_height)) => Some(max_height.max(min_height)),
        (Some(min_height), None) => Some(min_height),
        (None, Some(max_height)) => Some(max_height),
        (None, None) => None,
    }
}

/// Returns whether block `align-content` needs descendant paint bounds.
///
/// Horizontal block containers know their alignment-subject block size from
/// normal-flow layout height. In vertical writing modes the block axis is
/// physical horizontal, so same-page alignment uses captured descendant paint
/// bounds as the concrete alignment subject:
/// <https://www.w3.org/TR/css-align-3/#align-content-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(in crate::layout) fn vertical_block_align_content_needs_fragment_bounds(
    style: &ComputedStyle,
) -> bool {
    WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes()
        && style.align_content.keyword != ContentAlignmentKeyword::Normal
}

pub(in crate::layout) fn vertical_block_align_content_x_offset(
    style: &ComputedStyle,
    content_left: f32,
    content_width: f32,
    subject_bounds: Option<PaintClip>,
) -> f32 {
    if !vertical_block_align_content_needs_fragment_bounds(style) {
        return 0.0;
    }
    let Some(subject_bounds) = subject_bounds else {
        return 0.0;
    };
    let subject_width = subject_bounds.width().max(0.0);
    let free_space = content_width.max(0.0) - subject_width;
    let toward_block_end = content_alignment_offset_toward_end(
        style.align_content,
        free_space,
        block_align_content_defaults_to_safe_overflow(style),
    );
    match block_start_side(style.writing_mode) {
        PhysicalSide::Left => content_left + toward_block_end - subject_bounds.x(),
        PhysicalSide::Right => {
            content_left + content_width.max(0.0)
                - toward_block_end
                - (subject_bounds.x() + subject_width)
        }
        PhysicalSide::Top | PhysicalSide::Bottom => 0.0,
    }
}

/// Return whether simple block text needs the full inline layout pipeline.
///
/// CSS Text and CSS Writing Modes features such as bidi reordering, word
/// spacing, letter spacing, transforms, and non-horizontal writing modes are
/// resolved by the inline item pipeline. Plain text layout can only be used for
/// text whose measured and painted form is equivalent without that machinery.
pub(in crate::layout) fn plain_inline_content_needs_inline_items(
    text: &str,
    style: &ComputedStyle,
) -> bool {
    style.writing_mode != WritingMode::HorizontalTb
        || contains_bidi_text(text)
        || style.used_word_spacing() != layout_pt(0.0)
        || style.used_letter_spacing() != layout_pt(0.0)
        || style.text_transform != css::TextTransform::NONE
        || style.text_decoration.clone().has_visible_line()
        || !style.text_decoration_layers.is_empty()
        || !matches!(style.text_emphasis_style, css::TextEmphasisStyle::None)
}

/// Return whether a last child's bottom margin can stay collapsed through the parent.
///
/// CSS 2.2 lets an in-flow last child's bottom margin adjoin its parent's bottom
/// margin when the parent has auto height and no separating border/padding/line
/// boxes. A used `min-height` only blocks that collapse when it increases the
/// parent's used content height, so block layout compares constraints against
/// the content height with the candidate child margin removed. If the collapse
/// is blocked, the child margin still must not inflate the parent's
/// min-height-constrained used height:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn block_end_margin_collapse_survives_height_constraints(
    style: &ComputedStyle,
    content_width: f32,
    vertical_extras: f32,
    content_height_without_child_margin: f32,
) -> bool {
    let requested_content_height = used_content_box_height_or_auto(
        style,
        layout_pt(content_height_without_child_margin),
        non_content_pt(vertical_extras),
    )
    .map(SemanticLengthExt::points)
    .unwrap_or(content_height_without_child_margin);
    let height_depends_on_intrinsic_content =
        needs_intrinsic_height_contribution(style.box_values.height.clone())
            || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
            || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
    let constrained_height = if height_depends_on_intrinsic_content {
        constrain_height_with_intrinsic(
            style,
            content_box_pt(requested_content_height),
            content_box_pt(content_height_without_child_margin),
            content_box_pt(content_height_without_child_margin),
            PercentageBasis::definite(content_box_pt(content_width)),
            non_content_pt(vertical_extras),
        )
        .points()
    } else {
        constrain_content_height(
            style,
            content_box_pt(requested_content_height),
            PercentageBasis::definite(layout_pt(content_width)),
        )
        .points()
    };
    constrained_height <= content_height_without_child_margin + 0.01
}

/// Horizontal and size inputs for one normal-flow block box.
///
/// CSS 2.2 block formatting computes inline-size, margins, padding, and
/// relative-position offsets before child layout determines the final block
/// extent. This struct therefore stores the pre-layout physical inline
/// geometry and exposes typed page-space helpers once a block top and used
/// content height are known:
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting> and
/// <https://www.w3.org/TR/CSS22/box.html>.
pub(in crate::layout) struct BlockLayoutGeometry {
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) relative_offset: RelativeOffset,
    pub(in crate::layout) border_widths: css::Edges,
    pub(in crate::layout) vertical_extras: f32,
    pub(in crate::layout) containing_block_content_height: BlockSizePercentageBasis,
    /// Definite physical content height exported for descendant percentage
    /// resolution. This remains physical even for a vertical block, whose
    /// logical inline size is the same axis.
    pub(in crate::layout) definite_content_height: Option<PhysicalContentHeight>,
    pub(in crate::layout) content_logical_inline_size: LogicalInlineContentSize,
    pub(in crate::layout) outer_inline: BlockInlineBounds,
    pub(in crate::layout) content_inline: BlockInlineBounds,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockLayoutInlineConstraint {
    pub(in crate::layout) containing_left: f32,
    pub(in crate::layout) containing_right: f32,
    /// Containing-block logical inline size used by margin and padding
    /// percentages.
    /// The containing block's logical inline percentage basis. This belongs
    /// to the containing flow, not the child's writing mode: an orthogonal
    /// child's percentage margins still resolve against its containing
    /// block's inline size.
    pub(in crate::layout) percentage_basis: PercentageBasis<LogicalInlineContentSize>,
    /// Containing-block physical width used by the physical `width` property.
    ///
    /// These two bases differ for orthogonal flows: CSS Box percentages keep
    /// using the containing block's logical inline size, while CSS Sizing's
    /// physical width maps to the child's logical block axis.
    /// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-flows>
    pub(in crate::layout) physical_width_percentage_basis: PhysicalContentWidth,
    /// A float-avoidance band can supply an auto width in border-box space.
    /// Keeping that space explicit prevents accidental comparison with the
    /// content-box physical width percentage basis above.
    pub(in crate::layout) auto_border_box_width: Option<BorderBoxLength>,
}

impl BlockLayoutGeometry {
    pub(in crate::layout) fn outer_inline(&self) -> BlockInlineBounds {
        self.outer_inline
    }

    pub(in crate::layout) fn content_inline(&self) -> BlockInlineBounds {
        self.content_inline
    }

    pub(in crate::layout) fn outer_width(&self) -> f32 {
        self.outer_inline.size()
    }

    pub(in crate::layout) fn content_width(&self) -> f32 {
        self.content_inline.size()
    }

    pub(in crate::layout) fn content_logical_inline_size(&self) -> LogicalInlineContentSize {
        self.content_logical_inline_size
    }

    /// Return the final block border box in block formatting coordinates.
    ///
    /// CSS Box defines the border box as the outer painted box excluding
    /// margins. Block layout knows the top edge before descendants are laid out
    /// and the final block size afterward, so this is the point where Quire can
    /// form a typed block-layout rectangle:
    /// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
    pub(in crate::layout) fn border_box_top_rect(
        &self,
        outer_x: f32,
        block_top: f32,
        block_height: f32,
    ) -> BlockBorderBox {
        BlockBorderBox::new(outer_x, block_top, self.outer_width(), block_height)
    }

    /// Return the block padding box as a top-edge page rectangle.
    ///
    /// CSS Positioned Layout uses the padding box of positioned ancestors as
    /// the containing block for absolute descendants:
    /// <https://www.w3.org/TR/css-position-3/#def-cb>.
    pub(in crate::layout) fn padding_box_top_rect(
        &self,
        outer_x: f32,
        block_top: f32,
        content_height: f32,
    ) -> PageTopRect {
        PageTopRect::new(
            outer_x + self.border_widths.left,
            block_top - self.border_widths.top,
            self.content_width() + self.style.padding.left + self.style.padding.right,
            content_height + self.style.padding.top + self.style.padding.bottom,
        )
    }
}

/// Physical inline-axis bounds for a block formatting box.
///
/// CSS normal-flow block layout resolves the used inline size and physical
/// inline-start offset before child layout determines the block-axis extent.
/// This wrapper keeps those values labelled as block formatting coordinates
/// instead of carrying unqualified `x` and `width` scalars through layout:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockInlineBounds {
    pub(in crate::layout) span: PageInlineSpan,
}

impl BlockInlineBounds {
    pub(in crate::layout) fn new(start: f32, size: f32) -> Self {
        Self {
            span: PageInlineSpan::new(start, size),
        }
    }

    pub(in crate::layout) fn start(self) -> f32 {
        self.span.left_x()
    }

    pub(in crate::layout) fn size(self) -> f32 {
        self.span.width()
    }
}

/// A CSS block border box in block formatting coordinates.
///
/// The origin is the physical top-left border edge used by CSS 2.2 normal-flow
/// block layout, and the block extent grows downward. This is intentionally a
/// block-layout type, not a paint-space rectangle; callers must project through
/// [`page_top_rect`](Self::page_top_rect) before creating paint or PDF data:
/// <https://www.w3.org/TR/CSS22/box.html#box-dimensions> and
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockBorderBox {
    pub(in crate::layout) rect: BlockRect,
}

impl BlockBorderBox {
    pub(in crate::layout) fn new(x: f32, top_y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: BlockRect::new(
                BlockPoint::new(x, top_y),
                BlockSize::new(width.max(0.0), height.max(0.0)),
            ),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(in crate::layout) fn top_y(self) -> f32 {
        self.rect.origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(in crate::layout) fn page_top_rect(self) -> PageTopRect {
        PageTopRect::new(self.x(), self.top_y(), self.width(), self.height())
    }
}
