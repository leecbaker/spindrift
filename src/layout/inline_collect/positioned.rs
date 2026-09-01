use super::collection::positioned_descendant_has_explicit_inset;
use super::static_position::{HypotheticalBlockMarginBox, StaticHypotheticalBox};
use super::*;
use crate::layout::block::children::shared::PositionedAutoSizeChildParticipation;
use crate::layout::inline_layout::InlineLineStackCursor;
use crate::units::content_box_to_margin_box_length;

#[derive(Clone)]
pub(super) struct DeferredInlinePositionedDescendant {
    pub(super) element: Element,
    pub(super) signature: ElementSignature,
    pub(super) style: ComputedStyle,
    /// The inline source whose hypothetical-flow geometry defines the
    /// static-position rectangle. This is distinct from the enclosing block
    /// formatting context used to lay out the descendant itself.
    pub(super) static_position_container_style: ComputedStyle,
    pub(super) containing_block_source: InlinePositioningContainingBlockSource,
}

/// An inline-level positioned descendant whose static-position rectangle
/// cannot be selected until the enclosing source stream has supplied the
/// complete line.  The record keeps the DOM/style boundary immutable while
/// delaying only the geometry-dependent positioned layout.
///
/// The marker is a source-owned boundary, rather than an atom pointer or
/// source-order index: line breaking may copy, split, or bidi-reorder the
/// collected items before the selected line is materialized.
/// <https://drafts.csswg.org/css-position-3/#static-position>
#[derive(Clone)]
pub(super) struct DeferredInlineStaticPositionedDescendant {
    pub(super) element: Element,
    /// Selector ancestry of the positioned source at collection time. The
    /// deferred replay can run after that source scope has unwound, but its
    /// descendants must still match child and descendant selectors against
    /// this element while resolving automatic sizes and final layout.
    pub(super) signature: ElementSignature,
    pub(super) style: ComputedStyle,
    /// The block formatting context that selected the hypothetical line.
    /// This is deliberately distinct from the lexical inline ancestor: the
    /// latter may reset `text-indent`, direction, or writing mode without
    /// changing the line box that defines an inline static-position
    /// rectangle.
    pub(super) line_formatting_context_style: ComputedStyle,
    pub(super) static_position_container_style: ComputedStyle,
    /// The block formatting context that owns the hypothetical source. It is
    /// captured at source order because deferred replay may occur after an
    /// anonymous-inline split has changed the active builder context.
    pub(super) static_position_containing_block: Option<StaticPositionContainingBlock>,
    /// The nearest positioned inline establishes the actual absolute
    /// containing block. It is independent of the static-position
    /// containing block retained above, and therefore must survive deferred
    /// hypothetical-line replay even when every inset is `auto`.
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    pub(super) positioning_containing_block_source: Option<InlinePositioningContainingBlockSource>,
    /// Relative offsets do not affect normal-flow line fitting, but they do
    /// affect the hypothetical box's final page position.
    pub(super) hypothetical_ancestor_offset: InlineVisualOffset,
    pub(super) content: DeferredStaticPositionedContent,
    pub(super) static_position_source: DeferredStaticPositionSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]

pub(super) enum DeferredStaticPositionedContent {
    Dom,
    Frozen,
}

/// The immutable normal-flow provenance for a deferred positioned source.
///
/// Inline sources use an element-stable marker that survives line processing.
/// A block source remains on the older source-order path until its distinct
/// block-in-inline split marker can be materialized by block layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::layout) enum DeferredStaticPositionSource {
    InlineMarker {
        marker: InlineStaticPositionMarkerId,
        fallback_source_order_index: usize,
    },
    LegacyBlockSourceOrderIndex(usize),
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

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

impl<'a> LayoutBuilder<'a> {
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
        static_position_source: Option<DeferredStaticPositionSource>,
        hypothetical_ancestor_offset: InlineVisualOffset,
        output: &[InlineItem],
    ) {
        if self.positioned_auto_size_child_participation(style)
            == PositionedAutoSizeChildParticipation::ExcludeOutOfFlow
        {
            // A block container with only positioned block children is
            // represented by the inline collector. Reject the descendant at
            // this boundary before hypothetical placeholder measurement can
            // contribute line boxes to the positioned parent's auto-size
            // probe. The committed pass still constructs its static rectangle.
            return;
        }
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
            let mut static_position =
                self.inline_static_position_from_hypothetical_placeholder(
                    element,
                    &positioned_style,
                    stylesheets,
                    child_boxes,
                    table_fragment,
                    block_style,
                    match static_position_source {
                        Some(DeferredStaticPositionSource::InlineMarker { marker, .. }) => marker,
                        Some(DeferredStaticPositionSource::LegacyBlockSourceOrderIndex(_))
                        | None => InlineStaticPositionMarkerId::for_element(element),
                    },
                    match static_position_source {
                        Some(DeferredStaticPositionSource::InlineMarker {
                            fallback_source_order_index,
                            ..
                        }) => Some(fallback_source_order_index),
                        Some(DeferredStaticPositionSource::LegacyBlockSourceOrderIndex(_))
                        | None => None,
                    },
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
                "checkpoint=deferred-replay element={:?} source=inline marker={:?} static_axes=({:?},{:?}) rect=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
                element.id,
                static_position_source,
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
                static_position_source.and_then(|source| match source {
                    DeferredStaticPositionSource::InlineMarker { .. } => None,
                    DeferredStaticPositionSource::LegacyBlockSourceOrderIndex(index) => Some(index),
                }),
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
            "checkpoint=capture element={:?} source=block hypothetical=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) static_axes=({:?},{:?}) rect=(x:{:.2},top:{:.2},width:{:.2},height:{:.2}) buffered_block_offset={:.2} containing_inline={:?}",
            element.id,
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
            .block_percentage_context_stack
            .current_percentage_basis();
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
            if let Some(prepared) = self.prepare_inline_line_record(record, context) {
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

    pub(super) fn layout_deferred_inline_positioned_descendants(
        &mut self,
        descendants: Vec<DeferredInlinePositionedDescendant>,
        stylesheets: &Stylesheets<'_>,
        block_style: &ComputedStyle,
        output: &mut [InlineItem],
    ) {
        for descendant in descendants {
            let layers = self.with_ancestor_signature(descendant.signature, |layout| {
                // Rebuild only at the final inline edge, where its ancestor's
                // containing block can be measured from a complete item
                // stream. Retain the positioned source on the selector stack
                // throughout both child-box construction and layout.
                let child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                    &descendant.element,
                    stylesheets,
                    &descendant.style,
                );
                let positioned_layer_start = layout.positioned_layers.len();
                layout.layout_positioned_inline_descendant(
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
                layout.positioned_layers.split_off(positioned_layer_start)
            });
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
    pub(super) fn layout_deferred_inline_static_positioned_descendants(
        &mut self,
        descendants: Vec<DeferredInlineStaticPositionedDescendant>,
        stylesheets: &Stylesheets<'_>,
        output: &[InlineItem],
    ) {
        for descendant in descendants {
            self.with_ancestor_signature(descendant.signature, |layout| {
                let frozen_child_boxes =
                    matches!(descendant.content, DeferredStaticPositionedContent::Frozen).then(
                        || {
                            layout.build_frozen_child_boxes_with_current_ancestors(
                                &descendant.element,
                                stylesheets,
                                &descendant.style,
                            )
                        },
                    );
                layout.layout_positioned_inline_descendant(
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
                    Some(descendant.static_position_source),
                    descendant.hypothetical_ancestor_offset,
                    output,
                );
            });
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
    pub(super) fn layout_undeferred_ruby_positioned_descendants(
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
}
