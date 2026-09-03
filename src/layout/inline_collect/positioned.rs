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
    pub(super) box_source: DeferredPositionedBoxSource,
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
    pub(super) box_source: DeferredPositionedBoxSource,
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

/// Owned provenance for an element-backed positioned formatting box.
///
/// A generated pseudo borrows its originating element in the box tree, so a
/// deferred record cannot retain `BoxSource` directly. This compact owned
/// form preserves the semantic box role needed for generated content,
/// counters, and stable static-position source identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::layout) enum DeferredPositionedBoxSource {
    Principal,
    GeneratedPseudo(box_tree::GeneratedPseudoKind),
}

impl DeferredPositionedBoxSource {
    pub(in crate::layout) fn from_box_source(source: &box_tree::BoxSource<'_>) -> Self {
        match source {
            box_tree::BoxSource::Principal => Self::Principal,
            box_tree::BoxSource::GeneratedPseudo(pseudo) => Self::GeneratedPseudo(pseudo.kind),
        }
    }
}

/// The immutable normal-flow provenance for a deferred positioned source.
///
/// Inline sources use an element-stable marker that survives line processing.
/// A block source remains on the older source-order path until its distinct
/// block-in-inline split marker can be materialized by block layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::layout) enum DeferredStaticPositionSource {
    InlineSource(InlineStaticPositionSourceId),
    LegacyBlockSourceOrderIndex(usize),
}

/// The two logical corners that form an inline absolute-positioning containing
/// block. Keeping coordinates associated with their logical axes avoids
/// inventing a bounding rectangle for disjoint bidi fragments.
/// <https://drafts.csswg.org/css-position-3/#def-cb>
#[derive(Debug, Clone, Copy)]
struct InlineContainingBlockLogicalCorners {
    start: InlinePositioningLogicalCorner,
    end: InlinePositioningLogicalCorner,
    axes: WritingModeAxes,
}

impl InlineContainingBlockLogicalCorners {
    /// Form the positioned containing block from the first fragment's
    /// start-most content edges and the end-most fragment's end-most edges.
    ///
    /// This is the only adapter that projects those logical edges into a
    /// physical `PageTopRect`; callers retain a `ContainingBlock` rather than
    /// recombining page coordinates themselves.
    /// <https://drafts.csswg.org/css-position-3/#def-cb>
    fn to_containing_block(self) -> ContainingBlock {
        let (first_x, first_y) = self.physical_coordinates(self.start);
        let (end_x, end_y) = self.physical_coordinates(self.end);

        let left = first_x.min(end_x);
        let right = first_x.max(end_x);
        let bottom = first_y.min(end_y);
        let top = first_y.max(end_y);
        ContainingBlock::from_page_top_rect(PageTopRect::new(left, top, right - left, top - bottom))
    }

    fn physical_coordinates(self, corner: InlinePositioningLogicalCorner) -> (f32, f32) {
        let inline = corner.inline.physical_page_coordinate();
        let block = corner.block.physical_page_coordinate();
        if self.axes.physical_axis(LogicalAxis::Inline) == PhysicalAxis::Horizontal {
            (inline, block)
        } else {
            debug_assert_eq!(
                self.axes.physical_axis(LogicalAxis::Block),
                PhysicalAxis::Horizontal
            );
            (block, inline)
        }
    }
}

/// Reduce source-keyed prepared fragment geometry to the two logical corners
/// required by CSS Positioned Layout.
///
/// The producer can be hypothetical line replay today or committed
/// fragmentainer layout later; selection depends only on prepared geometry.
/// <https://drafts.csswg.org/css-position-3/#def-cb>
#[derive(Debug)]
struct PreparedInlinePositioningGeometryReducer {
    source: InlinePositioningContainingBlockId,
    axes: WritingModeAxes,
    first_fragment_start: Option<InlinePositioningLogicalCorner>,
    last_fragment_end: Option<InlinePositioningLogicalCorner>,
}

impl PreparedInlinePositioningGeometryReducer {
    fn new(source: InlinePositioningContainingBlockId, axes: WritingModeAxes) -> Self {
        Self {
            source,
            axes,
            first_fragment_start: None,
            last_fragment_end: None,
        }
    }

    fn observe_line(&mut self, line: &PreparedInlineLine) {
        let Some(geometry) = line
            .positioning_geometry
            .iter()
            .find(|geometry| geometry.source == self.source)
        else {
            return;
        };
        debug_assert_eq!(geometry.axes, self.axes);
        if geometry.start_marker.is_some() {
            self.first_fragment_start
                .get_or_insert_with(|| geometry.start_corner().expect("marker supplies a corner"));
        }
        if geometry.end_marker.is_some() {
            self.last_fragment_end = geometry.end_corner();
        }
    }

    fn finish(self) -> Option<InlineContainingBlockLogicalCorners> {
        Some(InlineContainingBlockLogicalCorners {
            start: self.first_fragment_start?,
            end: self.last_fragment_end?,
            axes: self.axes,
        })
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn prepared_line(geometry: PreparedInlinePositioningGeometry) -> PreparedInlineLine {
        PreparedInlineLine {
            metrics: InlineLineMetrics {
                width: 0.0,
                height: 0.0,
                baseline_offset: 0.0,
            },
            // Positioning geometry is deliberately independent of paint scope
            // nesting, so the reducer has no paint tree to inspect.
            paint_items: Vec::new(),
            positioning_geometry: vec![geometry],
            decoration_origin_fragments: Default::default(),
        }
    }

    fn geometry_with_marker(
        axes: WritingModeAxes,
        edge: InlineLogicalEdge,
        content: PageTopRect,
    ) -> PreparedInlinePositioningGeometry {
        let mut geometry =
            PreparedInlinePositioningGeometry::new(InlinePositioningContainingBlockId(1), axes);
        geometry.record_content_rect(content);
        geometry.record_marker(edge, content);
        geometry
    }

    fn reduced_rect(writing_mode: WritingMode, direction: Direction) -> PageTopRect {
        let axes = WritingModeAxes::new(writing_mode, direction);
        let mut reducer = PreparedInlinePositioningGeometryReducer::new(
            InlinePositioningContainingBlockId(1),
            axes,
        );
        reducer.observe_line(&prepared_line(geometry_with_marker(
            axes,
            InlineLogicalEdge::Start,
            PageTopRect::new(40.0, 100.0, 20.0, 30.0),
        )));
        reducer.observe_line(&prepared_line(geometry_with_marker(
            axes,
            InlineLogicalEdge::End,
            PageTopRect::new(10.0, 60.0, 15.0, 10.0),
        )));
        reducer.finish().unwrap().to_containing_block().rect
    }

    #[test]
    fn reducer_selects_vertical_and_sideways_logical_corners() {
        let cases = [
            (
                WritingMode::VerticalRl,
                Direction::Ltr,
                PageTopRect::new(10.0, 100.0, 50.0, 50.0),
            ),
            (
                WritingMode::VerticalRl,
                Direction::Rtl,
                PageTopRect::new(10.0, 70.0, 50.0, 10.0),
            ),
            (
                WritingMode::VerticalLr,
                Direction::Ltr,
                PageTopRect::new(25.0, 100.0, 15.0, 50.0),
            ),
            (
                WritingMode::VerticalLr,
                Direction::Rtl,
                PageTopRect::new(25.0, 70.0, 15.0, 10.0),
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Ltr,
                PageTopRect::new(10.0, 100.0, 50.0, 50.0),
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Rtl,
                PageTopRect::new(10.0, 70.0, 50.0, 10.0),
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Ltr,
                PageTopRect::new(25.0, 70.0, 15.0, 10.0),
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Rtl,
                PageTopRect::new(25.0, 100.0, 15.0, 50.0),
            ),
        ];

        for (writing_mode, direction, expected) in cases {
            assert_eq!(
                reduced_rect(writing_mode, direction),
                expected,
                "{writing_mode:?} {direction:?}"
            );
        }
    }

    #[test]
    fn prepared_horizontal_ltr_geometry_selects_independent_logical_extrema() {
        let axes = WritingModeAxes::new(WritingMode::HorizontalTb, Direction::Ltr);
        let mut geometry =
            PreparedInlinePositioningGeometry::new(InlinePositioningContainingBlockId(1), axes);
        geometry.record_content_rect(PageTopRect::new(40.0, 100.0, 20.0, 30.0));
        geometry.record_content_rect(PageTopRect::new(10.0, 90.0, 15.0, 50.0));

        assert_eq!(
            geometry.start_corner(),
            Some(InlinePositioningLogicalCorner {
                inline: InlinePositioningInlineCoordinate::new(10.0),
                block: InlinePositioningBlockCoordinate::new(100.0),
            })
        );
        assert_eq!(
            geometry.end_corner(),
            Some(InlinePositioningLogicalCorner {
                inline: InlinePositioningInlineCoordinate::new(60.0),
                block: InlinePositioningBlockCoordinate::new(40.0),
            })
        );
    }

    #[test]
    fn prepared_horizontal_rtl_geometry_reverses_inline_extrema() {
        let axes = WritingModeAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let mut geometry =
            PreparedInlinePositioningGeometry::new(InlinePositioningContainingBlockId(1), axes);
        geometry.record_content_rect(PageTopRect::new(40.0, 100.0, 20.0, 30.0));
        geometry.record_content_rect(PageTopRect::new(10.0, 90.0, 15.0, 50.0));

        assert_eq!(
            geometry.start_corner().unwrap().inline,
            InlinePositioningInlineCoordinate::new(60.0)
        );
        assert_eq!(
            geometry.end_corner().unwrap().inline,
            InlinePositioningInlineCoordinate::new(10.0)
        );
    }

    #[test]
    fn prepared_positioning_geometry_uses_marker_only_without_content() {
        let axes = WritingModeAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let mut geometry =
            PreparedInlinePositioningGeometry::new(InlinePositioningContainingBlockId(1), axes);
        geometry.record_marker(
            InlineLogicalEdge::Start,
            PageTopRect::new(80.0, 100.0, 0.0, 20.0),
        );
        geometry.record_marker(
            InlineLogicalEdge::End,
            PageTopRect::new(30.0, 80.0, 0.0, 20.0),
        );

        let line = prepared_line(geometry);
        let mut reducer = PreparedInlinePositioningGeometryReducer::new(
            InlinePositioningContainingBlockId(1),
            axes,
        );
        reducer.observe_line(&line);
        let corners = reducer.finish().unwrap();
        assert_eq!(
            corners.start.inline,
            InlinePositioningInlineCoordinate::new(80.0)
        );
        assert_eq!(
            corners.end.inline,
            InlinePositioningInlineCoordinate::new(30.0)
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
        box_source: DeferredPositionedBoxSource,
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
        if let DeferredPositionedBoxSource::GeneratedPseudo(kind) = box_source {
            let counter_scope =
                self.begin_pseudo_counter_scope(element, kind.counter_event_source(), style);
            self.element_side_effect_suppression_depth += 1;
            let previous_positioned_generated_source = self.positioned_generated_source;
            self.positioned_generated_source = Some(
                InlineStaticPositionSourceId::for_generated_pseudo(element, kind),
            );
            let mut generated_content_style;
            let style = if matches!(
                style.content,
                css::Content::Replacement {
                    image: css::GeneratedContentPart::Image { .. },
                    ..
                }
            ) {
                generated_content_style = style.clone();
                generated_content_style.object_fit = css::ObjectFit::None;
                generated_content_style.object_position = css::BackgroundPosition::INITIAL;
                &generated_content_style
            } else {
                style
            };
            self.layout_positioned_inline_descendant(
                element,
                style,
                DeferredPositionedBoxSource::Principal,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                static_position_container_style,
                positioning_containing_block_source,
                static_position_containing_block,
                static_position_source,
                hypothetical_ancestor_offset,
                output,
            );
            self.positioned_generated_source = previous_positioned_generated_source;
            self.element_side_effect_suppression_depth -= 1;
            self.end_counter_scope(counter_scope);
            return;
        }
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
            let mut static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                match static_position_source {
                    Some(DeferredStaticPositionSource::InlineSource(marker)) => marker,
                    Some(DeferredStaticPositionSource::LegacyBlockSourceOrderIndex(_)) | None => {
                        InlineStaticPositionSourceId::for_element(element)
                    }
                },
                static_position_source.is_none(),
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
                    DeferredStaticPositionSource::InlineSource(_) => None,
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
                Some(self.push_positioned_containing_block(mode, containing_block))
            });
        self.out_of_flow_prebreak_suppression_depth += 1;
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.out_of_flow_prebreak_suppression_depth -= 1;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
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

    /// Resolves the content-edge rectangle established by a positioned inline.
    ///
    /// Inline collection retains zero-advance edge atoms for positioned
    /// ancestors. Replaying those source markers through normal line
    /// preparation gives the first and last generated inline fragments their
    /// final physical coordinates, including bidi reordering, fragmentation,
    /// and writing-mode transforms. CSS Positioned Layout defines the absolute
    /// containing block from the start-most content edges of the first fragment
    /// and the end-most content edges of the end-most fragment:
    /// <https://drafts.csswg.org/css-position-3/#def-cb>.
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
        let source_axes =
            WritingModeAxes::new(source.style.writing_mode, source.style.used_direction());
        let mut reducer = PreparedInlinePositioningGeometryReducer::new(source.id, source_axes);
        for record in &records {
            let placement = stack.place_line(record);
            placement.apply(self);
            self.apply_line_block_start_trim_for_paint(record, block_style.writing_mode);
            if let Some(prepared) = self.prepare_inline_line_record(record, context) {
                reducer.observe_line(&prepared);
            }
        }
        self.cursor_y = saved_cursor_y;
        self.content_left = saved_left;
        self.content_right = saved_right;

        let containing_block_edges = reducer.finish()?;
        let trace_start = containing_block_edges.physical_coordinates(containing_block_edges.start);
        let trace_end = containing_block_edges.physical_coordinates(containing_block_edges.end);
        let containing_block = containing_block_edges.to_containing_block();
        let containing_rect = containing_block.rect;
        log::trace!(
            target: "quire::layout::static_position",
            "checkpoint=positioned-inline-containing-block source={:?} axes=({:?},{:?}) start=(x:{:.2},y:{:.2}) end=(x:{:.2},y:{:.2}) containing_block=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
            source.id,
            source.style.writing_mode,
            source.style.used_direction(),
            trace_start.0,
            trace_start.1,
            trace_end.0,
            trace_end.1,
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
                let child_boxes = match descendant.box_source {
                    DeferredPositionedBoxSource::Principal => layout
                        .build_frozen_child_boxes_with_current_ancestors(
                            &descendant.element,
                            stylesheets,
                            &descendant.style,
                        ),
                    DeferredPositionedBoxSource::GeneratedPseudo(_) => Vec::new(),
                };
                let positioned_layer_start = layout.positioned_layers.len();
                layout.layout_positioned_inline_descendant(
                    &descendant.element,
                    &descendant.style,
                    descendant.box_source,
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
                let frozen_child_boxes = match (descendant.content, descendant.box_source) {
                    (DeferredStaticPositionedContent::Dom, _) => None,
                    (
                        DeferredStaticPositionedContent::Frozen,
                        DeferredPositionedBoxSource::Principal,
                    ) => Some(layout.build_frozen_child_boxes_with_current_ancestors(
                        &descendant.element,
                        stylesheets,
                        &descendant.style,
                    )),
                    (
                        DeferredStaticPositionedContent::Frozen,
                        DeferredPositionedBoxSource::GeneratedPseudo(_),
                    ) => Some(Vec::new()),
                };
                layout.layout_positioned_inline_descendant(
                    &descendant.element,
                    &descendant.style,
                    descendant.box_source,
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
                        DeferredPositionedBoxSource::from_box_source(
                            &child
                                .element_core()
                                .expect("an element-backed formatting box has a source")
                                .source,
                        ),
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
