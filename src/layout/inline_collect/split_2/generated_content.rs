use std::rc::Rc;

use super::*;
use crate::layout::inline_layout::InlineLineStackCursor;
use crate::units::{content_box_to_margin_box_length, glyph_baseline_displacement_pt};

/// Layout-only style for the hypothetical in-flow box used to select an
/// ordinary-flow static-position rectangle.
///
/// This deliberately differs from the actual abspos style: CSS 2.2 asks for
/// the first box the element would generate with `position: static` and
/// `float: none`. Its margins remain normal-flow margins.
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
struct StaticHypotheticalBox {
    style: css::ZoomedLayoutStyle,
}

/// Logical content geometry for a hypothetical inline static-position box.
///
/// An inline placeholder participates in line selection before its positioned
/// source is laid out. Its advance must therefore remain distinct from its
/// line-box block extent until the writing-mode projection that constructs the
/// physical inline atom:
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy)]
struct StaticInlinePlaceholderLogicalGeometry {
    inline_advance: LogicalInlineContentSize,
    block_extent: LogicalBlockContentSize,
}

impl StaticInlinePlaceholderLogicalGeometry {
    /// Project the logical content geometry and physical box-model edges into
    /// the inline-layout backend's physical atom size.
    fn margin_box_inline_size(self, style: &ComputedStyle) -> InlineSize {
        let horizontal_non_content = style.padding.left
            + style.padding.right
            + horizontal_border_width(style)
            + style.margin.left
            + style.margin.right;
        let vertical_non_content = style.padding.top
            + style.padding.bottom
            + vertical_border_width(style)
            + style.margin.top
            + style.margin.bottom;
        if style.writing_mode.has_vertical_lines() {
            InlineSize::new(
                self.block_extent.points() + horizontal_non_content,
                self.inline_advance.points() + vertical_non_content,
            )
        } else {
            InlineSize::new(
                self.inline_advance.points() + horizontal_non_content,
                self.block_extent.points() + vertical_non_content,
            )
        }
    }
}

/// Physical vertical edges of an inline atom after crossing from inline paint
/// coordinates to page-top static-position coordinates.
///
/// `PhysicalInlineRect` stores its `y` origin at the physical bottom edge,
/// whereas `PageTopRect` stores its `top_y` at the physical top edge. Keep
/// that convention change named so a vertical logical-side selection cannot
/// accidentally use the opposite edge.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy)]
struct StaticInlinePlaceholderPageEdges {
    top_y: f32,
    bottom_y: f32,
}

/// A block-level source's hypothetical normal-flow margin box, retained until
/// its static-position rectangle has selected the source's block edge.
///
/// Block static rectangles span their containing block in the logical inline
/// axis, so [`StaticPositionContainingBlock::rectangle_at_hypothetical_block_box`]
/// intentionally discards the hypothetical box's inline coordinate. A
/// relatively positioned inline ancestor still translates that full span in
/// its inline axis, however. Keep the edge selection and that one remaining
/// projection together so the block-axis offset is neither lost nor applied
/// twice.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
#[derive(Debug, Clone, Copy)]
struct HypotheticalBlockMarginBox {
    area: PageTopRect,
    relative_ancestor_offset: InlineVisualOffset,
}

impl HypotheticalBlockMarginBox {
    fn from_placeholder(
        placeholder: PageTopRect,
        relative_ancestor_offset: InlineVisualOffset,
    ) -> Self {
        Self {
            area: PageTopRect::new(
                placeholder.x() + relative_ancestor_offset.x(),
                placeholder.top_y() + relative_ancestor_offset.y(),
                placeholder.width(),
                placeholder.height(),
            ),
            relative_ancestor_offset,
        }
    }

    fn static_rectangle(
        self,
        containing_block: StaticPositionContainingBlock,
    ) -> StaticPositionRectangle {
        let mut rectangle = containing_block.rectangle_at_hypothetical_block_box(self.area);
        rectangle.area = self.translate_inline_span(
            rectangle.area,
            containing_block.axes.physical_axis(LogicalAxis::Inline),
        );
        rectangle
    }

    fn translate_inline_span(self, area: PageTopRect, inline_axis: PhysicalAxis) -> PageTopRect {
        match inline_axis {
            PhysicalAxis::Horizontal => PageTopRect::new(
                area.x() + self.relative_ancestor_offset.x(),
                area.top_y(),
                area.width(),
                area.height(),
            ),
            PhysicalAxis::Vertical => PageTopRect::new(
                area.x(),
                area.top_y() + self.relative_ancestor_offset.y(),
                area.width(),
                area.height(),
            ),
        }
    }
}

impl StaticInlinePlaceholderPageEdges {
    fn from_inline_paint_rect(rect: PhysicalInlineRect) -> Self {
        Self {
            top_y: rect.y() + rect.height(),
            bottom_y: rect.y(),
        }
    }

    fn logical_inline_start_y(self, writing_mode: WritingMode, direction: Direction) -> f32 {
        match inline_start_side(writing_mode, direction) {
            PhysicalSide::Top => self.top_y,
            PhysicalSide::Bottom => self.bottom_y,
            PhysicalSide::Left | PhysicalSide::Right => {
                unreachable!("a vertical inline axis must start at the physical top or bottom edge")
            }
        }
    }
}

impl StaticHypotheticalBox {
    fn from_positioned(style: css::ZoomedLayoutStyle) -> Self {
        let mut style = style;
        style.position = Position::Static;
        style.float = Float::None;
        style.clear = Clear::None;
        // The computed display of an absolutely positioned non-atomic inline
        // has been blockified. Reconstitute the hypothetical static display
        // before it enters the line builder. Atomic inline sources preserve
        // their inner display type for the same hypothetical formatting.
        if matches!(
            style.abspos_static_source,
            css::StaticPositionSource::Inline
        ) {
            style.display = css::Display::INLINE;
        } else if let Some(display) = style.abspos_static_source.atomic_inline_display() {
            style.display = display;
        }
        Self { style }
    }
}

/// An explicitly inset positioned descendant held until its positioned inline
/// ancestor has emitted the final edge that completes its containing block.
///
/// The request owns only DOM and computed-style inputs. Its frozen child boxes
/// are rebuilt when the scope closes, avoiding a borrowed formatting-tree
/// lifetime in the inline collection state.
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
#[derive(Clone)]
struct DeferredInlinePositionedDescendant {
    element: Element,
    style: ComputedStyle,
    /// The inline source whose hypothetical-flow geometry defines the
    /// static-position rectangle. This is distinct from the enclosing block
    /// formatting context used to lay out the descendant itself.
    static_position_container_style: ComputedStyle,
    containing_block_source: InlinePositioningContainingBlockSource,
}

/// An inline-level positioned descendant whose static-position rectangle
/// cannot be selected until the enclosing source stream has supplied the
/// complete line.  The record keeps the DOM/style boundary immutable while
/// delaying only the geometry-dependent positioned layout.
///
/// The marker index is a source-order boundary, rather than an atom pointer:
/// line breaking may copy, split, or bidi-reorder the collected items before
/// the hypothetical placeholder is measured.
/// <https://drafts.csswg.org/css-position-3/#static-position>
#[derive(Clone)]
struct DeferredInlineStaticPositionedDescendant {
    element: Element,
    style: ComputedStyle,
    /// The block formatting context that selected the hypothetical line.
    /// This is deliberately distinct from the lexical inline ancestor: the
    /// latter may reset `text-indent`, direction, or writing mode without
    /// changing the line box that defines an inline static-position
    /// rectangle.
    line_formatting_context_style: ComputedStyle,
    static_position_container_style: ComputedStyle,
    /// The block formatting context that owns the hypothetical source. It is
    /// captured at source order because deferred replay may occur after an
    /// anonymous-inline split has changed the active builder context.
    static_position_containing_block: Option<StaticPositionContainingBlock>,
    /// The nearest positioned inline establishes the actual absolute
    /// containing block. It is independent of the static-position
    /// containing block retained above, and therefore must survive deferred
    /// hypothetical-line replay even when every inset is `auto`.
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    positioning_containing_block_source: Option<InlinePositioningContainingBlockSource>,
    /// Relative offsets do not affect normal-flow line fitting, but they do
    /// affect the hypothetical box's final page position.
    hypothetical_ancestor_offset: InlineVisualOffset,
    content: DeferredStaticPositionedContent,
    static_position_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenInlineReplayEligibility {
    Eligible,
    HasInlineFlowEffects,
    EstablishesScrollContainer,
}

/// A side-effect-free frozen inline source that can be selected while sizing
/// an orthogonal block and committed after its final physical geometry is
/// known.
///
/// `eligibility` records why the candidate may not be replayed. In
/// particular, a scroll container has clipping and fragmentation behavior
/// beyond a simple line stack, so it stays on the normal final-layout path.
/// The item stream is assembled by the same frozen-box collector as final
/// inline layout. Positioned descendants are retained as static-position
/// recipes: they are out of flow and therefore cannot affect the selected
/// line stack, but must be materialized against the final containing box.
/// <https://drafts.csswg.org/css-overflow-3/#scroll-containers>
/// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
#[derive(Clone)]
pub(in crate::layout) struct FrozenInlineReplayInput {
    items: Vec<InlineItem>,
    deferred_static_positioned_descendants: Vec<DeferredInlineStaticPositionedDescendant>,
    eligibility: FrozenInlineReplayEligibility,
}

impl FrozenInlineReplayInput {
    pub(in crate::layout) fn selection_items(&self) -> Vec<InlineItem> {
        self.items.clone()
    }

    pub(in crate::layout) fn is_replay_safe(&self) -> bool {
        self.eligibility == FrozenInlineReplayEligibility::Eligible
    }
}

#[derive(Clone, Copy)]
enum DeferredStaticPositionedContent {
    Dom,
    Frozen,
}

/// The generated fragment that owns one source-order edge of a positioned
/// inline's padding-box containing block.
///
/// CSS 2.2 selects the first and last generated inline boxes, rather than the
/// union of every painted fragment. Keeping the edge role with its prepared
/// line geometry prevents visual/bidi order from becoming an accidental
/// containing-block rule.
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
#[derive(Debug, Clone, Copy)]
struct InlinePositioningFragmentEdgeCapture {
    logical_edge: InlineLogicalEdge,
    rect: PageTopRect,
}

/// The source-order fragment edges that form an inline absolute-positioning
/// containing block.
///
/// CSS Positioned Layout selects logical start edges from the first fragment
/// and logical end edges from the end-most fragment. A physical bounding union
/// is wrong for vertical writing modes (and bidi fragments), because it can
/// take both sides of either source fragment instead of the required logical
/// corner. Keep the fragment roles and their axes together until this named
/// geometry conversion.
/// <https://drafts.csswg.org/css-position-3/#def-cb>
#[derive(Debug, Clone, Copy)]
struct InlineContainingBlockContentEdges {
    first_fragment: PageTopRect,
    end_fragment: PageTopRect,
    axes: WritingModeAxes,
}

impl InlineContainingBlockContentEdges {
    /// Form the positioned containing block from the first fragment's logical
    /// start edges and the end fragment's logical end edges.
    ///
    /// This is the only adapter that projects those logical edges into a
    /// physical `PageTopRect`; callers retain a `ContainingBlock` rather than
    /// recombining page coordinates themselves.
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    fn to_containing_block(self) -> ContainingBlock {
        let (horizontal_start, horizontal_end) = self.physical_axis_edges(PhysicalAxis::Horizontal);
        let (vertical_start, vertical_end) = self.physical_axis_edges(PhysicalAxis::Vertical);

        let first_x = Self::coordinate_on_side(self.first_fragment, horizontal_start);
        let end_x = Self::coordinate_on_side(self.end_fragment, horizontal_end);
        let first_y = Self::coordinate_on_side(self.first_fragment, vertical_start);
        let end_y = Self::coordinate_on_side(self.end_fragment, vertical_end);

        let left = first_x.min(end_x);
        let right = first_x.max(end_x);
        let bottom = first_y.min(end_y);
        let top = first_y.max(end_y);
        ContainingBlock::from_page_top_rect(PageTopRect::new(left, top, right - left, top - bottom))
    }

    fn physical_axis_edges(self, axis: PhysicalAxis) -> (PhysicalSide, PhysicalSide) {
        let logical_axis = if self.axes.physical_axis(LogicalAxis::Inline) == axis {
            LogicalAxis::Inline
        } else {
            debug_assert_eq!(self.axes.physical_axis(LogicalAxis::Block), axis);
            LogicalAxis::Block
        };
        let (start, end) = match logical_axis {
            LogicalAxis::Inline => (LogicalSide::InlineStart, LogicalSide::InlineEnd),
            LogicalAxis::Block => (LogicalSide::BlockStart, LogicalSide::BlockEnd),
        };
        (self.axes.physical_side(start), self.axes.physical_side(end))
    }

    fn coordinate_on_side(rect: PageTopRect, side: PhysicalSide) -> f32 {
        match side {
            PhysicalSide::Left => rect.x(),
            PhysicalSide::Right => rect.x() + rect.width(),
            PhysicalSide::Top => rect.top_y(),
            PhysicalSide::Bottom => rect.bottom_y(),
        }
    }
}

/// Paint produced while resolving a positioned descendant whose lexical inline
/// containing block has not yet been selected for paint.
///
/// The source id is an explicit anchor in the collected inline stream.  The
/// effect is attached to that start edge, rather than the builder's global
/// positioned-layer list, so line-clamp selection commits it exactly when the
/// source edge is replayed.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
enum DeferredClampEffect {
    PositionedLayers {
        owner: InlinePositioningContainingBlockId,
        layers: Vec<PositionedPaintLayer>,
    },
}

impl DeferredClampEffect {
    fn attach_to_owner(self, output: &mut [InlineItem]) {
        let Self::PositionedLayers { owner, layers } = self;
        if layers.is_empty() {
            return;
        }
        let owner_start = output.iter_mut().find_map(|item| {
            let InlineItem::Atom(atom) = item else {
                return None;
            };
            let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
            else {
                return None;
            };
            (edge.logical_edge == InlineLogicalEdge::Start
                && edge.positioning_containing_block_id == Some(owner))
            .then_some(atom)
        });
        if let Some(owner_start) = owner_start {
            owner_start.append_escaped_positioned_layers(layers);
        } else {
            debug_assert!(
                false,
                "positioned inline effect must have its source start edge"
            );
        }
    }
}

fn positioned_descendant_has_explicit_inset(style: &ComputedStyle) -> bool {
    [
        &style.box_values.inset_top,
        &style.box_values.inset_right,
        &style.box_values.inset_bottom,
        &style.box_values.inset_left,
    ]
    .into_iter()
    .any(|inset| !matches!(inset, css::ComputedLengthPercentageOrAuto::Auto))
}

/// Mark generated text belonging to one footnote call without assigning the
/// footnote to a page yet. The marker is retained through graph selection and
/// consumed only when the owning line is committed.
fn mark_inline_items_as_footnote_call(items: &mut [InlineItem], element: ElementId) {
    for item in items {
        if let InlineItem::Word(word) = item {
            word.source = InlineTextSource::FootnoteCall(element);
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Captures the static-position containing block while the hypothetical
    /// source is still in normal flow. A deferred inline replay may run after
    /// an anonymous split has restored a wider ancestor context, but CSS
    /// Position keeps the hypothetical box's containing block unchanged.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    fn current_static_position_containing_block(&self) -> Option<StaticPositionContainingBlock> {
        self.static_position_containing_blocks.last().copied()
    }

    pub(in crate::layout) fn push_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.push_element_content_items_from_dom_with_positioned_descendants(
            element,
            style,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_element_content_items_from_dom_with_positioned_descendants(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        line_formatting_context_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<
            BorrowedInlinePositioningContainingBlockSource<'_>,
        >,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_items_with_positioned_descendants(
                        element,
                        style,
                        line_formatting_context_style,
                        stylesheets,
                        inherited_link.clone(),
                        placement,
                        active_positioning_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
                        deferred_static_positioned_descendants.as_deref_mut(),
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                box_tree::CounterEventSource::Principal,
                inherited_link.clone(),
                placement.baseline_shift(),
                placement.visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        source: box_tree::CounterEventSource,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        propagated_decoration_layers.clone(),
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                source,
                inherited_link.clone(),
                baseline_shift,
                visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    pub(in crate::layout) fn collect_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.collect_inline_items_with_positioned_descendants(
            element,
            style,
            style,
            stylesheets,
            inherited_link,
            placement,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    /// Collects inline DOM content, deferring explicit-inset descendants of a
    /// positioned inline until its final source edge is available.
    ///
    /// CSS 2.2 derives a positioned inline's containing block from its first
    /// and last line boxes, so eager layout at the descendant's DOM position
    /// observes an incomplete rectangle.
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
    #[allow(clippy::too_many_arguments)]
    fn collect_inline_items_with_positioned_descendants(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        line_formatting_context_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        active_positioning_containing_block: Option<
            BorrowedInlinePositioningContainingBlockSource<'_>,
        >,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let mut stack_profile_scope =
            stack_profile::enter("collect_inline_items_with_positioned_descendants");
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for child in &element.children {
            #[cfg(all(feature = "stack-profile", target_os = "macos"))]
            stack_profile_scope.set_source_index(element_index);
            match &child.kind {
                NodeKind::Text(text) => {
                    if element_suppresses_direct_text_children(element) {
                        continue;
                    }
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        placement.baseline_shift(),
                        placement.visual_offset,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    if is_html_select_item_element(child_element)
                        && !has_html_select_context(element, &self.ancestors)
                    {
                        continue;
                    }
                    let child_signature = ElementSignature::from_sibling_snapshot(
                        element_index,
                        sibling_tags.clone(),
                    )
                    .expect("source child must have a cached sibling signature");
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(style),
                    );
                    // CSS Display Appendix B makes `display: contents`
                    // compute to `none` for HTML line-break controls. Check
                    // the computed display before recognizing `<br>`'s HTML
                    // forced-break behavior so a suppressed break contributes
                    // neither a line boundary nor a placeholder.
                    // <https://drafts.csswg.org/css-display-3/#unbox-html>
                    if child_style.display.is_none() {
                        continue;
                    }
                    // The box-tree builder retargets a suppressed source
                    // event to the nearest visible source boundary. This
                    // direct-DOM path bypasses frozen formatting boxes, so it
                    // must consume that boundary here just as the frozen-box
                    // collector does.
                    // <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>
                    self.capture_suppressed_named_strings_before(child_element.id);
                    if is_line_break_element(child_element) {
                        // `<br>` is an HTML forced-break boundary. Its
                        // generated UA `::before` content supplies the
                        // rendering fallback, but cannot remove the semantic
                        // line break or its static-position placeholder.
                        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                        output.push(InlineItem::Break(InlineBreak {
                            clear: child_style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        continue;
                    }
                    if child_style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            child_element.clone(),
                            child_signature,
                            child_style,
                            false,
                            active_positioning_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                                .or_else(|| {
                                    inline_style_establishes_positioning_containing_block(style)
                                        .then(|| InlinePositioningContainingBlockSource {
                                            id: InlinePositioningContainingBlockId(output.len()),
                                            style: Box::new(
                                                self.style_with_current_used_lengths(style),
                                            ),
                                        })
                                }),
                        ))));
                        continue;
                    }
                    // CSS Ruby inlinifies a direct in-flow block child before
                    // it reaches the generic DOM inline collector. The
                    // frozen box-tree normalizer performs the equivalent
                    // transformation, but this source-DOM path must retain
                    // the same atomic flow-root boundary for painting and
                    // sizing.
                    // <https://drafts.csswg.org/css-ruby-1/#anon-gen-inlinize>
                    let ruby_inlinifies_direct_block = (style.display.is_ruby()
                        || style.display.is_ruby_internal())
                        && !matches!(child_style.position, Position::Absolute | Position::Fixed)
                        && child_style.display.is_block_level()
                        && child_style.display.is_flow();
                    if ruby_inlinifies_direct_block {
                        child_style.display =
                            Display::new(DisplayOuter::Inline, DisplayInner::FlowRoot);
                    }
                    // This DOM collector owns inline-formatting content.
                    // Normal-flow block-level source boxes are represented
                    // by frozen formatting boxes. Inline-level tables instead
                    // establish an atomic inline formatting context and must
                    // remain in this collector. An absolutely positioned
                    // descendant instead participates in the inline
                    // ancestor's static-position algorithm regardless of its
                    // outer display type.
                    // <https://www.w3.org/TR/css-position-3/#abspos-layout>
                    // <https://www.w3.org/TR/css-display-3/#inlinification>
                    let participates_in_inline_collection = !child_style.display.is_none()
                        && (matches!(child_style.position, Position::Absolute | Position::Fixed)
                            || !child_style.display.is_block_level());
                    if participates_in_inline_collection
                        && matches!(child_style.position, Position::Absolute | Position::Fixed)
                    {
                        if positioned_descendant_has_explicit_inset(&child_style)
                            && let Some(containing_block_source) =
                                active_positioning_containing_block
                            && let Some(deferred) = deferred_positioned_descendants.as_deref_mut()
                        {
                            deferred.push(DeferredInlinePositionedDescendant {
                                element: child_element.clone(),
                                style: child_style,
                                static_position_container_style: style.clone(),
                                containing_block_source: containing_block_source.into_owned(),
                            });
                            continue;
                        }
                        // A non-atomic inline abspos source owns a static
                        // rectangle spanning the *selected* hypothetical
                        // line.  That line can include following siblings,
                        // so measuring it at this source position would use
                        // an incomplete line strut.  Keep its source boundary
                        // and replay after this inline scope closes.
                        // <https://drafts.csswg.org/css-position-3/#static-position>
                        if !(child_style.abspos_static_source.is_atomic_inline()
                            || child_style.display.is_atomic_inline())
                            && let Some(deferred) =
                                deferred_static_positioned_descendants.as_deref_mut()
                        {
                            deferred.push(DeferredInlineStaticPositionedDescendant {
                                element: child_element.clone(),
                                style: child_style,
                                line_formatting_context_style: line_formatting_context_style
                                    .clone(),
                                static_position_container_style: style.clone(),
                                static_position_containing_block: self
                                    .current_static_position_containing_block(),
                                positioning_containing_block_source:
                                    active_positioning_containing_block.map(
                                        BorrowedInlinePositioningContainingBlockSource::into_owned,
                                    ),
                                hypothetical_ancestor_offset: placement.visual_offset,
                                content: DeferredStaticPositionedContent::Dom,
                                static_position_index: output.len(),
                            });
                            continue;
                        }
                        let positioning_containing_block_source =
                            inline_style_establishes_positioning_containing_block(style).then(
                                || InlinePositioningContainingBlockSource {
                                    id: InlinePositioningContainingBlockId(output.len()),
                                    style: Box::new(self.style_with_current_used_lengths(style)),
                                },
                            );
                        self.layout_positioned_inline_descendant(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            None,
                            if child_style.abspos_static_source.is_inline_level()
                                || child_style.display.is_inline_level()
                            {
                                line_formatting_context_style
                            } else {
                                style
                            },
                            style,
                            positioning_containing_block_source
                                .as_ref()
                                .map(InlinePositioningContainingBlockSource::as_borrowed),
                            None,
                            None,
                            placement.visual_offset,
                            output,
                        );
                        continue;
                    }
                    if !participates_in_inline_collection {
                        continue;
                    }
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_is_atomic_inline = child_style.display.is_atomic_inline()
                        || (is_replaced_element(child_element)
                            && child_style.display.is_inline_level());
                    let child_placement = placement
                        // Atomic inline boxes resolve their own baseline from
                        // their margin box below. Applying the generic inline
                        // box shift here as well would align the same
                        // `vertical-align` value twice.
                        // <https://www.w3.org/TR/css-inline-3/#atomic-inline>
                        .with_added_baseline_placement(if !child_is_atomic_inline {
                            self.vertical_align_baseline_shift_for_inline_style(&child_style, style)
                        } else {
                            InlineBaselinePlacement::from_inherited_glyph_displacement(
                                glyph_baseline_displacement_pt(0.0),
                            )
                        })
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&child_style),
                        );
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &style.text_decoration_origins.effective_layers_vec(),
                        &child_style,
                    );
                    apply_propagated_decoration_layers(&mut child_style, &decoration_layers);
                    // The inline collector recurses through the source DOM
                    // rather than frozen formatting boxes. Keep the current
                    // child in the selector ancestry for all of its
                    // descendants so child combinators remain direct-child
                    // combinators instead of degrading to descendant matches.
                    // <https://drafts.csswg.org/selectors-4/#child-combinators>
                    self.with_ancestor_signature(child_signature.clone(), |layout| {
                        // Atomic inline boxes establish an independent formatting
                        // context even when the parent block is using the DOM
                        // inline collector. Rebuild their frozen child stream at
                        // this boundary so their descendants, own decorations,
                        // and exported baseline remain one atomic line item.
                        // Flattening a nonempty inline-block into an inline scope
                        // loses that boundary and drops its captured paint.
                        // <https://www.w3.org/TR/css-display-3/#atomic-inline>
                        // <https://www.w3.org/TR/css-inline-3/#inline-boxes>
                        if child_is_atomic_inline {
                            let child_boxes = layout
                                .build_frozen_child_boxes_with_current_ancestors(
                                    child_element,
                                    stylesheets,
                                    &child_style,
                                );
                            let built_table_fragment;
                            let table_fragment = if child_style.display.is_table() {
                                built_table_fragment = box_tree::build_frozen_table_fragment(
                                    child_element,
                                    &child_signature,
                                    &child_style,
                                    &child_boxes,
                                );
                                Some(&built_table_fragment)
                            } else {
                                None
                            };
                            let counter_scope =
                                layout.begin_counter_scope(child_element, &child_style);
                            let atom = layout.inline_atom_for_element(
                                child_element,
                                &child_signature,
                                &child_style,
                                &child_boxes,
                                table_fragment,
                                stylesheets,
                                child_placement.baseline_shift(),
                                child_placement.visual_offset,
                                link.clone(),
                            );
                            layout.end_counter_scope(counter_scope);
                            if let Some(mut atom) = atom {
                                atom.baseline_shift += layout
                                    .vertical_align_baseline_shift_for_atom(&atom, style)
                                    .glyph_displacement()
                                    .get();
                                output.push(InlineItem::Atom(Box::new(atom)));
                            }
                            return;
                        }
                        let scope = layout.begin_inline_element_scope(
                            child_element,
                            &child_style,
                            link.clone(),
                            child_placement,
                            InlineElementScopeOptions::DOM_PAINT.with_preserved_empty_metrics(
                                empty_inline_scope_has_distinct_metrics(style, &child_style),
                            ),
                            output,
                        );
                        let scope_positioning_containing_block =
                            scope.positioning_containing_block_source();
                        let next_positioning_containing_block =
                            if inline_style_establishes_positioning_containing_block(&child_style) {
                                scope_positioning_containing_block
                            } else {
                                active_positioning_containing_block
                            };
                        let scope_establishes_positioned_containing_block =
                            scope_positioning_containing_block.is_some();
                        let mut scope_deferred_positioned_descendants = Vec::new();
                        let mut scope_deferred_static_positioned_descendants = Vec::new();
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.before_style.as_deref(),
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Commit,
                            output,
                        );
                        if child_style.content.is_generated() {
                            layout.push_element_content_items_from_dom_with_positioned_descendants(
                                child_element,
                                &child_style,
                                line_formatting_context_style,
                                stylesheets,
                                link.clone(),
                                child_placement,
                                next_positioning_containing_block,
                                if scope_establishes_positioned_containing_block {
                                    Some(&mut scope_deferred_positioned_descendants)
                                } else {
                                    deferred_positioned_descendants.as_deref_mut()
                                },
                                Some(&mut scope_deferred_static_positioned_descendants),
                                output,
                            );
                        } else {
                            layout.collect_inline_items_with_positioned_descendants(
                                child_element,
                                &child_style,
                                line_formatting_context_style,
                                stylesheets,
                                link.clone(),
                                child_placement,
                                next_positioning_containing_block,
                                if scope_establishes_positioned_containing_block {
                                    Some(&mut scope_deferred_positioned_descendants)
                                } else {
                                    deferred_positioned_descendants.as_deref_mut()
                                },
                                Some(&mut scope_deferred_static_positioned_descendants),
                                output,
                            );
                        }
                        layout.push_generated_pseudo_items(
                            child_element,
                            &child_style,
                            child_style.after_style.as_deref(),
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            GeneratedPseudoCounterMode::Commit,
                            output,
                        );
                        layout.end_inline_element_scope(scope, &child_style, output);
                        layout.layout_deferred_inline_static_positioned_descendants(
                            scope_deferred_static_positioned_descendants,
                            stylesheets,
                            output,
                        );
                        if scope_establishes_positioned_containing_block {
                            layout.layout_deferred_inline_positioned_descendants(
                                scope_deferred_positioned_descendants,
                                stylesheets,
                                style,
                                output,
                            );
                        }
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) {
        let mut deferred_static_positioned_descendants = Vec::new();
        self.collect_inline_box_items_with_float_containing_block(
            children,
            stylesheets,
            inherited_link,
            baseline_shift,
            visual_offset,
            block_style,
            block_style,
            block_style,
            propagated_decoration_layers,
            None,
            None,
            Some(&mut deferred_static_positioned_descendants),
            output,
        );
        self.layout_deferred_inline_static_positioned_descendants(
            deferred_static_positioned_descendants,
            stylesheets,
            output,
        );
    }

    /// Collect the final inline-box representation without committing its
    /// positioned descendants or other layout side effects.
    ///
    /// This is deliberately not the intrinsic collector: atomic inline
    /// construction, inline edges, whitespace inputs, and bidi controls must
    /// match the stream final line layout would select. The snapshot restores
    /// counter, paint, and positioning state after collection; positioned
    /// descendants are preserved as recipes for the final replay.
    /// <https://drafts.csswg.org/css-inline-3/#line-box>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_frozen_inline_replay_input(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
    ) -> FrozenInlineReplayInput {
        let snapshot = self.snapshot();
        let mut items = Vec::new();
        let mut deferred_static_positioned_descendants = Vec::new();
        self.with_positioned_layout_suppressed(|layout| {
            layout.collect_inline_box_items_with_float_containing_block(
                children,
                stylesheets,
                inherited_link,
                baseline_shift,
                visual_offset,
                block_style,
                block_style,
                block_style,
                propagated_decoration_layers,
                None,
                None,
                Some(&mut deferred_static_positioned_descendants),
                &mut items,
            );
        });
        self.restore(snapshot);
        let eligibility =
            if block_style.overflow_x.is_scrollable() || block_style.overflow_y.is_scrollable() {
                FrozenInlineReplayEligibility::EstablishesScrollContainer
            } else if items
                .iter()
                .all(|item| matches!(item, InlineItem::Word(_) | InlineItem::Atom(_)))
            {
                FrozenInlineReplayEligibility::Eligible
            } else {
                FrozenInlineReplayEligibility::HasInlineFlowEffects
            };
        FrozenInlineReplayInput {
            items,
            deferred_static_positioned_descendants,
            eligibility,
        }
    }

    /// Materialize the out-of-flow portion of a frozen replay only after the
    /// selected orthogonal line stack has established its containing box.
    pub(in crate::layout) fn layout_frozen_inline_replay_positioned_descendants(
        &mut self,
        input: &FrozenInlineReplayInput,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_deferred_inline_static_positioned_descendants(
            input.deferred_static_positioned_descendants.clone(),
            stylesheets,
            &input.items,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_inline_box_items_with_float_containing_block(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        parent_baseline_style: &ComputedStyle,
        static_position_container_style: &ComputedStyle,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        active_float_containing_block: Option<BorrowedInlinePositioningContainingBlockSource<'_>>,
        mut deferred_positioned_descendants: Option<&mut Vec<DeferredInlinePositionedDescendant>>,
        mut deferred_static_positioned_descendants: Option<
            &mut Vec<DeferredInlineStaticPositionedDescendant>,
        >,
        output: &mut Vec<InlineItem>,
    ) {
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let mut stack_profile_scope =
            stack_profile::enter("collect_inline_box_items_with_float_containing_block");
        for (_child_index, child) in children.iter().enumerate() {
            #[cfg(all(feature = "stack-profile", target_os = "macos"))]
            stack_profile_scope.set_source_index(_child_index);
            if let Some((element, _, style, child_boxes)) = child.element_parts()
                && matches!(style.position, Position::Absolute | Position::Fixed)
            {
                if positioned_descendant_has_explicit_inset(style)
                    && let Some(containing_block_source) = active_float_containing_block
                    && let Some(deferred) = deferred_positioned_descendants.as_deref_mut()
                {
                    deferred.push(DeferredInlinePositionedDescendant {
                        element: element.clone(),
                        style: style.clone(),
                        static_position_container_style: static_position_container_style.clone(),
                        containing_block_source: containing_block_source.into_owned(),
                    });
                    continue;
                }
                if !(style.abspos_static_source.is_atomic_inline()
                    || style.display.is_atomic_inline())
                    && let Some(deferred) = deferred_static_positioned_descendants.as_deref_mut()
                {
                    deferred.push(DeferredInlineStaticPositionedDescendant {
                        element: element.clone(),
                        style: style.clone(),
                        line_formatting_context_style: block_style.clone(),
                        static_position_container_style: static_position_container_style.clone(),
                        static_position_containing_block: self
                            .current_static_position_containing_block(),
                        positioning_containing_block_source: active_float_containing_block
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        hypothetical_ancestor_offset: visual_offset,
                        content: DeferredStaticPositionedContent::Frozen,
                        static_position_index: output.len(),
                    });
                    continue;
                }
                let table_fragment = match child {
                    box_tree::FormattingBox::AtomicInline(box_) => box_.table_fragment.as_ref(),
                    box_tree::FormattingBox::Table(box_) => Some(&box_.fragment),
                    _ => None,
                };
                self.layout_positioned_inline_descendant(
                    element,
                    style,
                    stylesheets,
                    Some(child_boxes),
                    table_fragment,
                    block_style,
                    static_position_container_style,
                    active_float_containing_block,
                    None,
                    None,
                    visual_offset,
                    output,
                );
                continue;
            }
            if let Some((element, signature, style, _)) = child.element_parts()
                && style.float != Float::None
            {
                output.push(InlineItem::Float(Box::new(InlineFloat::new(
                    element.clone(),
                    signature.clone(),
                    style.clone(),
                    style.content.is_generated(),
                    active_float_containing_block
                        .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                ))));
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                // CSS Pseudo-Elements tree-abiding generated content can
                // generate block-level boxes. Even empty block pseudos create
                // block boundaries in an inline formatting context, such as
                // `dt::before { content: ""; display: block }`.
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                self.collect_inline_box_items_with_float_containing_block(
                    &box_.core.children,
                    stylesheets,
                    inherited_link.clone(),
                    baseline_shift,
                    visual_offset,
                    block_style,
                    parent_baseline_style,
                    static_position_container_style,
                    propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &box_.core.style,
                    ),
                    active_float_containing_block,
                    deferred_positioned_descendants.as_deref_mut(),
                    deferred_static_positioned_descendants.as_deref_mut(),
                    output,
                );
                if formatting_box_has_inline_content(&box_.core.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &text_style,
                    );
                    apply_propagated_decoration_layers(&mut text_style, &decoration_layers);
                    self.push_inline_words(
                        &box_.text,
                        &text_style,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let footnote_call = matches!(
                        &box_.core.source,
                        box_tree::BoxSource::GeneratedPseudo(pseudo)
                            if pseudo.kind == box_tree::GeneratedPseudoKind::FootnoteCall
                    )
                    .then_some(box_.core.element.id);
                    let principal_source =
                        matches!(&box_.core.source, box_tree::BoxSource::Principal);
                    if principal_source {
                        self.capture_suppressed_named_strings_before(box_.core.element.id);
                    }
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    inline_style.suppress_inapplicable_transform();
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &propagated_decoration_layers,
                        &inline_style,
                    );
                    apply_propagated_decoration_layers(&mut inline_style, &decoration_layers);
                    // A ruby text container containing only out-of-flow
                    // descendants has no anonymous annotation box, but its
                    // descendants still need the normal positioned/float
                    // collection path below. Suppressing this source box
                    // outright would lose an abspos descendant and its
                    // static-position containing block.
                    // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
                    // Only defer ruby materialization when a first-letter
                    // pseudo actually needs to select from the base level.
                    // A ruby at the beginning of an ordinary line must still
                    // go through paired base/annotation layout; otherwise
                    // the generic inline recursion flattens its annotations
                    // into parent text.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
                    // <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
                    let ruby_can_own_first_letter = inline_style.display.is_ruby()
                        && block_style.first_letter_style.is_some()
                        && !output.iter().any(inline_item_has_typographic_content);
                    let child_placement = InlinePlacement::new(baseline_shift, visual_offset)
                        .with_added_baseline_placement(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &inline_style,
                                parent_baseline_style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&inline_style),
                        );
                    // A ruby formatting context contributes its bases to the
                    // parent inline stream while annotations are sidecars.
                    // Materialize the normalized in-flow levels as a coupled
                    // ruby atom until the graph gains per-column break nodes;
                    // this keeps annotations out of ordinary parent text,
                    // spacing, and justification. Positioned and floated
                    // descendants stay on the generic scope path below so
                    // their containing-block ownership remains intact.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
                    if inline_style.display.is_ruby() {
                        let out_of_flow_overlay =
                            ruby_has_out_of_flow_descendant(&box_.core.children).then(|| {
                                ruby_out_of_flow_overlay(&box_tree::FormattingBox::Inline(
                                    box_.clone(),
                                ))
                            });
                        if self.collect_normalized_ruby_items(
                            &box_.core.children,
                            &inline_style,
                            stylesheets,
                            box_.core
                                .element
                                .attrs
                                .get("href")
                                .cloned()
                                .or_else(|| inherited_link.clone()),
                            child_placement,
                            block_style,
                            ruby_can_own_first_letter
                                .then_some(block_style.first_letter_style.as_deref())
                                .flatten(),
                            decoration_layers.clone(),
                            output,
                        ) {
                            if let Some(out_of_flow_overlay) = out_of_flow_overlay {
                                self.collect_inline_box_items_with_float_containing_block(
                                    std::slice::from_ref(&out_of_flow_overlay),
                                    stylesheets,
                                    inherited_link.clone(),
                                    child_placement.baseline_shift(),
                                    child_placement.visual_offset,
                                    block_style,
                                    &inline_style,
                                    static_position_container_style,
                                    decoration_layers.clone(),
                                    active_float_containing_block,
                                    deferred_positioned_descendants.as_deref_mut(),
                                    deferred_static_positioned_descendants.as_deref_mut(),
                                    output,
                                );
                            }
                            if principal_source {
                                self.capture_suppressed_named_strings_after(box_.core.element.id);
                            }
                            continue;
                        }
                    }
                    // A principal HTML `br` is a semantic forced line break,
                    // not merely the UA `::before` newline used as its
                    // fallback representation.  Box-tree collection reaches
                    // this path after pseudo generation, so recognize the
                    // principal box directly; author `br::before { content:
                    // none }` must not erase the HTML line boundary.
                    // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                    if principal_source && is_line_break_element(box_.core.element) {
                        output.push(InlineItem::Break(InlineBreak {
                            clear: inline_style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let contents_generated_pseudo = inline_style.display.is_contents()
                        && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_));
                    // `display: contents` retains generated content but
                    // suppresses the pseudo-element's principal box. Do not
                    // open an inline paint/positioning scope for it: borders,
                    // backgrounds, margins, and inline-box geometry belong to
                    // the absent box, not its generated text or images.
                    // <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
                    let scope = (!contents_generated_pseudo).then(|| {
                        self.begin_inline_element_scope(
                            box_.core.element,
                            &inline_style,
                            link.clone(),
                            child_placement,
                            InlineElementScopeOptions::BOX_PAINT
                                .with_fragment_edges(box_.fragment_edges)
                                .with_preserved_empty_metrics(
                                    empty_inline_scope_has_distinct_metrics(
                                        block_style,
                                        &inline_style,
                                    ),
                                ),
                            output,
                        )
                    });
                    let scope_positioning_containing_block = scope
                        .as_ref()
                        .and_then(InlineElementScopeState::positioning_containing_block_source);
                    let next_float_containing_block =
                        if inline_style_establishes_positioning_containing_block(&inline_style) {
                            scope_positioning_containing_block
                        } else {
                            active_float_containing_block
                        };
                    let scope_establishes_positioned_containing_block =
                        scope_positioning_containing_block.is_some();
                    let ruby_positioning_source = (inline_style.display.is_ruby()
                        || inline_style.display.is_ruby_internal())
                    .then(|| {
                        scope_positioning_containing_block
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                    })
                    .flatten();
                    let mut scope_deferred_positioned_descendants = Vec::new();
                    let inlinified_ruby_children = inline_style
                        .display
                        .is_ruby_internal()
                        .then(|| ruby::inlinified_direct_children(&box_.core.children));
                    let inline_children = inlinified_ruby_children
                        .as_deref()
                        .unwrap_or(&box_.core.children);
                    if inline_style.content.is_generated() {
                        let generated_pseudo_content_style =
                            matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
                                .then(|| generated_pseudo_inline_content_style(&inline_style));
                        let content_style = generated_pseudo_content_style
                            .as_ref()
                            .unwrap_or(&inline_style);
                        let start_len = output.len();
                        self.push_element_content_items_from_boxes(
                            box_.core.element,
                            content_style,
                            match &box_.core.source {
                                box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                                    pseudo.kind.counter_event_source()
                                }
                                box_tree::BoxSource::Principal => {
                                    box_tree::CounterEventSource::Principal
                                }
                            },
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            block_style,
                            decoration_layers.clone(),
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.core.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.core.element,
                            clear,
                            output,
                            start_len,
                        );
                        if let Some(footnote_call) = footnote_call {
                            mark_inline_items_as_footnote_call(
                                &mut output[start_len..],
                                footnote_call,
                            );
                        }
                    } else {
                        self.collect_inline_box_items_with_float_containing_block(
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            block_style,
                            &inline_style,
                            &inline_style,
                            decoration_layers,
                            next_float_containing_block,
                            if scope_establishes_positioned_containing_block {
                                Some(&mut scope_deferred_positioned_descendants)
                            } else {
                                deferred_positioned_descendants.as_deref_mut()
                            },
                            deferred_static_positioned_descendants.as_deref_mut(),
                            output,
                        );
                    }
                    if let Some(scope) = scope {
                        self.end_inline_element_scope(scope, &inline_style, output);
                    }
                    if scope_establishes_positioned_containing_block {
                        let deferred_element_ids = scope_deferred_positioned_descendants
                            .iter()
                            .map(|descendant| descendant.element.id)
                            .collect::<Vec<_>>();
                        self.layout_deferred_inline_positioned_descendants(
                            scope_deferred_positioned_descendants,
                            stylesheets,
                            block_style,
                            output,
                        );
                        if let Some(source) = ruby_positioning_source.as_ref() {
                            self.layout_undeferred_ruby_positioned_descendants(
                                &box_.core.children,
                                stylesheets,
                                block_style,
                                source,
                                &deferred_element_ids,
                                output,
                            );
                        }
                    }
                    if principal_source {
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                    }
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let principal_source =
                        matches!(&box_.core.source, box_tree::BoxSource::Principal);
                    if principal_source && is_line_break_element(box_.core.element) {
                        // HTML keeps `<br>` as a forced line-break control
                        // even when an author declaration gives its principal
                        // box an atomic inline display. Treating that box as
                        // an ordinary atom would import the generated newline
                        // as an internal line, giving the break a height and a
                        // baseline of its own before a following float.
                        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                        self.capture_suppressed_named_strings_before(box_.core.element.id);
                        output.push(InlineItem::Break(InlineBreak {
                            clear: box_.core.style.clear,
                            origin: InlineBreakOrigin::Explicit,
                        }));
                        self.capture_suppressed_named_strings_after(box_.core.element.id);
                        continue;
                    }
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            link,
                            baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::Table(box_)
                    if box_.core.style.display.is_inline_level() =>
                {
                    // The table tree retains its durable `Table` variant
                    // independently of its outer display. An `inline-table`
                    // is nevertheless an atomic inline in this formatting
                    // context, so collecting it as a block drops both its
                    // intrinsic inline contribution and exported baseline.
                    // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        Some(&box_.fragment),
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    }
                }
                box_tree::FormattingBox::Replaced(box_) => {
                    // Replaced boxes are atomic inline-level boxes. They
                    // participate in the same line construction, float
                    // blockification, baseline calculation, and principal
                    // paint path as an `AtomicInline` box; dropping them
                    // here makes a normal-flow block or float lose inline
                    // images, canvases, and other replaced content.
                    // <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
                    // <https://www.w3.org/TR/css-display-3/#replaced-element>
                    if box_.core.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.core.element.clone(),
                            box_.core.signature.clone(),
                            (*box_.core.style).clone(),
                            box_.core.style.content.is_generated(),
                            active_float_containing_block
                                .map(BorrowedInlinePositioningContainingBlockSource::into_owned),
                        ))));
                        continue;
                    }
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.inline_atom_for_element(
                        box_.core.element,
                        &box_.core.signature,
                        &box_.core.style,
                        &box_.core.children,
                        None,
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    );
                    self.end_counter_scope(counter_scope);
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, parent_baseline_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            link,
                            baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_inline_box_items_with_float_containing_block(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        parent_baseline_style,
                        static_position_container_style,
                        propagated_decoration_layers_for_child(
                            &propagated_decoration_layers,
                            &box_.style,
                        ),
                        active_float_containing_block,
                        deferred_positioned_descendants.as_deref_mut(),
                        deferred_static_positioned_descendants.as_deref_mut(),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Flex(_) => {}
                box_tree::FormattingBox::Table(_) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_positioned_inline_descendant(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        static_position_container_style: &ComputedStyle,
        positioning_containing_block_source: Option<
            BorrowedInlinePositioningContainingBlockSource<'_>,
        >,
        static_position_containing_block: Option<StaticPositionContainingBlock>,
        static_position_index: Option<usize>,
        hypothetical_ancestor_offset: InlineVisualOffset,
        output: &[InlineItem],
    ) {
        if self.positioned_inline_layout_suppression_depth > 0 {
            return;
        }
        let source_was_inline_level =
            style.abspos_static_source.is_inline_level() || style.display.is_inline_level();
        if source_was_inline_level {
            // A horizontal replaced source inside a principal vertical flow
            // is measured in a scratch physical span. Its normal-flow
            // parent is replayed at the vertical ancestor's block-start, but
            // positioned paint is owned independently and therefore needs the
            // same hypothetical horizontal static rectangle before layout.
            // Block sources retain their own block static-position rules.
            // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
            // <https://www.w3.org/TR/css-position-3/#static-position>
            let previous_principal_static_position = self.absolute_static_position;
            if (is_replaced_element(element) || matches!(element.tag.as_str(), "audio" | "svg"))
                && !block_style.writing_mode.has_vertical_lines()
                && let Some(vertical_parent) = self.static_position_containing_blocks.last()
                && vertical_parent.axes.writing_mode().has_vertical_lines()
                && vertical_parent.axes.writing_mode() == WritingMode::VerticalRl
            {
                let child_physical_width = (self.content_right - self.content_left).max(0.0);
                let static_x = match block_start_side(vertical_parent.axes.writing_mode()) {
                    PhysicalSide::Left => vertical_parent.content_rect.x(),
                    PhysicalSide::Right => {
                        vertical_parent.content_rect.x() + vertical_parent.content_rect.width()
                            - child_physical_width
                    }
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("a vertical writing mode must have a horizontal block axis")
                    }
                };
                self.absolute_static_position = Some(
                    AbsoluteStaticPosition::from_page_horizontal_position(static_x, static_x),
                );
            }
            let mut positioned_style = style.clone();
            positioned_style.abspos_static_source = if style.abspos_static_source.is_atomic_inline()
            {
                style.abspos_static_source
            } else if style.display.is_atomic_inline() {
                css::StaticPositionSource::from_display(style.display)
            } else {
                css::StaticPositionSource::Inline
            };
            let mut static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                static_position_index,
                output,
            );
            let static_area = static_position.rectangle.area;
            static_position.rectangle.area = PageTopRect::new(
                static_area.x() + hypothetical_ancestor_offset.x(),
                static_area.top_y() + hypothetical_ancestor_offset.y(),
                static_area.width(),
                static_area.height(),
            );
            let static_area = static_position.rectangle.area;
            log::trace!(
                target: "quire::layout::inline_static_verbose",
                "checkpoint=deferred-replay element={:?} source=inline deferred_index={:?} static_axes=({:?},{:?}) rect=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
                element.id,
                static_position_index,
                static_position.rectangle.writing_mode,
                static_position.rectangle.direction,
                static_area.x(),
                static_area.top_y(),
                static_area.width(),
                static_area.height(),
            );
            let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
            let positioned_containing_block_scope =
                positioning_containing_block_source.and_then(|source| {
                    let mode = PositionedContainingBlockMode::for_style(source.style)?;
                    let containing_block = self.inline_positioning_containing_block_from_items(
                        source,
                        block_style,
                        output,
                    )?;
                    // An inline-block lays out its contents in a temporary
                    // page before replaying its atom at the final line
                    // position.  This inline source is local to that same
                    // temporary page, so its positioned descendants must
                    // escape with the atom rather than retain the temporary
                    // page coordinates.
                    // <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>
                    if self.escaped_atom_positioning_depth > 0 {
                        self.escaped_atom_containing_block = Some(containing_block);
                    }
                    Some(self.push_positioned_containing_block(mode, containing_block))
                });
            self.out_of_flow_prebreak_suppression_depth += 1;
            self.layout_positioned_block_with_inline_static_position(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_position,
            );
            self.out_of_flow_prebreak_suppression_depth -= 1;
            if let Some(scope) = positioned_containing_block_scope {
                self.pop_positioned_containing_block(scope);
                self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
            }
            self.absolute_static_position = previous_principal_static_position;
            return;
        }

        let placeholder_geometry = self.hypothetical_block_static_placeholder_geometry(
            element,
            style,
            stylesheets,
            child_boxes,
            block_style,
        );
        let placeholder_box = self
            .block_static_position_placeholder_box_from_buffer(
                output,
                block_style,
                placeholder_geometry,
                static_position_index,
            )
            .unwrap_or_else(|| PageTopRect::new(self.content_left, self.cursor_y, 0.0, 0.0));
        let hypothetical_block_margin_box = HypotheticalBlockMarginBox::from_placeholder(
            placeholder_box,
            hypothetical_ancestor_offset,
        );
        if self.escaped_atom_positioning_depth == 0
            && !output.is_empty()
            && output.iter().all(|item| {
            matches!(item, InlineItem::Word(word) if word.text.chars().all(|character| character == '\u{a0}'))
        })
        {
            // This is a block-level source after an inline-only buffer. Its
            // hypothetical box starts at the block formatting context's
            // inline edge, not after the buffered glyph advance. The latter
            // is the static position for an inline-level source, and moves a
            // block abspos after an NBSP by that glyph's width.
            // <https://www.w3.org/TR/css-position-3/#static-position>
            let previous = self.absolute_static_position;
            self.absolute_static_position = Some(
                AbsoluteStaticPosition::from_page_horizontal_position(
                    self.content_left,
                    self.content_right,
                ),
            );
            self.out_of_flow_prebreak_suppression_depth += 1;
            self.layout_positioned_block(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
            );
            self.out_of_flow_prebreak_suppression_depth -= 1;
            self.absolute_static_position = previous;
            return;
        }
        let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
        let previous_block_static_rectangle = self.absolute_static_position;
        // A block-level positioned source reached from an inline collection
        // (for example after whitespace in an otherwise block container)
        // bypasses the ordinary block-child dispatcher. Capture the same
        // immutable static-position rectangle at this boundary before its
        // delayed positioned layout unwinds the source formatting context.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        // The rectangle's logical axes belong to its static containing
        // block, not necessarily to the lexical inline that happened to
        // dispatch this child. A positioned inline establishes that owner
        // explicitly; otherwise the active block formatting context does.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        let (static_writing_mode, static_direction, static_justify_items, static_align_items) =
            if let Some(context) = static_position_containing_block
                .or_else(|| self.static_position_containing_blocks.last().copied())
            {
                (
                    context.axes.writing_mode(),
                    context.axes.direction(),
                    context.justify_items,
                    css::SelfAlignment::NORMAL,
                )
            } else {
                (
                    static_position_container_style.writing_mode,
                    static_position_container_style.used_direction(),
                    static_position_container_style.justify_items,
                    static_position_container_style.align_items,
                )
            };
        // Buffered inline content precedes a block-level hypothetical source.
        // Capture its block-start now, rather than retaining the current
        // unadvanced inline collector cursor and expecting later positioned
        // layout to reconstruct that information.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        let static_rectangle = static_position_containing_block
            .or_else(|| self.static_position_containing_blocks.last().copied())
            .map(|context| hypothetical_block_margin_box.static_rectangle(context))
            .unwrap_or_else(|| {
                // A root-level fallback has no enclosing block context to
                // retain. It still obeys the block static-rectangle shape.
                let area = if static_writing_mode.has_vertical_lines() {
                    PageTopRect::new(
                        placeholder_box.x(),
                        self.cursor_y,
                        0.0,
                        self.current_content_logical_inline_size(),
                    )
                } else {
                    PageTopRect::new(
                        self.content_left,
                        placeholder_box.top_y(),
                        (self.content_right - self.content_left).max(0.0),
                        0.0,
                    )
                };
                StaticPositionRectangle {
                    area: hypothetical_block_margin_box.translate_inline_span(
                        area,
                        WritingModeAxes::new(static_writing_mode, static_direction)
                            .physical_axis(LogicalAxis::Inline),
                    ),
                    writing_mode: static_writing_mode,
                    direction: static_direction,
                    justify_items: static_justify_items,
                    align_items: static_align_items,
                }
            });
        log::trace!(
            target: "quire::layout::static_position",
            "checkpoint=capture element={:?} source=block deferred_index={:?} hypothetical=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) static_axes=({:?},{:?}) rect=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) buffered_block_offset={:.2} containing_inline={:?}",
            element.id,
            static_position_index,
            placeholder_box.x(),
            placeholder_box.top_y(),
            placeholder_box.width(),
            placeholder_box.height(),
            static_rectangle.writing_mode,
            static_rectangle.direction,
            static_rectangle.area.x(),
            static_rectangle.area.top_y(),
            static_rectangle.area.width(),
            static_rectangle.area.height(),
            0.0,
            positioning_containing_block_source.map(|source| source.id),
        );
        let absolute_static_position = self.absolute_static_position.unwrap_or_else(|| {
            AbsoluteStaticPosition::from_page_horizontal_position(
                self.content_left,
                self.content_right,
            )
        });
        self.absolute_static_position =
            Some(if static_rectangle.writing_mode.has_vertical_lines() {
                // The physical page-top fallback is the vertical flow's logical
                // inline axis. Preserve the static rectangle's just-captured
                // inline edge instead of an earlier block-marker coordinate.
                absolute_static_position.with_inline_static_position_rectangle(static_rectangle)
            } else {
                absolute_static_position.with_static_position_rectangle(static_rectangle)
            });
        let positioned_containing_block_scope =
            positioning_containing_block_source.and_then(|source| {
                let mode = PositionedContainingBlockMode::for_style(source.style)?;
                let containing_block = self.inline_positioning_containing_block_from_items(
                    source,
                    block_style,
                    output,
                );
                let containing_block = containing_block?;
                // See the corresponding inline-level branch above.  The
                // source containing block is expressed in the temporary
                // atom page and therefore moves with that atom on escape.
                if self.escaped_atom_positioning_depth > 0 {
                    self.escaped_atom_containing_block = Some(containing_block);
                }
                Some(self.push_positioned_containing_block(mode, containing_block))
            });
        self.out_of_flow_prebreak_suppression_depth += 1;
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.out_of_flow_prebreak_suppression_depth -= 1;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
            self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
        }
        self.absolute_static_position = previous_block_static_rectangle;
    }

    /// Measure the hypothetical normal-flow margin box used by a block-level
    /// positioned source while inline collection selects its static rectangle.
    ///
    /// A vertical source's physical width is its logical block-size.  This is
    /// deliberately measured through the shared block-width resolver, then
    /// expanded through the box model once, rather than borrowing the parent
    /// line-height used by the zero-footprint selection marker.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    #[allow(clippy::too_many_arguments)]
    fn hypothetical_block_static_placeholder_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        block_style: &ComputedStyle,
    ) -> BlockStaticPositionPlaceholderGeometry {
        if !block_style.writing_mode.has_vertical_lines() {
            return BlockStaticPositionPlaceholderGeometry::Horizontal;
        }

        let mut hypothetical =
            StaticHypotheticalBox::from_positioned(self.style_with_current_viewport_lengths(style));
        let hypothetical_style = &mut hypothetical.style;
        apply_used_box_metrics_for_logical_inline_basis(
            hypothetical_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let horizontal_non_content = non_content_pt(
            hypothetical_style.padding.left
                + hypothetical_style.padding.right
                + horizontal_border_width(hypothetical_style),
        );
        let vertical_non_content = non_content_pt(
            hypothetical_style.padding.top
                + hypothetical_style.padding.bottom
                + vertical_border_width(hypothetical_style),
        );
        let containing_block_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        let definite_content_height = used_content_box_height_or_auto_with_basis(
            hypothetical_style,
            containing_block_height,
            vertical_non_content,
        )
        .map(PhysicalContentHeight::new);
        let containing_physical_width =
            layout_pt((self.content_right - self.content_left).max(0.0));
        let content_width = self.used_block_physical_content_width(
            element,
            hypothetical_style,
            stylesheets,
            child_boxes,
            BlockContentWidthInputs {
                available_outer_width: layout_pt(
                    containing_physical_width.points()
                        - hypothetical_style.margin.left
                        - hypothetical_style.margin.right,
                ),
                percentage_basis: PercentageBasis::definite(containing_physical_width),
                horizontal_non_content,
                definite_content_height,
                auto_width_role: BlockAutoWidthRole::NormalFlow,
            },
        );
        BlockStaticPositionPlaceholderGeometry::Vertical {
            physical_margin_box_block_extent: content_box_to_margin_box_length(
                content_width.content_box_length(),
                horizontal_non_content,
                layout_pt(hypothetical_style.margin.left + hypothetical_style.margin.right),
            ),
        }
    }

    /// Resolves the padding-box rectangle established by a positioned inline.
    ///
    /// Inline collection retains zero-advance edge atoms for positioned
    /// ancestors. Replaying those source markers through normal line
    /// preparation gives the first and last generated inline fragments their
    /// final physical coordinates, including bidi reordering, fragmentation,
    /// and writing-mode transforms. CSS 2.2 defines the absolute containing
    /// block from exactly those padding edges:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    fn inline_positioning_containing_block_from_items(
        &mut self,
        source: BorrowedInlinePositioningContainingBlockSource<'_>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) -> Option<ContainingBlock> {
        let mut items = output.to_vec();
        // The positioned descendant is encountered before its enclosing
        // inline scope emits the end marker. Add that marker only to this
        // hypothetical line sequence so the real source stream remains in
        // DOM order.
        self.push_inline_box_edge_item(
            source.style,
            InlineBoxEdge::End,
            Some(source.id),
            0.0,
            InlineVisualOffset::zero(),
            None,
            &mut items,
        );
        let available_width = self.current_content_logical_inline_size().max(1.0);
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);

        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let context = sequence.context(block_style);
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let mut plaintext_direction_state = None;
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        if matches!(
            block_style.writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        ) {
            stack.advance(records.first().map(|record| record.height()).unwrap_or(0.0));
        }
        let mut start = None;
        let mut end = None;
        for record in &records {
            stack.apply(self);
            self.apply_line_block_start_trim_for_paint(record, block_style.writing_mode);
            if let Some(prepared) =
                self.prepare_inline_line_record(record, context, &mut plaintext_direction_state)
            {
                // The prepared line is layout output, even though it is
                // shared with painting. Capture only the fragment(s) on the
                // source-order start/end lines; a union of all painted
                // fragments incorrectly turns multiline and bidi fragments
                // into a physical bounding box.
                let mut source_fragment_bounds: Option<(f32, f32, f32, f32)> = None;
                for item in &prepared.paint_items {
                    if let PreparedInlinePaintItem::FragmentBackground(fragment) = item
                        && fragment.fragment.ancestor_inline_decorations().iter().any(
                            |decoration| {
                                decoration.positioning_containing_block_id == Some(source.id)
                            },
                        )
                    {
                        let rect = fragment.rect;
                        let bounds = (rect.x(), rect.y(), rect.width(), rect.height());
                        source_fragment_bounds = Some(match source_fragment_bounds {
                            Some((left, bottom, right, top)) => (
                                left.min(bounds.0),
                                bottom.min(bounds.1),
                                right.max(bounds.0 + bounds.2),
                                top.max(bounds.1 + bounds.3),
                            ),
                            None => (bounds.0, bounds.1, bounds.0 + bounds.2, bounds.1 + bounds.3),
                        });
                    }
                }
                for item in &prepared.paint_items {
                    let PreparedInlinePaintItem::Atom(atom) = item else {
                        continue;
                    };
                    let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) =
                        atom.atom.content()
                    else {
                        continue;
                    };
                    if edge.positioning_containing_block_id != Some(source.id) {
                        continue;
                    }
                    let rect = atom.border_box;
                    // Horizontal containing-block replay has long used the
                    // explicit edge atoms successfully. In vertical flow an
                    // edge atom is zero-advance on the physical inline axis,
                    // so pair it with the prepared source fragment on that
                    // line to retain the padding-box block extent.
                    let atom_bounds = (
                        rect.x(),
                        rect.y(),
                        rect.x() + rect.width(),
                        rect.y() + rect.height(),
                    );
                    let bounds =
                        WritingModeAxes::new(source.style.writing_mode, source.style.direction)
                            .swaps_physical_axes()
                            .then_some(source_fragment_bounds)
                            .flatten()
                            .unwrap_or(atom_bounds);
                    let edge_capture = InlinePositioningFragmentEdgeCapture {
                        logical_edge: edge.logical_edge,
                        rect: PageTopRect::new(
                            bounds.0,
                            bounds.3,
                            (bounds.2 - bounds.0).max(0.0),
                            (bounds.3 - bounds.1).max(0.0),
                        ),
                    };
                    match edge.logical_edge {
                        InlineLogicalEdge::Start => {
                            start.get_or_insert(edge_capture);
                        }
                        InlineLogicalEdge::End => end = Some(edge_capture),
                    };
                }
            }
            stack.advance(record.height());
        }
        self.cursor_y = saved_cursor_y;
        self.content_left = saved_left;
        self.content_right = saved_right;

        let start = start?;
        let end = end?;
        debug_assert_eq!(start.logical_edge, InlineLogicalEdge::Start);
        debug_assert_eq!(end.logical_edge, InlineLogicalEdge::End);
        let containing_block_edges = InlineContainingBlockContentEdges {
            first_fragment: start.rect,
            end_fragment: end.rect,
            axes: WritingModeAxes::new(source.style.writing_mode, source.style.used_direction()),
        };
        let containing_block = containing_block_edges.to_containing_block();
        let containing_rect = containing_block.rect;
        log::trace!(
            target: "quire::layout::static_position",
            "checkpoint=positioned-inline-containing-block source={:?} axes=({:?},{:?}) start=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) end=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) containing_block=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
            source.id,
            source.style.writing_mode,
            source.style.used_direction(),
            start.rect.x(),
            start.rect.top_y(),
            start.rect.width(),
            start.rect.height(),
            end.rect.x(),
            end.rect.top_y(),
            end.rect.width(),
            end.rect.height(),
            containing_rect.x(),
            containing_rect.top_y(),
            containing_rect.width(),
            containing_rect.height(),
        );
        Some(containing_block)
    }

    fn layout_deferred_inline_positioned_descendants(
        &mut self,
        descendants: Vec<DeferredInlinePositionedDescendant>,
        stylesheets: &Stylesheets<'_>,
        block_style: &ComputedStyle,
        output: &mut [InlineItem],
    ) {
        for descendant in descendants {
            // Rebuild only at the final inline edge, where its ancestor's
            // containing block can be measured from a complete item stream.
            // This preserves the immutable frozen-tree boundary without
            // carrying borrowed child boxes through collection.
            let child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                &descendant.element,
                stylesheets,
                &descendant.style,
            );
            let positioned_layer_start = self.positioned_layers.len();
            self.layout_positioned_inline_descendant(
                &descendant.element,
                &descendant.style,
                stylesheets,
                Some(&child_boxes),
                None,
                block_style,
                &descendant.static_position_container_style,
                Some(descendant.containing_block_source.as_borrowed()),
                None,
                None,
                InlineVisualOffset::zero(),
                output,
            );
            let layers = self.positioned_layers.split_off(positioned_layer_start);
            DeferredClampEffect::PositionedLayers {
                owner: descendant.containing_block_source.id,
                layers,
            }
            .attach_to_owner(output);
        }
    }

    /// Resolve inline static-position geometry only after the enclosing
    /// source stream is complete.  The selected hypothetical line may be
    /// enlarged by source following the abspos marker, even though the
    /// positioned box itself is out of flow.
    fn layout_deferred_inline_static_positioned_descendants(
        &mut self,
        descendants: Vec<DeferredInlineStaticPositionedDescendant>,
        stylesheets: &Stylesheets<'_>,
        output: &[InlineItem],
    ) {
        for descendant in descendants {
            let frozen_child_boxes =
                matches!(descendant.content, DeferredStaticPositionedContent::Frozen).then(|| {
                    self.build_frozen_child_boxes_with_current_ancestors(
                        &descendant.element,
                        stylesheets,
                        &descendant.style,
                    )
                });
            self.layout_positioned_inline_descendant(
                &descendant.element,
                &descendant.style,
                stylesheets,
                frozen_child_boxes.as_deref(),
                None,
                &descendant.line_formatting_context_style,
                &descendant.static_position_container_style,
                descendant
                    .positioning_containing_block_source
                    .as_ref()
                    .map(InlinePositioningContainingBlockSource::as_borrowed),
                descendant.static_position_containing_block,
                Some(descendant.static_position_index),
                descendant.hypothetical_ancestor_offset,
                output,
            );
        }
    }

    /// Replay explicitly inset positioned descendants that are nested in a
    /// ruby role but did not travel through the ordinary inline collector.
    ///
    /// Ruby's anonymous base/text containers may be structurally empty after
    /// excluding out-of-flow descendants. They must nevertheless inherit a
    /// positioned ruby/rbc scope as their containing block; CSS Ruby does not
    /// turn that ownership into ordinary annotation content.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    #[allow(clippy::too_many_arguments)]
    fn layout_undeferred_ruby_positioned_descendants(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        block_style: &ComputedStyle,
        containing_block_source: &InlinePositioningContainingBlockSource,
        already_deferred: &[ElementId],
        output: &[InlineItem],
    ) {
        for child in children {
            let Some((element, _, style, child_boxes)) = child.element_parts() else {
                if let box_tree::FormattingBox::AnonymousBlock(box_) = child {
                    self.layout_undeferred_ruby_positioned_descendants(
                        &box_.children,
                        stylesheets,
                        block_style,
                        containing_block_source,
                        already_deferred,
                        output,
                    );
                }
                continue;
            };
            if matches!(style.position, Position::Absolute | Position::Fixed)
                && positioned_descendant_has_explicit_inset(style)
            {
                if !already_deferred.contains(&element.id) {
                    let table_fragment = match child {
                        box_tree::FormattingBox::AtomicInline(box_) => box_.table_fragment.as_ref(),
                        box_tree::FormattingBox::Table(box_) => Some(&box_.fragment),
                        _ => None,
                    };
                    self.layout_positioned_inline_descendant(
                        element,
                        style,
                        stylesheets,
                        Some(child_boxes),
                        table_fragment,
                        block_style,
                        block_style,
                        Some(containing_block_source.as_borrowed()),
                        None,
                        None,
                        InlineVisualOffset::zero(),
                        output,
                    );
                }
                continue;
            }
            self.layout_undeferred_ruby_positioned_descendants(
                child_boxes,
                stylesheets,
                block_style,
                containing_block_source,
                already_deferred,
                output,
            );
        }
    }

    /// Collect a ruby container through its normalized base/annotation
    /// columns. Both the source-DOM and frozen-box collectors use this one
    /// materialization boundary, ensuring that authored layout-internal roles
    /// do not alter ruby pairing semantics.
    #[allow(clippy::too_many_arguments)]
    fn collect_normalized_ruby_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        ruby_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        link: Option<String>,
        placement: InlinePlacement,
        block_style: &ComputedStyle,
        first_letter_style: Option<&ComputedStyle>,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) -> bool {
        let normalized = ruby::NormalizedRuby::from_children(children);
        debug_assert!(
            normalized
                .columns
                .iter()
                .all(|column| column.annotations.len() == normalized.annotation_level_count)
        );
        let mut ruby_atoms = Vec::with_capacity(normalized.columns.len());
        let mut pending_first_letter_style = first_letter_style;
        for column in &normalized.columns {
            let Some((mut atom, has_base_content)) = self.ruby_inline_atom(
                column,
                &normalized.annotation_container_styles,
                ruby_style,
                stylesheets,
                link.clone(),
                placement,
                block_style,
                pending_first_letter_style,
                propagated_decoration_layers.clone(),
            ) else {
                return false;
            };
            if has_base_content {
                pending_first_letter_style = None;
            }
            atom.baseline_shift += self
                .vertical_align_baseline_shift_for_atom(&atom, block_style)
                .glyph_displacement()
                .get();
            ruby_atoms.push(InlineItem::Atom(Box::new(atom)));
        }
        normalize_ruby_column_group_metrics(&mut ruby_atoms, block_style);
        normalize_ruby_annotation_span_inline_sizes(&mut ruby_atoms, block_style);
        if ruby_atoms.is_empty() {
            return false;
        }
        output.extend(ruby_atoms);
        true
    }

    /// Build a coupled ruby base/annotation atom from normalized in-flow
    /// segments.  This is the materialization boundary between CSS Ruby's
    /// paired levels and the parent inline graph.
    ///
    /// The graph currently keeps a whole ruby group together; later work can
    /// replace this atom with per-column graph ranges without changing the
    /// normalization or paint representation.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    #[allow(clippy::too_many_arguments)]
    fn ruby_inline_atom(
        &mut self,
        column: &ruby::RubyColumn<'_>,
        annotation_container_styles: &[Option<Rc<ComputedStyle>>],
        ruby_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        link: Option<String>,
        placement: InlinePlacement,
        block_style: &ComputedStyle,
        first_letter_style: Option<&ComputedStyle>,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
    ) -> Option<(InlineAtom, bool)> {
        // A Ruby level is not an independently originating block line. Its
        // own `::first-line` rules must therefore remain dormant; the parent
        // block applies its selected overlay to the complete ruby formatting
        // context once the parent first line is known.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        let mut base_style = column.base.style.as_deref().unwrap_or(ruby_style).clone();
        base_style.first_line_style = None;
        base_style.suppress_inapplicable_transform();
        let mut base_items = Vec::new();
        self.collect_inline_box_items(
            &column.base.boxes,
            stylesheets,
            link.clone(),
            placement.baseline_shift(),
            placement.visual_offset,
            block_style,
            propagated_decoration_layers.clone(),
            &mut base_items,
        );
        let has_base_content = base_items.iter().any(inline_item_has_typographic_content);
        if let Some(first_letter_style) = first_letter_style {
            apply_first_letter_style_to_ruby_base_items(&mut base_items, first_letter_style);
        }
        // Ruby's no-break-inside default means this temporary measurement can
        // use a deliberately unbounded inline span.  Its selected fragments
        // are replayed into the final coupled width below.
        let unconstrained_inline_size = 1_000_000.0;
        let base_items_for_distribution = base_items.clone();
        let mut base = RubyInlineLevel {
            sequence: self.collect_ruby_level_line_sequence(
                base_items,
                &base_style,
                unconstrained_inline_size,
                0.0,
                0.0,
            ),
            style: Box::new(base_style.clone()),
            overhang_policy: ruby_style.ruby_overhang,
            paint_inline_size: ruby::RubyPaintInlineSpan::default(),
            containing_inline_size: ruby::RubyColumnInlineSpan::default(),
            starts_span: true,
            column_span: 1,
        };
        let mut annotations = Vec::with_capacity(column.annotations.len());
        let mut annotation_sides = Vec::with_capacity(column.annotations.len());
        let mut annotation_items_for_distribution = Vec::with_capacity(column.annotations.len());
        for (annotation_index, annotation) in column.annotations.iter().enumerate() {
            let annotation_container_style = annotation_container_styles
                .get(annotation_index)
                .and_then(Option::as_deref)
                .unwrap_or(ruby_style);
            let mut annotation_style = annotation
                .segment
                .style
                .as_deref()
                .unwrap_or(ruby_style)
                .clone();
            annotation_style.first_line_style = None;
            annotation_style.suppress_inapplicable_transform();
            annotation_sides.push(annotation_style.ruby_position.interlinear_side());
            let mut annotation_items = Vec::new();
            // A structurally present annotation containing generated `" "`
            // is real ruby content. Only the explicitly synthesized empty
            // counterpart has no inner formatting context to collect.
            // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
            if annotation.starts_span && !annotation.segment.is_empty() {
                self.collect_inline_box_items(
                    &annotation.segment.boxes,
                    stylesheets,
                    link.clone(),
                    placement.baseline_shift(),
                    placement.visual_offset,
                    block_style,
                    propagated_decoration_layers.clone(),
                    &mut annotation_items,
                );
            }
            annotation_items_for_distribution.push(annotation_items.clone());
            annotations.push(RubyInlineLevel {
                sequence: self.collect_ruby_level_line_sequence(
                    annotation_items,
                    &annotation_style,
                    unconstrained_inline_size,
                    0.0,
                    0.0,
                ),
                style: Box::new(annotation_style.clone()),
                overhang_policy: annotation_container_style.ruby_overhang,
                paint_inline_size: ruby::RubyPaintInlineSpan::default(),
                containing_inline_size: ruby::RubyColumnInlineSpan::default(),
                starts_span: annotation.starts_span,
                column_span: annotation.span,
            });
        }
        if !has_base_content
            && !annotations.iter().any(|sequence| {
                sequence
                    .sequence
                    .records
                    .iter()
                    .any(|record| record.fragment.is_some())
            })
        {
            return None;
        }
        let sequence_inline_size = |sequence: &inline_layout::InlineLineSequence| {
            sequence
                .records
                .iter()
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max)
        };
        // The source atom is conservatively measured at the widest level so
        // candidate line fitting never accepts an annotation that cannot fit.
        // The paired base-column span remains distinct: selected-line ruby
        // overhang later borrows adjacent inline space and reduces this
        // provisional advance without changing source geometry.
        let provisional_inline_size = ruby::RubyInlineSpan::new(
            column
                .annotations
                .iter()
                .zip(annotations.iter())
                // A spanning annotation is sized and aligned across the complete
                // paired base range. It must not inflate each base column (and
                // thereby manufacture parent-line justification opportunities);
                // excess annotation width overhangs the spanned range.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
                .filter(|(annotation, _)| annotation.span == 1)
                .map(|(_, sequence)| sequence_inline_size(&sequence.sequence))
                .fold(sequence_inline_size(&base.sequence), f32::max),
        )
        .points();
        let column_inline_size = sequence_inline_size(&base.sequence);
        base.paint_inline_size =
            ruby::RubyPaintInlineSpan::new(sequence_inline_size(&base.sequence));
        base.containing_inline_size = ruby::RubyColumnInlineSpan::new(column_inline_size);
        for annotation in &mut annotations {
            annotation.paint_inline_size =
                ruby::RubyPaintInlineSpan::new(sequence_inline_size(&annotation.sequence));
            annotation.containing_inline_size = ruby::RubyColumnInlineSpan::new(column_inline_size);
        }
        self.distribute_ruby_level_space_around(
            &mut base,
            &base_items_for_distribution,
            column_inline_size,
        );
        for ((annotation, source_items), pairing) in annotations
            .iter_mut()
            .zip(annotation_items_for_distribution.iter())
            .zip(column.annotations.iter())
        {
            // A spanning annotation is positioned by the column group that
            // owns its full span. This per-column atom only has its local
            // span available today, so retain its natural alignment until
            // group-level span paint is materialized below.
            if pairing.span == 1 {
                self.distribute_ruby_level_space_around(
                    annotation,
                    source_items,
                    column_inline_size,
                );
            }
        }
        let base_block_size = base.sequence.total_height().max(base_style.line_height);
        let annotation_block_sizes = annotations
            .iter()
            .map(|annotation| annotation.sequence.total_height())
            .collect::<Vec<_>>();
        let annotation_block_size = annotation_block_sizes.iter().sum::<f32>();
        let base_baseline = base
            .sequence
            .records
            .iter()
            .find_map(|record| {
                record
                    .fragment
                    .as_ref()
                    .map(|fragment| fragment.metrics.baseline_offset)
            })
            .unwrap_or_else(|| {
                self.font_system
                    .rendered_first_line_baseline_offset(ruby_style)
                    .points()
            });
        Some((
            InlineAtom::new(
                InlineAtomContent::Ruby {
                    base_text: column.base.boundary_text(),
                    base,
                    annotations,
                    annotation_sides,
                    base_block_size,
                    annotation_block_sizes,
                },
                ruby_style.clone(),
                None,
                InlineSize::new(
                    provisional_inline_size,
                    base_block_size + annotation_block_size,
                ),
                annotation_block_size + base_baseline,
                placement.baseline_shift(),
                link,
                None,
            )
            .with_visual_offset(placement.visual_offset),
            has_base_content,
        ))
    }

    /// Select one ruby base or annotation level in its own float context.
    ///
    /// CSS Ruby positions the complete ruby container against parent floats.
    /// Its captured base and annotation levels are then painted inside that
    /// already-positioned atom, so allowing either phase to inherit the
    /// parent's float exclusions would apply the same band a second time.
    /// Floats and positioned descendants authored *inside* ruby are retained
    /// on the generic overlay path before this local sequence is built.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    fn collect_ruby_level_line_sequence(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
    ) -> inline_layout::InlineLineSequence {
        self.with_replay_float_scope(ReplayFloatScope::IsolatedFormattingContext, |layout| {
            layout.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                available_width,
                padding_left,
                hanging_indent,
            )
        })
        .with_replay_float_scope(ReplayFloatScope::IsolatedFormattingContext)
    }

    /// Apply the selected `ruby-align` distribution to a level.
    ///
    /// The CSS Ruby UA rule delegates its inner opportunities to
    /// `text-justify: ruby`; this implementation uses the existing
    /// typographic-unit justification path for CJK-wide units, then reserves
    /// half an equal opportunity at each edge of the level.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
    fn distribute_ruby_level_space_around(
        &mut self,
        level: &mut RubyInlineLevel,
        items: &[InlineItem],
        containing_inline_size: f32,
    ) {
        let ruby_align = level.style.ruby_align;
        if matches!(ruby_align, css::RubyAlign::Start | css::RubyAlign::Center) {
            return;
        }
        let natural_inline_size = ruby_line_sequence_inline_size(&level.sequence);
        let Some(unit_count) = ruby_distribution_unit_count(items) else {
            return;
        };
        let free_space = (containing_inline_size - natural_inline_size).max(0.0);
        if free_space <= 0.0 {
            return;
        }
        let core_inline_size = match ruby_align {
            // `space-between` distributes only across interior CJK unit
            // boundaries. A single unit has no opportunity and remains
            // centered by the selected-line alignment geometry.
            css::RubyAlign::SpaceBetween if unit_count > 1 => containing_inline_size,
            css::RubyAlign::SpaceBetween => return,
            // `space-around` has one extra opportunity split across the two
            // edges. With N CJK units, the N equal shares leave N-1 internal
            // gaps and one split edge gap.
            css::RubyAlign::SpaceAround => {
                let per_opportunity = free_space / unit_count as f32;
                (containing_inline_size - per_opportunity).max(natural_inline_size)
            }
            css::RubyAlign::Start | css::RubyAlign::Center => unreachable!(),
        };
        let mut distribution_style = (*level.style).clone();
        distribution_style.text_align = TextAlign::JustifyAll;
        distribution_style.text_justify = TextJustify::InterCharacter;
        level.sequence = self.collect_ruby_level_line_sequence(
            items.to_vec(),
            &distribution_style,
            core_inline_size,
            0.0,
            0.0,
        );
        *level.style = distribution_style;
        level.paint_inline_size = ruby::RubyPaintInlineSpan::new(core_inline_size);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_from_hypothetical_placeholder(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        static_position_index: Option<usize>,
        output: &[InlineItem],
    ) -> StaticPositionCapture {
        let placeholder = self.inline_static_position_placeholder_atom(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
        );
        let static_position_index = static_position_index
            .unwrap_or(output.len())
            .min(output.len());
        let mut hypothetical_items = Vec::with_capacity(output.len() + 1);
        hypothetical_items.extend_from_slice(&output[..static_position_index]);
        hypothetical_items.push(InlineItem::Atom(Box::new(placeholder)));
        hypothetical_items.extend_from_slice(&output[static_position_index..]);
        let available_width = self.current_content_logical_inline_size().max(1.0);
        log::trace!(
            target: "quire::layout::inline_static_verbose",
            "checkpoint=placeholder element={:?} source=inline deferred_index={} prior_items={} available_logical_inline={:.2} static_axes=({:?},{:?}) page=(left:{:.2},top:{:.2},right:{:.2})",
            element.id,
            static_position_index,
            output.len(),
            available_width,
            block_style.writing_mode,
            block_style.used_direction(),
            self.content_left,
            self.cursor_y,
            self.content_right,
        );
        // CSS Positioned Layout defines the static-position rectangle as the
        // box's hypothetical normal-flow position. Carrying a non-painting
        // placeholder through ordinary inline line selection keeps forced
        // breaks, wrapping, and line metrics aligned with the same CSS Text
        // machinery used for real inline content:
        // https://www.w3.org/TR/css-position-3/#staticpos-rect
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        // Static-position resolution is a hypothetical inline layout. Its
        // float placement may build paint fragments and exclusions while
        // fitting the placeholder, but none of those side effects belong to
        // the real inline run that follows.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://www.w3.org/TR/CSS22/visuren.html#abs-non-replaced-height>
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);
        let placeholder_capture =
            self.inline_static_position_from_placeholder_sequence(element, &sequence, block_style);
        let capture = placeholder_capture.unwrap_or_else(|| StaticPositionCapture {
            rectangle: StaticPositionRectangle {
                area: if block_style.writing_mode.has_vertical_lines() {
                    PageTopRect::new(
                        self.content_left,
                        self.cursor_y,
                        block_style.line_height,
                        0.0,
                    )
                } else {
                    PageTopRect::new(
                        self.content_left,
                        self.cursor_y,
                        0.0,
                        block_style.line_height,
                    )
                },
                writing_mode: block_style.writing_mode,
                direction: block_style.used_direction(),
                justify_items: block_style.justify_items,
                align_items: block_style.align_items,
            },
        });
        let static_area = capture.rectangle.area;
        log::trace!(
            target: "quire::layout::inline_static_verbose",
            "checkpoint=capture element={:?} source=inline deferred_index={:?} output_items={} axes=({:?},{:?}) rect=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
            element.id,
            static_position_index,
            output.len(),
            capture.rectangle.writing_mode,
            capture.rectangle.direction,
            static_area.x(),
            static_area.top_y(),
            static_area.width(),
            static_area.height(),
        );
        capture
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_placeholder_atom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> InlineAtom {
        let available_width = (self.content_right - self.content_left).max(style.font_size);
        let mut hypothetical =
            StaticHypotheticalBox::from_positioned(self.style_with_current_viewport_lengths(style));
        let placeholder_style = &mut hypothetical.style;
        // A static-position rectangle is selected from a hypothetical
        // in-flow box. Its positioning, float, and clear values are reset,
        // while normal-flow margins remain part of the hypothetical box.
        // Preceding floats and ancestor clearance remain in the builder
        // snapshot and continue to constrain the placeholder line.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
        // Atomic inline sources have already been blockified by box-tree
        // construction. Their source `inline-block`/replaced display is not
        // retained as a separate used-display value yet, so preserve their
        // established capture path rather than turning that blockified box
        // into a hypothetical block. Non-atomic inline sources have the
        // required used display here and can be reset directly.
        apply_used_box_metrics_for_logical_inline_basis(
            placeholder_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let horizontal_non_content = placeholder_style.padding.left
            + placeholder_style.padding.right
            + horizontal_border_width(placeholder_style);
        let positioned_available_outer_width =
            (available_width - placeholder_style.margin.left - placeholder_style.margin.right)
                .max(placeholder_style.font_size);
        let vertical_non_content = placeholder_style.padding.top
            + placeholder_style.padding.bottom
            + vertical_border_width(placeholder_style);
        let containing_block_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        let resolved_content_height = used_content_box_height_or_auto_with_basis(
            placeholder_style,
            containing_block_height,
            non_content_pt(vertical_non_content),
        )
        .map(|height| {
            constrain_content_height(
                placeholder_style,
                height,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .points()
        });
        let content_width = if placeholder_style.writing_mode.has_vertical_lines()
            && placeholder_style.box_values.width.is_auto()
        {
            self.used_block_physical_content_width(
                element,
                placeholder_style,
                stylesheets,
                child_boxes,
                BlockContentWidthInputs {
                    available_outer_width: layout_pt(positioned_available_outer_width),
                    percentage_basis: PercentageBasis::definite(layout_pt(available_width)),
                    horizontal_non_content: non_content_pt(horizontal_non_content),
                    definite_content_height: resolved_content_height
                        .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                    auto_width_role: BlockAutoWidthRole::NormalFlow,
                },
            )
            .points()
        } else {
            self.used_intrinsic_or_shrink_to_fit_width(
                element,
                placeholder_style,
                stylesheets,
                layout_pt(positioned_available_outer_width),
                non_content_pt(horizontal_non_content),
                child_boxes,
                table_fragment,
            )
            .points()
        };
        let geometry = if placeholder_style.writing_mode.has_vertical_lines() {
            // Physical width is logical block-size, but the placeholder's
            // line advance is its logical inline max-content contribution.
            // Reusing `content_width` here made a five-glyph vertical source
            // advance by one glyph and captured the static rectangle at the
            // wrong inline edge.
            let inline_advance = resolved_content_height
                .map(|height| LogicalInlineContentSize::new(content_box_pt(height)))
                .unwrap_or_else(|| {
                    self.intrinsic_inline_contribution_for_element(
                        element,
                        placeholder_style,
                        stylesheets,
                        child_boxes,
                    )
                    .max_content
                });
            StaticInlinePlaceholderLogicalGeometry {
                inline_advance,
                block_extent: LogicalBlockContentSize::new(content_box_pt(content_width)),
            }
        } else {
            StaticInlinePlaceholderLogicalGeometry {
                inline_advance: LogicalInlineContentSize::new(content_box_pt(content_width)),
                block_extent: LogicalBlockContentSize::new(content_box_pt(
                    resolved_content_height.unwrap_or(placeholder_style.line_height),
                )),
            }
        };
        let atom_size = geometry.margin_box_inline_size(placeholder_style);
        let line_baseline_offset = if placeholder_style.display.is_atomic_inline()
            || placeholder_style.abspos_static_source.is_atomic_inline()
        {
            Self::inline_block_baseline_offset(
                placeholder_style,
                used_property_containment(element, placeholder_style).layout,
                atom_size.height,
                None,
            )
        } else {
            self.font_system
                .rendered_first_line_baseline_offset(placeholder_style)
                .points()
        };

        InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            placeholder_style.clone(),
            None,
            atom_size,
            line_baseline_offset,
            0.0,
            None,
            None,
        )
    }

    pub(in crate::layout) fn inline_static_position_from_placeholder_sequence(
        &mut self,
        element: &Element,
        sequence: &inline_layout::InlineLineSequence,
        block_style: &ComputedStyle,
    ) -> Option<StaticPositionCapture> {
        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let static_position_containing_block = self.current_static_position_containing_block();
        let (static_writing_mode, static_direction, static_justify_items, static_align_items) =
            static_position_containing_block.map_or(
                (
                    block_style.writing_mode,
                    block_style.used_direction(),
                    block_style.justify_items,
                    block_style.align_items,
                ),
                |context| {
                    (
                        context.axes.writing_mode(),
                        context.axes.direction(),
                        context.justify_items,
                        css::SelfAlignment::NORMAL,
                    )
                },
            );
        let context = sequence.context(block_style);
        let mut plaintext_direction_state = None;
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        for record in &records {
            if let Some(fragment) = &record.fragment && fragment.items().iter().any(|item| {
                matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                )
            }) {
                stack.apply(self);
                // A paintless RTL placeholder is emitted at the physical
                // edge selected by the float band. A left float leaves that
                // carrier at the band's left edge, while a right float leaves
                // it at the band's right edge. Recover the logical
                // inline-start from the selected line record, not from the
                // untrimmed block content span.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let rtl_placeholder_left_float_width =
                    self.float_contexts.last().and_then(|context| {
                        context
                            .shapes
                            .iter()
                            .rev()
                            .find(|shape| shape.side == UsedFloatSide::Left)
                            .map(|shape| shape.rect.width())
                    });
                let position = self
                    .prepare_inline_line_record(record, context, &mut plaintext_direction_state)
                    .and_then(|prepared| {
                        // The prepared line owns the canonical static
                        // baseline. Do not reconstruct it from the atom's
                        // border geometry: leading and font metrics can make
                        // that a different coordinate.
                        let baseline_y = self.cursor_y - prepared.metrics.baseline_offset;
                        prepared.paint_items.iter().find_map(|item| {
                            let PreparedInlinePaintItem::Atom(atom) = item else {
                                return None;
                            };
                            let horizontal_rtl = !block_style.writing_mode.has_vertical_lines()
                                && block_style.used_direction() == Direction::Rtl;
                            let logical_inline_start_x = if horizontal_rtl {
                                atom.border_box.x()
                                    + atom.border_box.width()
                                    + rtl_placeholder_left_float_width.unwrap_or(0.0)
                            } else {
                                // The inline static-position rectangle is
                                // anchored at the hypothetical box's content
                                // insertion edge. The absolute-position
                                // equation subsequently restores the
                                // positioned box's own padding and border;
                                // retaining them in both coordinates would
                                // apply inline-start non-content twice.
                                // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
                                atom.border_box.x()
                                    - atom.atom.style().padding.left
                                    - used_border_widths(atom.atom.style()).left
                            };
                            // CSS Position defines an inline static rectangle
                            // at the hypothetical box's logical inline-start.
                            // The selected edge belongs to the hypothetical
                            // box, whose direction can differ from the line
                            // formatting context; the rectangle is tagged
                            // separately with the static containing block's
                            // axes for late alignment.
                            // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
                            // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
                            let logical_inline_start_y = if atom.atom.style().writing_mode.has_vertical_lines() {
                                StaticInlinePlaceholderPageEdges::from_inline_paint_rect(
                                    atom.border_box,
                                )
                                .logical_inline_start_y(
                                    atom.atom.style().writing_mode,
                                    static_direction,
                                )
                            } else {
                                // Horizontal inline axes select a physical x
                                // edge below; their page-top y coordinate is
                                // already carried by the prepared atom.
                                atom.border_box.y()
                            };
                            // CSS 2 defines the RTL static `right` position
                            // from the hypothetical box's *right margin
                            // edge*. The prepared atom records the line's
                            // indented insertion edge; add its complete
                            // logical margin-box advance rather than
                            // substituting the static containing block's
                            // physical right edge. The latter happens to
                            // agree on an unindented line, but loses
                            // `text-indent` and bidi placement.
                            // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
                            let static_line_inline_start_x = if static_position_containing_block.is_some_and(
                                |context| {
                                    context.axes.physical_axis(LogicalAxis::Inline)
                                        == PhysicalAxis::Horizontal
                                        && context.axes.direction() == Direction::Rtl
                                },
                            )
                            {
                                logical_inline_start_x
                                    + inline_atom_logical_inline_size(&atom.atom, block_style)
                            } else {
                                logical_inline_start_x
                            };
                            let is_static_placeholder = matches!(
                                atom.atom.content(),
                                InlineAtomContent::StaticPositionPlaceholder
                            );
                            if is_static_placeholder {
                                log::trace!(
                                    target: "quire::layout::inline_static_verbose",
                                    "checkpoint=prepared-line element={:?} source=inline cursor_y={:.2} line=(width:{:.2},height:{:.2},baseline_offset:{:.2}) atom_axes=({:?},{:?}) block_axes=({:?},{:?}) atom_border=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) logical_inline_start=(x:{:.2},y:{:.2}) prepared_baseline_y={:.2}",
                                    element.id,
                                    self.cursor_y,
                                    prepared.metrics.width,
                                    prepared.metrics.height,
                                    prepared.metrics.baseline_offset,
                                    atom.atom.style().writing_mode,
                                    atom.atom.style().used_direction(),
                                    block_style.writing_mode,
                                    block_style.used_direction(),
                                    atom.border_box.x(),
                                    atom.border_box.y(),
                                    atom.border_box.width(),
                                    atom.border_box.height(),
                                    static_line_inline_start_x,
                                    logical_inline_start_y,
                                    baseline_y,
                                );
                            }
                            is_static_placeholder.then_some(StaticPositionCapture {
                                // The inline static-position rectangle has
                                // zero inline-axis thickness at the
                                // hypothetical box's logical inline-start.
                                // For horizontal RTL that is the atom's
                                // physical right edge, not its left edge.
                                // Keep both physical horizontal fallbacks at
                                // that one edge so CSS 2's RTL equation can
                                // select the right inset late.
                                // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
                                // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
                                rectangle: StaticPositionRectangle {
                                    area: if block_style.writing_mode.has_vertical_lines() {
                                        PageTopRect::new(
                                            atom.border_box.x(),
                                            logical_inline_start_y,
                                            record.height(),
                                            0.0,
                                        )
                                    } else {
                                        PageTopRect::new(
                                            static_line_inline_start_x,
                                            // The line-stack cursor is the
                                            // selected hypothetical line's
                                            // resolved block-start. A
                                            // paintless ordinary inline atom
                                            // is baseline-aligned within
                                            // that line, so its border top
                                            // can be one line advance later
                                            // and is not the static edge.
                                            if atom.atom.style().abspos_static_source.is_atomic_inline()
                                                && atom.atom.style().box_values.height.is_auto()
                                            {
                                                self.cursor_y + prepared.metrics.baseline_offset
                                            } else {
                                                self.cursor_y
                                            },
                                            0.0,
                                            record.height(),
                                        )
                                    },
                                    writing_mode: static_writing_mode,
                                    direction: static_direction,
                                    justify_items: static_justify_items,
                                    align_items: static_align_items,
                                },
                            })
                        })
                    });
                self.cursor_y = saved_cursor_y;
                self.content_left = saved_left;
                self.content_right = saved_right;
                return position;
            }
            // The following line is positioned after the trimmed line box's
            // paint-origin shift as well as its remaining line extent.
            stack.advance(record.height() + record.block_start_trim);
        }
        self.cursor_y = saved_cursor_y;
        self.content_left = saved_left;
        self.content_right = saved_right;
        None
    }

    pub(in crate::layout) fn collect_intrinsic_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        context: IntrinsicInlineCollectionContext<'_>,
        output: &mut Vec<InlineItem>,
    ) {
        for child in children {
            if let Some((_, _, style, _)) = child.element_parts()
                && (matches!(style.position, Position::Absolute | Position::Fixed)
                    || style.float != Float::None)
            {
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.core.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                let decoration_layers = propagated_decoration_layers_for_child(
                    &context.propagated_decoration_layers,
                    &box_.core.style,
                );
                self.collect_intrinsic_inline_box_items(
                    &box_.core.children,
                    stylesheets,
                    inherited_link.clone(),
                    context
                        .clone()
                        .with_block_style(&box_.core.style)
                        .with_propagated_decoration_layers(decoration_layers),
                    output,
                );
                if formatting_box_has_inline_content(&box_.core.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &context.propagated_decoration_layers,
                        &text_style,
                    );
                    apply_propagated_decoration_layers(&mut text_style, &decoration_layers);
                    self.push_inline_words(
                        &box_.text,
                        &text_style,
                        inherited_link.clone(),
                        context.baseline_shift,
                        context.visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let mut inline_style = box_tree::owned_style(&box_.core.style);
                    let decoration_layers = propagated_decoration_layers_for_child(
                        &context.propagated_decoration_layers,
                        &inline_style,
                    );
                    apply_propagated_decoration_layers(&mut inline_style, &decoration_layers);
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement =
                        InlinePlacement::new(context.baseline_shift, context.visual_offset)
                            .with_added_baseline_placement(
                                self.vertical_align_baseline_shift_for_inline_style(
                                    &inline_style,
                                    context.block_style,
                                ),
                            )
                            .with_added_visual_offset(
                                self.inline_visual_offset_for_style(&inline_style),
                            );
                    let scope = self.begin_inline_element_scope(
                        box_.core.element,
                        &inline_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::BOX_INTRINSIC
                            .with_fragment_edges(box_.fragment_edges)
                            .with_preserved_empty_metrics(empty_inline_scope_has_distinct_metrics(
                                context.block_style,
                                &inline_style,
                            )),
                        output,
                    );
                    let ruby_positioning_source = (inline_style.display.is_ruby()
                        || inline_style.display.is_ruby_internal())
                    .then(|| {
                        scope
                            .positioning_containing_block_source()
                            .map(BorrowedInlinePositioningContainingBlockSource::into_owned)
                    })
                    .flatten();
                    let inlinified_ruby_children = inline_style
                        .display
                        .is_ruby_internal()
                        .then(|| ruby::inlinified_direct_children(&box_.core.children));
                    let inline_children = inlinified_ruby_children
                        .as_deref()
                        .unwrap_or(&box_.core.children);
                    if inline_style.content.is_generated() {
                        let start_len = output.len();
                        self.push_intrinsic_element_content_items_from_boxes(
                            box_.core.element,
                            &inline_style.clone(),
                            inline_children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift(),
                            child_placement.visual_offset,
                            decoration_layers.clone(),
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.core.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.core.element,
                            clear,
                            output,
                            start_len,
                        );
                    } else {
                        self.collect_intrinsic_inline_box_items(
                            inline_children,
                            stylesheets,
                            link.clone(),
                            context
                                .clone()
                                .with_baseline_shift(child_placement.baseline_shift())
                                .with_visual_offset(child_placement.visual_offset)
                                .with_block_style(&inline_style.clone())
                                .with_propagated_decoration_layers(decoration_layers),
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                    // Intrinsic inline collection is also used to construct
                    // the retained item stream for inline formatting
                    // contexts. Ruby's generated empty counterparts can
                    // therefore hide an explicitly inset positioned child
                    // from the ordinary in-flow traversal. Replay it from
                    // the ruby role's completed inline scope, whose paired
                    // start/end edges define the containing block.
                    if let Some(source) = ruby_positioning_source.as_ref() {
                        self.layout_undeferred_ruby_positioned_descendants(
                            &box_.core.children,
                            stylesheets,
                            context.block_style,
                            source,
                            &[],
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, context.block_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.core.element, &box_.core.style);
                        self.push_inline_words(
                            &text,
                            &box_.core.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::Table(box_)
                    if box_.core.style.display.is_inline_level() =>
                {
                    let link = box_
                        .core
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.core.style));
                    let counter_snapshot = self.counter_set.clone();
                    let counter_scope =
                        self.begin_counter_scope(box_.core.element, &box_.core.style);
                    let atom = self.intrinsic_inline_atom_for_element(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        Some(&box_.fragment),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    );
                    self.end_counter_scope(counter_scope);
                    self.counter_set = counter_snapshot;
                    if let Some(mut atom) = atom {
                        atom.baseline_shift += self
                            .vertical_align_baseline_shift_for_atom(&atom, context.block_style)
                            .glyph_displacement()
                            .get();
                        output.push(InlineItem::Atom(Box::new(atom)));
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_intrinsic_inline_box_items(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        context
                            .clone()
                            .with_block_style(&box_.style)
                            .with_propagated_decoration_layers(
                                propagated_decoration_layers_for_child(
                                    &context.propagated_decoration_layers,
                                    &box_.style,
                                ),
                            ),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
                box_tree::FormattingBox::Table(_) => {}
            }
        }
    }
}

/// Return whether an already-collected item prevents a following ruby base
/// from being the block's first typographic letter.
fn inline_item_has_typographic_content(item: &InlineItem) -> bool {
    match item {
        InlineItem::Word(word) => !word.text.trim().is_empty(),
        InlineItem::Atom(atom) => !atom.content().is_inline_edge(),
        InlineItem::Float(_)
        | InlineItem::Break(_)
        | InlineItem::PageScopeStart(_)
        | InlineItem::PageScopeEnd => false,
    }
}

/// Whether a ruby subtree needs the generic inline scope so its positioned or
/// floated descendants retain their normal containing-block/float ownership.
/// Such descendants are excluded from ruby's anonymous base/annotation box
/// generation and therefore cannot be captured in the coupled paint atom.
fn ruby_has_out_of_flow_descendant(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| {
        if let Some((_, _, style, descendants)) = child.element_parts() {
            matches!(style.position, Position::Absolute | Position::Fixed)
                || style.float != Float::None
                || ruby_has_out_of_flow_descendant(descendants)
        } else {
            match child {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    ruby_has_out_of_flow_descendant(&box_.children)
                }
                box_tree::FormattingBox::Text(_) => false,
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    ruby_has_out_of_flow_descendant(&box_.core.children)
                }
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::Inline(_)
                | box_tree::FormattingBox::AtomicInline(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => false,
            }
        }
    })
}

/// Clone only the positioned/float branch of a ruby subtree for the generic
/// positioned-inline collector. The ruby formatter consumes in-flow bases and
/// annotations itself, but CSS Ruby does not remove out-of-flow descendants
/// from their normal containing-block and float ownership.
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
fn ruby_out_of_flow_overlay<'a>(box_: &box_tree::FormattingBox<'a>) -> box_tree::FormattingBox<'a> {
    fn has_out_of_flow_style(style: &ComputedStyle) -> bool {
        matches!(style.position, Position::Absolute | Position::Fixed) || style.float != Float::None
    }

    if box_
        .element_parts()
        .is_some_and(|(_, _, style, _)| has_out_of_flow_style(style))
    {
        return box_.clone();
    }

    match box_.clone() {
        box_tree::FormattingBox::Inline(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::Inline(box_)
        }
        box_tree::FormattingBox::Block(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::Block(box_)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::InlineSplitBlockContext(box_)
        }
        box_tree::FormattingBox::AnonymousBlock(mut box_) => {
            box_.children = box_
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::AnonymousBlock(box_)
        }
        box_ => box_,
    }
}

/// Materialize `::first-letter` inside the base level of a ruby container.
///
/// The generic graph pass receives a ruby container through transparent inline
/// edges. Preserve the pseudo's tree-abiding ownership at the ruby boundary
/// before its annotation levels are removed from the parent stream.
/// <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
fn apply_first_letter_style_to_ruby_base_items(
    output: &mut Vec<InlineItem>,
    first_letter_style: &ComputedStyle,
) {
    let Some(index) = output.iter().position(|item| {
        matches!(item, InlineItem::Word(word) if crate::layout::first_letter_byte_range(&word.text).is_some())
    }) else {
        return;
    };
    let InlineItem::Word(word) = &output[index] else {
        unreachable!("the selected ruby first-letter item is a word")
    };
    let range = crate::layout::first_letter_byte_range(&word.text)
        .expect("selected ruby word has a typographic first letter");
    let word = (**word).clone();
    let mut replacement = Vec::with_capacity(3);
    if range.start > 0 {
        let mut prefix = word.clone();
        prefix.text = word.text[..range.start].to_owned();
        replacement.push(InlineItem::Word(Box::new(prefix)));
    }
    let mut letter = word.clone();
    letter.text = word.text[range.clone()].to_owned();
    letter.style = Rc::new(first_letter_style.clone());
    letter.mergeable = false;
    replacement.push(InlineItem::Word(Box::new(letter)));
    if range.end < word.text.len() {
        let mut suffix = word;
        suffix.text = suffix.text[range.end..].to_owned();
        replacement.push(InlineItem::Word(Box::new(suffix)));
    }
    output.splice(index..=index, replacement);
}

fn ruby_line_sequence_inline_size(sequence: &inline_layout::InlineLineSequence) -> f32 {
    sequence
        .records
        .iter()
        .filter_map(|record| record.fragment.as_ref())
        .map(|fragment| fragment.metrics.width)
        .fold(0.0, f32::max)
}

/// Count typographic units eligible for the UA default `text-justify: ruby`
/// behavior. Ruby distributes only CJK-wide units; Latin and Bopomofo content
/// has no ruby justification opportunities and is therefore centered.
fn ruby_distribution_unit_count(items: &[InlineItem]) -> Option<usize> {
    let mut count = 0usize;
    for item in items {
        let InlineItem::Word(word) = item else {
            return None;
        };
        for range in crate::text::CursiveProtectedUnitRanges::new(&word.text) {
            let unit = &word.text[range];
            if !unit
                .chars()
                .filter(|character| {
                    !crate::text::character_is_unicode_mark(*character)
                        && !crate::text::character_is_unicode_control(*character)
                })
                .all(crate::text::character_is_ruby_justification_eligible)
            {
                return None;
            }
            count += 1;
        }
    }
    (count > 1).then_some(count)
}

/// Give all columns of one normalized ruby container a common block-axis
/// metric stack. CSS Ruby places annotation levels across the column group,
/// not independently inside each base. In particular, an anonymous empty
/// base must export the same base baseline as its non-empty siblings.
///
/// This runs after every column has been measured, while the columns are
/// still consecutive source items. The parent opportunity graph therefore
/// retains one base-level participant per column, but its line metrics see a
/// single coupled ruby level stack.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
fn normalize_ruby_column_group_metrics(
    ruby_atoms: &mut [InlineItem],
    containing_style: &ComputedStyle,
) {
    let mut base_block_size = 0.0f32;
    let mut annotation_block_sizes: Vec<f32> = Vec::new();
    let mut base_baseline = 0.0f32;

    for item in ruby_atoms.iter() {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let InlineAtomContent::Ruby {
            base_block_size: column_base_block_size,
            annotation_block_sizes: column_annotation_block_sizes,
            ..
        } = atom.content()
        else {
            continue;
        };
        base_block_size = base_block_size.max(*column_base_block_size);
        for (index, block_size) in column_annotation_block_sizes.iter().enumerate() {
            if annotation_block_sizes.len() <= index {
                annotation_block_sizes.push(0.0);
            }
            annotation_block_sizes[index] = annotation_block_sizes[index].max(*block_size);
        }
        base_baseline = base_baseline.max(
            atom.baseline_offset_from_alignment_source_block_start(
                inline_atom_logical_border_block_size(atom, containing_style),
                containing_style,
            )
            .points()
                - column_annotation_block_sizes.iter().sum::<f32>(),
        );
    }

    let base_metrics = ruby::RubyLevelMetrics {
        before_baseline: ruby::RubyBlockExtent::new(base_baseline),
        after_baseline: ruby::RubyBlockExtent::new((base_block_size - base_baseline).max(0.0)),
        baseline: ruby::RubyBaselineOffset::new(base_baseline),
    };
    let annotation_levels = annotation_block_sizes
        .iter()
        .copied()
        .map(|block_extent| ruby::RubyLevelMetrics {
            // Annotation sequences are replayed from their own line-box
            // baseline. Their group metric records the level extent here;
            // paint applies that local baseline exactly once.
            before_baseline: ruby::RubyBlockExtent::default(),
            after_baseline: ruby::RubyBlockExtent::new(block_extent),
            baseline: ruby::RubyBaselineOffset::default(),
        })
        .collect::<Vec<_>>();
    let annotations_block_extent = annotation_levels
        .iter()
        .map(|level| level.block_extent().points())
        .sum::<f32>();
    let metrics = ruby::RubyColumnGroupMetrics {
        base: base_metrics,
        annotation_levels,
        exported_baseline: ruby::RubyBaselineOffset::new(
            annotations_block_extent + base_metrics.baseline.points(),
        ),
    };
    let group_block_size = metrics.base.block_extent().points() + annotations_block_extent;
    for item in ruby_atoms {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let content = Rc::make_mut(&mut atom.data);
        let InlineAtomContent::Ruby {
            base_block_size: column_base_block_size,
            annotation_block_sizes: column_annotation_block_sizes,
            ..
        } = &mut content.content
        else {
            continue;
        };
        *column_base_block_size = metrics.base.block_extent().points();
        *column_annotation_block_sizes = metrics
            .annotation_levels
            .iter()
            .map(|level| level.block_extent().points())
            .collect();
        atom.size.height = group_block_size;
        atom.baseline = InlineAtomBaseline::Exported {
            source: InlineAtomBaselineSource::BorderBox,
            offset_from_source_box_block_start: atomic_inline_baseline_source_pt(
                metrics.exported_baseline.points(),
            ),
        };
    }
}

/// Assign the combined base-column width to annotations that begin a ruby
/// span. The parent graph retains separate base advances, while the sidecar
/// paints once from the first covered column across the complete paired range.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-annotation-pairing>
fn normalize_ruby_annotation_span_inline_sizes(
    ruby_atoms: &mut [InlineItem],
    containing_style: &ComputedStyle,
) {
    let column_inline_sizes = ruby_atoms
        .iter()
        .filter_map(|item| {
            let InlineItem::Atom(atom) = item else {
                return None;
            };
            matches!(atom.content(), InlineAtomContent::Ruby { .. })
                .then(|| inline_atom_logical_border_inline_size(atom, containing_style))
        })
        .collect::<Vec<_>>();

    for (column_index, item) in ruby_atoms.iter_mut().enumerate() {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let content = Rc::make_mut(&mut atom.data);
        let InlineAtomContent::Ruby { annotations, .. } = &mut content.content else {
            continue;
        };
        for annotation in annotations {
            if annotation.starts_span && annotation.column_span > 1 {
                annotation.containing_inline_size = ruby::RubyColumnInlineSpan::new(
                    column_inline_sizes[column_index..column_index + annotation.column_span]
                        .iter()
                        .sum(),
                );
            }
        }
    }
}

/// Produce the anonymous replaced-content style inside an inline generated
/// pseudo-element.
///
/// A `content: url(...)` item is the child of the tree-abiding pseudo-element,
/// not a replacement of that pseudo-element itself. Its parent owns the
/// pseudo's box decoration. The generated inline atom retains the pseudo's
/// background so a sole image paints inside its decorated pseudo box, but it
/// must not copy box edges that would size the payload as the pseudo's border
/// box or paint a border twice:
/// <https://www.w3.org/TR/css-content-3/#content-property> and
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>.
fn generated_pseudo_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut content_style = style.clone();
    content_style.margin = css::Edges::ZERO;
    content_style.ua_margin_em = css::OptionalEdges::NONE;
    content_style.box_values.margin = css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
    content_style.padding = css::Edges::ZERO;
    content_style.box_values.padding = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_width = 0.0;
    content_style.border_widths = css::Edges::ZERO;
    content_style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_styles = css::BorderStyles::NONE;
    content_style.border_radius = css::BorderRadius::ZERO;
    content_style.corner_shapes = css::CornerShapes::ROUND;
    content_style.border_image = css::BorderImage::initial();
    content_style
}

pub(in crate::layout) fn annotate_line_break_element_breaks(
    element: &Element,
    style: &ComputedStyle,
    output: &mut [InlineItem],
    start_len: usize,
) {
    annotate_line_break_element_breaks_with_clear(element, style.clear, output, start_len);
}

pub(in crate::layout) fn annotate_line_break_element_breaks_with_clear(
    element: &Element,
    clear: Clear,
    output: &mut [InlineItem],
    start_len: usize,
) {
    if !is_line_break_element(element) || clear == Clear::None {
        return;
    }
    for item in output.iter_mut().skip(start_len) {
        match item {
            InlineItem::Break(break_) => break_.clear = clear,
            InlineItem::Word(word) if word.source.is_generated() => {
                std::rc::Rc::make_mut(&mut word.style).clear = clear;
            }
            _ => {}
        }
    }
}

fn generated_content_originating_clear(source: &box_tree::BoxSource<'_>) -> Option<Clear> {
    match source {
        box_tree::BoxSource::GeneratedPseudo(pseudo) => Some(pseudo.originating_clear),
        box_tree::BoxSource::Principal => None,
    }
}

fn inline_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logical placeholder geometry is projected once at the inline-layout
    /// boundary.  This keeps a vertical source's text advance (physical
    /// height) distinct from its logical block extent (physical width).
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    #[test]
    fn static_inline_placeholder_projects_logical_axes_for_all_writing_modes() {
        let geometry = StaticInlinePlaceholderLogicalGeometry {
            inline_advance: LogicalInlineContentSize::new(content_box_pt(80.0)),
            block_extent: LogicalBlockContentSize::new(content_box_pt(16.0)),
        };

        for (writing_mode, expected_size) in [
            (WritingMode::HorizontalTb, InlineSize::new(80.0, 16.0)),
            (WritingMode::VerticalLr, InlineSize::new(16.0, 80.0)),
            (WritingMode::VerticalRl, InlineSize::new(16.0, 80.0)),
            (WritingMode::SidewaysLr, InlineSize::new(16.0, 80.0)),
            (WritingMode::SidewaysRl, InlineSize::new(16.0, 80.0)),
        ] {
            let mut style = ComputedStyle::initial();
            style.writing_mode = writing_mode;
            assert_eq!(geometry.margin_box_inline_size(&style), expected_size);
        }
    }

    #[test]
    fn static_inline_placeholder_selects_page_edges_from_inline_paint_coordinates() {
        let edges =
            StaticInlinePlaceholderPageEdges::from_inline_paint_rect(PhysicalInlineRect::new(
                InlineRect::new(InlinePoint::new(12.0, 40.0), InlineSize::new(16.0, 80.0)),
            ));

        assert_eq!(
            edges.logical_inline_start_y(WritingMode::VerticalLr, Direction::Ltr),
            120.0
        );
        assert_eq!(
            edges.logical_inline_start_y(WritingMode::VerticalRl, Direction::Rtl),
            40.0
        );
        assert_eq!(
            edges.logical_inline_start_y(WritingMode::SidewaysLr, Direction::Ltr),
            40.0
        );
        assert_eq!(
            edges.logical_inline_start_y(WritingMode::SidewaysRl, Direction::Rtl),
            40.0
        );
    }

    #[test]
    fn block_static_placeholder_recovers_the_measured_margin_box_from_its_block_end_marker() {
        let geometry = BlockStaticPositionPlaceholderGeometry::Vertical {
            physical_margin_box_block_extent: margin_box_pt(16.0),
        };

        for (writing_mode, expected_left) in [
            (WritingMode::VerticalLr, 84.0),
            (WritingMode::VerticalRl, 100.0),
            (WritingMode::SidewaysLr, 84.0),
            (WritingMode::SidewaysRl, 100.0),
        ] {
            let span =
                geometry.vertical_margin_box_inline_span_from_block_end_marker(100.0, writing_mode);
            assert_eq!(span.left_x(), expected_left);
            assert_eq!(span.width(), 16.0);
        }
    }

    #[test]
    fn block_static_rectangle_preserves_a_relative_ancestor_inline_translation() {
        let hypothetical = HypotheticalBlockMarginBox::from_placeholder(
            PageTopRect::new(20.0, 30.0, 16.0, 40.0),
            InlineVisualOffset {
                vector: InlineVector::new(2.0, 3.0),
            },
        );
        let vertical = StaticPositionContainingBlock::new(
            WritingModeAxes::new(WritingMode::VerticalRl, Direction::Ltr),
            PageTopRect::new(10.0, 100.0, 80.0, 200.0),
            css::SelfAlignment::NORMAL,
        );
        let vertical_area = hypothetical.static_rectangle(vertical).area;
        assert_eq!(vertical_area.x(), 38.0);
        assert_eq!(vertical_area.top_y(), 103.0);
        assert_eq!(vertical_area.height(), 200.0);

        let horizontal = StaticPositionContainingBlock::new(
            WritingModeAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            PageTopRect::new(10.0, 100.0, 80.0, 200.0),
            css::SelfAlignment::NORMAL,
        );
        let horizontal_area = hypothetical.static_rectangle(horizontal).area;
        assert_eq!(horizontal_area.x(), 12.0);
        assert_eq!(horizontal_area.top_y(), 33.0);
        assert_eq!(horizontal_area.width(), 80.0);
    }

    /// A vertical-rl inline's block-start edge is physical right, while its
    /// inline-start edge is physical top. The containing block must select
    /// those first-fragment edges and the matching end-fragment left/bottom
    /// edges, rather than treating either source rectangle as a physical
    /// bounding-box contribution.
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    /// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
    #[test]
    fn vertical_rl_inline_containing_block_uses_logical_fragment_edges() {
        let containing_block = InlineContainingBlockContentEdges {
            first_fragment: PageTopRect::new(40.0, 100.0, 20.0, 30.0),
            end_fragment: PageTopRect::new(10.0, 60.0, 15.0, 10.0),
            axes: WritingModeAxes::new(WritingMode::VerticalRl, Direction::Ltr),
        }
        .to_containing_block();

        assert_eq!(
            containing_block.rect,
            PageTopRect::new(10.0, 100.0, 50.0, 50.0)
        );
    }
}
