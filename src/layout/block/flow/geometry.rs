use super::*;
use crate::layout::block::float::FLOAT_EPSILON;

/// A physical content height that CSS Sizing established as definite.
///
/// The physical axis is retained for block-flow geometry, while the wrapper
/// prevents an auto height from being used as a definite-size capability.
pub(in crate::layout) type DefinitePhysicalContentHeight = Definite<PhysicalContentHeight>;

pub(in crate::layout) fn writing_modes_are_orthogonal(a: WritingMode, b: WritingMode) -> bool {
    WritingModeAxes::new(a, Direction::Ltr).swaps_physical_axes()
        != WritingModeAxes::new(b, Direction::Ltr).swaps_physical_axes()
}

/// Build the child available space exported by a formatting-context root.
///
/// A physical-height fallback used to fit an orthogonal descendant is not a
/// CSS percentage basis. The nearest scroll container terminates the ancestor
/// lookup: it either contributes its constrained used height or causes the
/// initial containing block to be used. This policy applies at every
/// formatting-context boundary, not only ordinary block flow.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
pub(in crate::layout) fn child_available_space_for_formatting_context(
    style: &ComputedStyle,
    content_width: PhysicalContentWidth,
    definite_content_height: Option<DefinitePhysicalContentHeight>,
    inherited_orthogonal_available_height: OrthogonalAvailableHeight,
    initial_fallback_height: PhysicalContentHeight,
) -> ChildAvailableSpace {
    let local_orthogonal_constraint = orthogonal_fallback_physical_content_height(
        style,
        PercentageBasis::definite(content_width.content_box_length()),
    );
    let direct_orthogonal_constraint = direct_orthogonal_available_height(
        style,
        PercentageBasis::definite(content_width.content_box_length()),
    );
    let is_scroll_container = style_clips_overflow(style);
    let mut space = ChildAvailableSpace::new(
        style.writing_mode,
        content_width,
        !style.writing_mode.has_vertical_lines() || !style.box_values.width.is_auto(),
        definite_content_height.map(DefinitePhysicalContentHeight::value),
        inherited_orthogonal_available_height.value(),
    )
    // Preserve the tagged nearest-scroll-container policy through a
    // non-scrolling formatting context. Reconstructing from only the numeric
    // fallback would silently turn it into an ICB policy, allowing a nested
    // scroll container to discard the actual nearest scroller.
    .with_orthogonal_available_height(inherited_orthogonal_available_height)
    // A non-scrolling constrained block is an available-size source only for
    // its direct orthogonal child. Keeping it separate from the inherited
    // nearest-scroll-container policy prevents it from leaking through an
    // intermediate same-writing-mode formatting context.
    .with_direct_orthogonal_available_height(
        (!is_scroll_container)
            .then_some(direct_orthogonal_constraint)
            .flatten()
            // An immediate non-scrolling constraint can select an orthogonal
            // child's line-fitting measure, but it cannot enlarge that measure
            // beyond the initial containing block. The ICB remains the fallback
            // ceiling when the direct `height`/`min-height`/`max-height` is
            // taller; it is not a percentage-height basis.
            // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>
            .and_then(|height| height.capped_by_initial_containing_block(initial_fallback_height)),
    );
    if is_scroll_container {
        let initial = initial_fallback_height.points().max(0.0);
        // A minimum alone only constrains the scroll container's eventual
        // auto used height; it does not provide the definite/max constraint
        // used to choose an orthogonal child's line-fitting measure. A
        // definite `height` does, and a `max-height` selects the same measure
        // floored by `min-height` through `local_orthogonal_constraint`.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let has_max_height = used_max_height(
            style,
            PercentageBasis::definite(content_width.content_box_length()),
        )
        .is_some();
        let constrained_height = definite_content_height
            .map(DefinitePhysicalContentHeight::value)
            .or(has_max_height
                .then_some(local_orthogonal_constraint)
                .flatten())
            .map(PhysicalContentHeight::points);
        // A scroll container without a usable height/max-height/min-height
        // constraint is still the nearest scroller and stops an outer
        // scroll-container's fallback from leaking through.
        let fallback = constrained_height.map_or_else(
            || OrthogonalAvailableHeight::initial_containing_block(initial_fallback_height),
            |height| {
                OrthogonalAvailableHeight::nearest_scroll_container(PhysicalContentHeight::new(
                    content_box_pt(height.min(initial).max(0.0)),
                ))
            },
        );
        space = space.with_orthogonal_available_height(fallback);
    }
    space
}

/// Backwards-compatible name for normal block-flow callers.
pub(in crate::layout) fn child_available_space_for_block(
    style: &ComputedStyle,
    content_width: PhysicalContentWidth,
    definite_content_height: Option<DefinitePhysicalContentHeight>,
    inherited_orthogonal_available_height: OrthogonalAvailableHeight,
    initial_fallback_height: PhysicalContentHeight,
) -> ChildAvailableSpace {
    child_available_space_for_formatting_context(
        style,
        content_width,
        definite_content_height,
        inherited_orthogonal_available_height,
        initial_fallback_height,
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
    percentage_basis: PercentageBasis<ContentBoxLength>,
) -> Option<PhysicalContentHeight> {
    let min_height = used_min_height(style, percentage_basis).map(SemanticLengthExt::points);
    let max_height = used_max_height(style, percentage_basis).map(SemanticLengthExt::points);
    let height = match (min_height, max_height) {
        (Some(min_height), Some(max_height)) => max_height.max(min_height),
        (Some(min_height), None) => min_height,
        (None, Some(max_height)) => max_height,
        (None, None) => return None,
    };
    Some(PhysicalContentHeight::new(content_box_pt(height)))
}

/// Preserve whether a non-scrolling direct constraint comes from a larger
/// minimum floor rather than an ordinary maximum constraint. The two select
/// the same final line measure, but only the former also fixes the wrapped
/// physical-width contribution of an auto-sized vertical child.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
fn direct_orthogonal_available_height(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<ContentBoxLength>,
) -> Option<DirectOrthogonalAvailableHeight> {
    if let Some(height) = style.box_values.height.length_if_no_percent() {
        return Some(DirectOrthogonalAvailableHeight::Definite(
            PhysicalContentHeight::new(content_box_pt(height.max(0.0))),
        ));
    }
    let min_height = used_min_height(style, percentage_basis).map(SemanticLengthExt::points);
    let max_height = used_max_height(style, percentage_basis).map(SemanticLengthExt::points);
    let height = orthogonal_fallback_physical_content_height(style, percentage_basis)?;
    if min_height.is_some_and(|minimum| max_height.is_none_or(|maximum| minimum > maximum)) {
        Some(DirectOrthogonalAvailableHeight::MinimumFloor(height))
    } else {
        Some(DirectOrthogonalAvailableHeight::Maximum(height))
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
    content_inline_span: PageInlineSpan,
    subject_bounds: Option<PaintClip>,
) -> f32 {
    if !vertical_block_align_content_needs_fragment_bounds(style) {
        return 0.0;
    }
    let Some(subject_bounds) = subject_bounds else {
        return 0.0;
    };
    let subject_width = subject_bounds.width().max(0.0);
    let content_width = content_inline_span.width().max(0.0);
    let free_space = content_width - subject_width;
    let toward_block_end = content_alignment_offset_toward_end(
        style.align_content,
        free_space,
        block_align_content_defaults_to_safe_overflow(style),
    );
    match block_start_side(style.writing_mode) {
        PhysicalSide::Left => content_inline_span.left_x() + toward_block_end - subject_bounds.x(),
        PhysicalSide::Right => {
            content_inline_span.right_x() - toward_block_end - (subject_bounds.x() + subject_width)
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
        || style.text_decoration_origins.has_effective_layers()
        || !matches!(style.text_emphasis_style, css::TextEmphasisStyle::None)
}

/// Return whether a last child's bottom margin can stay collapsed through the parent.
///
/// CSS 2.2 lets an in-flow last child's bottom margin adjoin its parent's bottom
/// margin when the parent has auto height and no separating border/padding/line
/// boxes. A used min/max height only permits that collapse when it leaves the
/// parent's used content height unchanged. Block layout therefore compares the
/// constrained height against the content height with the candidate child
/// margin removed. If the collapse is blocked, the child margin still must not
/// inflate the parent's constrained used height:
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins> and
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn block_end_margin_collapse_survives_height_constraints(
    style: &ComputedStyle,
    content_width: PhysicalContentWidth,
    vertical_non_content: NonContentLength,
    content_height_without_child_margin: PhysicalContentHeight,
) -> bool {
    let requested_content_height = used_content_box_height_or_auto(
        style,
        layout_pt(content_height_without_child_margin.points()),
        vertical_non_content,
    )
    .map(SemanticLengthExt::points)
    .unwrap_or_else(|| content_height_without_child_margin.points());
    let height_depends_on_intrinsic_content =
        needs_intrinsic_height_contribution(style.box_values.height.value().clone())
            || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
            || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
    let constrained_height = if height_depends_on_intrinsic_content {
        constrain_height_with_intrinsic(
            style,
            content_box_pt(requested_content_height),
            content_height_without_child_margin.content_box_length(),
            content_height_without_child_margin.content_box_length(),
            PercentageBasis::definite(content_width.content_box_length()),
            vertical_non_content,
        )
        .points()
    } else {
        constrain_content_height(
            style,
            content_box_pt(requested_content_height),
            PercentageBasis::definite(content_width.content_box_length()),
        )
        .points()
    };
    (constrained_height - content_height_without_child_margin.points()).abs() <= 0.01
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
    /// Used style for this laid-out box. Its computed source remains in the
    /// frozen formatting tree and is the only value eligible for cascading.
    pub(in crate::layout) style: css::ZoomedLayoutStyle,
    pub(in crate::layout) relative_offset: RelativeOffset,
    pub(in crate::layout) border_edges: UsedEdges,
    pub(in crate::layout) vertical_non_content: NonContentLength,
    pub(in crate::layout) containing_block_content_height: BlockSizePercentageBasis,
    /// Definite physical content height exported for descendant percentage
    /// resolution. This remains physical even for a vertical block, whose
    /// logical inline size is the same axis.
    pub(in crate::layout) definite_content_height: Option<DefinitePhysicalContentHeight>,
    pub(in crate::layout) content_logical_inline_size: LogicalInlineContentSize,
    /// A replay-safe vertical inline sequence selected while resolving this
    /// orthogonal block's automatic physical width.
    pub(in crate::layout) selected_orthogonal_inline_layout: Option<SelectedOrthogonalInlineLayout>,
    pub(in crate::layout) outer_inline: BlockBorderBoxInlineBounds,
    pub(in crate::layout) content_inline: BlockContentBoxInlineBounds,
}

/// The one selected line layout consumed by a vertical orthogonal block's
/// automatic physical-width sizing and final inline painting.
///
/// CSS Writing Modes selects an orthogonal flow's available inline measure
/// during sizing, then maps the selected logical block stack to physical
/// width. Retaining the selected sequence makes that sizing result and final
/// paint one operation rather than two independently collected inline flows.
/// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
#[derive(Clone)]
pub(in crate::layout) struct SelectedOrthogonalInlineLayout {
    pub(in crate::layout) logical_inline_measure: LogicalInlineContentSize,
    pub(in crate::layout) line_sequence: inline_layout::InlineLineSequence,
    pub(in crate::layout) logical_block_contribution: LogicalBlockContentSize,
    pub(in crate::layout) frozen_replay_input: inline_collect::FrozenInlineReplayInput,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockLayoutInlineConstraint {
    pub(in crate::layout) containing_inline_span: PageInlineSpan,
    /// Containing-block logical inline size used by margin and padding
    /// percentages.
    /// The containing block's logical inline percentage basis. This belongs
    /// to the containing flow, not the child's writing mode: an orthogonal
    /// child's percentage margins still resolve against its containing
    /// block's inline size.
    pub(in crate::layout) percentage_basis: LogicalInlinePercentageBasis,
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

/// A one-child override exported by a principal vertical flow.
///
/// The legacy block traversal keeps its current child constraint in physical
/// page coordinates.  A propagated vertical principal flow instead supplies
/// a logical-inline percentage basis and a horizontal logical-block track to
/// exactly its direct child.  Keeping the source element with the constraint
/// makes the scope explicit: descendants establish their normal containing
/// block contexts and must not inherit this projection.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct DirectBlockLayoutConstraint {
    element: ElementId,
    inline: BlockLayoutInlineConstraint,
}

impl DirectBlockLayoutConstraint {
    pub(in crate::layout) fn new(element: ElementId, inline: BlockLayoutInlineConstraint) -> Self {
        Self { element, inline }
    }

    pub(in crate::layout) fn for_element(
        self,
        element: &Element,
    ) -> Option<BlockLayoutInlineConstraint> {
        (self.element == element.id).then_some(self.inline)
    }
}

impl BlockLayoutGeometry {
    pub(in crate::layout) fn outer_inline(&self) -> BlockBorderBoxInlineBounds {
        self.outer_inline
    }

    pub(in crate::layout) fn content_inline(&self) -> BlockContentBoxInlineBounds {
        self.content_inline
    }

    pub(in crate::layout) fn content_logical_inline_size(&self) -> LogicalInlineContentSize {
        self.content_logical_inline_size
    }

    /// Re-anchor this block's border box at the normal-flow span selected by
    /// float avoidance.
    ///
    /// The float-free band is an available-space constraint, not a replacement
    /// containing block. Width and percentage resolution may use that band,
    /// but a fixed-width block's used margin must not be applied again when
    /// the final border-box origin is replayed. The candidate span is measured
    /// before relative positioning, so restore that offset only for the final
    /// normal-flow geometry:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) fn reanchor_float_avoiding_border_box(
        &mut self,
        normal_flow_border_box_span: PageInlineSpan,
    ) {
        let outer_span = PageInlineSpan::new(
            normal_flow_border_box_span.left_x() + self.relative_offset.x(),
            normal_flow_border_box_span.width(),
        );
        self.outer_inline = BlockBorderBoxInlineBounds::new(outer_span);
        self.content_inline = BlockContentBoxInlineBounds::new(PageInlineSpan::new(
            outer_span.left_x() + self.border_edges.left.points() + self.style.padding.left,
            self.content_inline.width().points(),
        ));
    }

    /// Form the normal-flow border-box candidate used to avoid earlier float
    /// margin boxes.
    ///
    /// Relative positioning affects paint, not normal-flow float collision, so
    /// this is the sole conversion that removes the relative inline offset.
    /// A negative physical margin may legally let the corresponding border-box
    /// edge overflow its containing inline span:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) fn float_avoidance_candidate(
        &self,
        border_box_block_size: BorderBoxLength,
    ) -> FloatAvoidanceCandidate {
        let (inline_start_containment, inline_end_containment) = match self.style.direction {
            Direction::Ltr => (
                (self.style.margin.left < -FLOAT_EPSILON)
                    .then_some(FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow),
                (self.style.margin.right < -FLOAT_EPSILON)
                    .then_some(FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow),
            ),
            Direction::Rtl => (
                (self.style.margin.right < -FLOAT_EPSILON)
                    .then_some(FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow),
                (self.style.margin.left < -FLOAT_EPSILON)
                    .then_some(FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow),
            ),
        };
        FloatAvoidanceCandidate {
            normal_flow_border_box_inline_span: PageInlineSpan::new(
                self.outer_inline.span().left_x() - self.relative_offset.x(),
                self.outer_inline.span().width(),
            ),
            normal_flow_border_box_block_size: border_box_block_size,
            inline_start_containment: inline_start_containment
                .unwrap_or(FloatAvoidanceInlineContainment::Required),
            inline_end_containment: inline_end_containment
                .unwrap_or(FloatAvoidanceInlineContainment::Required),
        }
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
        block_top: f32,
        block_height: f32,
    ) -> BlockBorderBox {
        BlockBorderBox::from_rect(BlockRect::new(
            BlockPoint::new(self.outer_inline.span().left_x(), block_top),
            BlockSize::new(self.outer_inline.width().points(), block_height.max(0.0)),
        ))
    }

    /// Return the block padding box as a top-edge page rectangle.
    ///
    /// CSS Positioned Layout uses the padding box of positioned ancestors as
    /// the containing block for absolute descendants:
    /// <https://www.w3.org/TR/css-position-3/#def-cb>.
    pub(in crate::layout) fn padding_box_top_rect(
        &self,
        block_top: f32,
        content_height: f32,
    ) -> PageTopRect {
        PageTopRect::new(
            self.outer_inline.span().left_x() + self.border_edges.left.points(),
            block_top - self.border_edges.top.points(),
            self.content_inline.width().points()
                + self.style.padding.left
                + self.style.padding.right,
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
pub(in crate::layout) struct BlockBorderBoxInlineBounds {
    span: PageInlineSpan,
}

impl BlockBorderBoxInlineBounds {
    pub(in crate::layout) fn new(span: PageInlineSpan) -> Self {
        Self { span }
    }

    pub(in crate::layout) fn span(self) -> PageInlineSpan {
        self.span
    }

    pub(in crate::layout) fn width(self) -> BorderBoxLength {
        border_box_pt(self.span.width())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockContentBoxInlineBounds {
    span: PageInlineSpan,
}

impl BlockContentBoxInlineBounds {
    pub(in crate::layout) fn new(span: PageInlineSpan) -> Self {
        Self { span }
    }

    pub(in crate::layout) fn span(self) -> PageInlineSpan {
        self.span
    }

    pub(in crate::layout) fn width(self) -> ContentBoxLength {
        content_box_pt(self.span.width())
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
    pub(in crate::layout) fn from_rect(rect: BlockRect) -> Self {
        Self { rect }
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
