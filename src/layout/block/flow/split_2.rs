use super::*;

pub(in crate::layout) fn writing_modes_are_orthogonal(a: WritingMode, b: WritingMode) -> bool {
    matches!(
        (a, b),
        (
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl | WritingMode::VerticalLr
        ) | (
            WritingMode::VerticalRl | WritingMode::VerticalLr,
            WritingMode::HorizontalTb
        )
    )
}

pub(in crate::layout) fn child_available_space_for_block(
    style: &ComputedStyle,
    content_width: f32,
    definite_content_height: Option<f32>,
    initial_fallback_height: f32,
) -> ChildAvailableSpace {
    ChildAvailableSpace::new(
        style.writing_mode,
        content_width,
        definite_content_height,
        orthogonal_fallback_physical_content_height(style, content_width)
            .unwrap_or(initial_fallback_height),
    )
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
    percentage_basis: f32,
) -> Option<f32> {
    let min_height = used_min_height(style, percentage_basis);
    let max_height = used_max_height(style, percentage_basis);
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
    style.writing_mode != WritingMode::HorizontalTb
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
        || style.used_word_spacing() != 0.0
        || style.used_letter_spacing() != 0.0
        || style.text_transform != css::TextTransform::NONE
        || style.text_decoration.has_visible_line()
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
    let requested_content_height =
        used_content_height_or_auto(style, content_height_without_child_margin, vertical_extras)
            .unwrap_or(content_height_without_child_margin);
    let constrained_height = constrain_height(style, requested_content_height, content_width);
    constrained_height <= content_height_without_child_margin + 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_align_content_offset_uses_single_subject_fallbacks() {
        assert_eq!(
            block_align_content_y_offset(AlignContent::new(ContentAlignmentKeyword::End), 30.0),
            -30.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::new(ContentAlignmentKeyword::SpaceAround),
                30.0
            ),
            -15.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::safe(ContentAlignmentKeyword::Center),
                -20.0,
            ),
            0.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::unsafe_position(ContentAlignmentKeyword::Center),
                -20.0,
            ),
            10.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::new(ContentAlignmentKeyword::LastBaseline),
                -20.0
            ),
            0.0
        );
        let mut scroll_container_style = ComputedStyle::initial();
        scroll_container_style.align_content = AlignContent::new(ContentAlignmentKeyword::Center);
        scroll_container_style.overflow_y = css::Overflow::Auto;
        assert_eq!(
            block_align_content_y_offset_for_style(&scroll_container_style, -20.0),
            10.0
        );
        assert!(
            block_align_content_establishes_independent_formatting_context(AlignContent::new(
                ContentAlignmentKeyword::Center
            ))
        );
        assert!(
            !block_align_content_establishes_independent_formatting_context(AlignContent::new(
                ContentAlignmentKeyword::Normal
            ))
        );
    }

    #[test]
    fn vertical_block_align_content_offsets_use_logical_block_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.align_content = AlignContent::new(ContentAlignmentKeyword::Center);
        let subject = PaintClip::from_paint_rect(paint_space_rect(10.0, 20.0, 20.0, 40.0));
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            30.0
        );

        style.align_content = AlignContent::new(ContentAlignmentKeyword::End);
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            60.0
        );

        style.writing_mode = WritingMode::VerticalRl;
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            0.0
        );
    }

    #[test]
    fn block_border_box_projects_top_edge_to_paint_space() {
        let border_box = BlockBorderBox::new(12.0, 90.0, 40.0, 25.0);
        let page_top_rect = border_box.page_top_rect();
        assert_eq!(page_top_rect.bottom_y(), 65.0);
        assert_eq!(
            page_top_rect.paint_rect(),
            paint_space_rect(12.0, 65.0, 40.0, 25.0)
        );
    }
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
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) content_logical_inline_size: f32,
    pub(in crate::layout) outer_inline: BlockInlineBounds,
    pub(in crate::layout) content_inline: BlockInlineBounds,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockLayoutInlineConstraint {
    pub(in crate::layout) containing_left: f32,
    pub(in crate::layout) containing_right: f32,
    pub(in crate::layout) percentage_basis: f32,
    pub(in crate::layout) auto_border_box_width: Option<f32>,
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

    pub(in crate::layout) fn content_logical_inline_size(&self) -> f32 {
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

/// Inputs for deciding whether a definite-height block should prebreak.
///
/// CSS Fragmentation allows class A breaks between sibling block boxes before
/// layout. Keeping those inputs together makes the decision explicit while
/// allowing avoid-retry pagination state to tailor the rule:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
pub(in crate::layout) struct DefiniteBlockBreakContext<'a> {
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) vertical_extras: f32,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) remaining_height: f32,
    pub(in crate::layout) page_area_height: f32,
    pub(in crate::layout) current_page_has_content: bool,
    pub(in crate::layout) at_page_top: bool,
    pub(in crate::layout) suppress_for_avoid_retry: bool,
}

pub(in crate::layout) struct AvoidBreakRunCandidateMeta {
    pub(in crate::layout) index: usize,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: bool,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) height: f32,
}

pub(in crate::layout) struct PendingAvoidBreakRunCandidate {
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

pub(in crate::layout) struct AvoidBreakRunCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AvoidBreakRunCandidateMeta,
}

impl PendingAvoidBreakRunCandidate {
    /// Capture before the first builder mutation that a later avoid-break
    /// retry must undo.
    pub(in crate::layout) fn arm(self, builder: &LayoutBuilder<'_>) -> AvoidBreakRunCandidate {
        AvoidBreakRunCandidate {
            snapshot: Box::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl AvoidBreakRunCandidate {
    pub(in crate::layout) fn height(&self) -> f32 {
        self.meta.height
    }

    pub(in crate::layout) fn add_height(mut self, height: f32) -> Self {
        self.meta.height += height;
        self
    }

    pub(in crate::layout) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> AvoidBreakRunCandidateMeta {
        builder.restore(*self.snapshot);
        self.meta
    }
}

pub(in crate::layout) struct AdjoiningFloatReplayCandidateMeta {
    pub(in crate::layout) index: usize,
    pub(in crate::layout) element_index: usize,
    pub(in crate::layout) previous_flow_bottom_margin: Option<f32>,
    pub(in crate::layout) seen_flow_child: bool,
    pub(in crate::layout) trim_block_start_adjoining_margins: bool,
    pub(in crate::layout) collapsed_end_margin: bool,
    pub(in crate::layout) previous_child_page_end: Option<Option<String>>,
    pub(in crate::layout) float_run: FloatRunState,
    pub(in crate::layout) previous_break_after_avoid: bool,
}

pub(in crate::layout) struct PendingAdjoiningFloatReplayCandidate {
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
}

pub(in crate::layout) struct AdjoiningFloatReplayCandidate {
    snapshot: Box<LayoutSnapshot>,
    pub(in crate::layout) meta: AdjoiningFloatReplayCandidateMeta,
}

impl PendingAdjoiningFloatReplayCandidate {
    /// Capture before the self-collapsing child layout whose adjoining floats
    /// may need to be replayed at a later collapsed-margin origin.
    pub(in crate::layout) fn arm(
        self,
        builder: &LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidate {
        AdjoiningFloatReplayCandidate {
            snapshot: Box::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl AdjoiningFloatReplayCandidate {
    pub(in crate::layout) fn snapshot(&self) -> &LayoutSnapshot {
        &self.snapshot
    }

    pub(in crate::layout) fn snapshot_cursor_y(&self) -> f32 {
        self.snapshot.cursor_y
    }

    pub(in crate::layout) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> AdjoiningFloatReplayCandidateMeta {
        builder.restore(*self.snapshot);
        self.meta
    }
}

pub(in crate::layout) fn should_move_avoid_break_run_to_next_page(
    run_height: f32,
    next_height: f32,
    remaining_height: f32,
    page_area_height: f32,
    at_page_top: bool,
) -> bool {
    !at_page_top
        && next_height > remaining_height + 0.01
        && run_height + next_height <= page_area_height + 0.01
}

/// Returns whether a definite-height normal-flow block should start a new page.
///
/// CSS Fragmentation allows breaks between sibling block boxes. When a block's
/// used border-box height is definite and it fits in an empty page area but not
/// in the remaining fragmentainer space, laying it out after a class A break
/// keeps its own background, border, and descendants in the next page
/// coordinate space:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks> and
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
pub(in crate::layout) fn should_prebreak_definite_block(
    context: DefiniteBlockBreakContext<'_>,
) -> bool {
    if !context.current_page_has_content || context.at_page_top {
        return false;
    }
    let Some(content_height) = context.definite_content_height else {
        return false;
    };
    let block_height = context.style.margin.top
        + context.vertical_extras
        + content_height.max(0.0)
        + context.style.margin.bottom;
    if context.suppress_for_avoid_retry && block_height <= context.page_area_height + 0.01 {
        return false;
    }
    block_height > context.remaining_height + 0.01
        && block_height <= context.page_area_height + 0.01
}
