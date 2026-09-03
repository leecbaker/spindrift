use super::*;
use crate::layout::inline_layout::InlineLineStackCursor;

fn static_position_hypothetical_has_source(
    content: &InlineAtomContent,
    source: InlineStaticPositionSourceId,
) -> bool {
    matches!(
        content,
        InlineAtomContent::StaticPositionHypothetical {
            source: candidate,
            ..
        } if *candidate == source
    )
}

/// The physical block-axis extent needed to capture a block-level
/// static-position source from an inline collection.
///
/// In a vertical containing flow, physical width is the source's logical
/// block-size.  The line-selection marker has no such extent of its own, so
/// carrying the real hypothetical margin-box extent prevents it from being
/// replaced by an unrelated line-height approximation.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum BlockStaticPositionPlaceholderGeometry {
    Horizontal,
    Vertical {
        physical_margin_box_block_extent: MarginBoxLength,
    },
}

impl BlockStaticPositionPlaceholderGeometry {
    /// Place a vertical hypothetical margin box at the containing flow's
    /// logical block-start edge.
    fn vertical_margin_box_inline_span_at_block_start(
        self,
        block_start_x: f32,
        writing_mode: WritingMode,
    ) -> PageInlineSpan {
        let Self::Vertical {
            physical_margin_box_block_extent,
        } = self
        else {
            unreachable!("a horizontal static placeholder has no physical block span");
        };
        let width = physical_margin_box_block_extent.points();
        let left = match WritingModeAxes::new(writing_mode, Direction::Ltr)
            .physical_side(LogicalSide::BlockStart)
        {
            PhysicalSide::Left => block_start_x,
            PhysicalSide::Right => block_start_x - width,
            PhysicalSide::Top | PhysicalSide::Bottom => {
                unreachable!("a vertical writing mode has a horizontal block axis")
            }
        };
        PageInlineSpan::new(left, width)
    }

    /// Recover the vertical source's physical margin-box span from the
    /// zero-footprint line marker at its logical block-end.
    ///
    /// The marker is an edge, not the source box.  `vertical-lr` advances its
    /// block axis to the physical right, so its block-start lies one measured
    /// margin-box extent to the marker's left.  `vertical-rl` advances to the
    /// physical left, so the same marker is the span's left edge.
    fn vertical_margin_box_inline_span_from_block_end_marker(
        self,
        marker_block_end_x: f32,
        writing_mode: WritingMode,
    ) -> PageInlineSpan {
        let Self::Vertical {
            physical_margin_box_block_extent,
            ..
        } = self
        else {
            unreachable!(
                "a vertical block static-position marker requires a measured physical block extent"
            );
        };
        let width = physical_margin_box_block_extent.points();
        let left = match writing_mode {
            WritingMode::VerticalLr | WritingMode::SidewaysLr => marker_block_end_x - width,
            WritingMode::VerticalRl | WritingMode::SidewaysRl => marker_block_end_x,
            WritingMode::HorizontalTb => {
                unreachable!("a horizontal block static-position marker has no vertical span")
            }
        };
        PageInlineSpan::new(left, width)
    }
}

pub(super) struct StaticHypotheticalBox {
    pub(super) style: css::ZoomedLayoutStyle,
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
pub(super) struct HypotheticalBlockMarginBox {
    area: PageTopRect,
    relative_ancestor_offset: InlineVisualOffset,
}

impl HypotheticalBlockMarginBox {
    pub(super) fn from_placeholder(
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

    pub(super) fn static_rectangle(
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

    pub(super) fn translate_inline_span(
        self,
        area: PageTopRect,
        inline_axis: PhysicalAxis,
    ) -> PageTopRect {
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
    pub(super) fn from_positioned(style: css::ZoomedLayoutStyle) -> Self {
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

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn block_static_position_placeholder_box_from_buffer(
        &mut self,
        output: &[InlineItem],
        block_style: &ComputedStyle,
        geometry: BlockStaticPositionPlaceholderGeometry,
        static_position_index: Option<usize>,
    ) -> Option<PageTopRect> {
        // Zero-sized split inline edge atoms preserve decoration boundaries,
        // but do not themselves occupy the line that selects the static
        // position. Nonzero edge atoms, such as an inline-start border before
        // a block-in-inline split, still create the hypothetical line that
        // precedes the block-level positioned box.
        let static_position_index = static_position_index
            .unwrap_or(output.len())
            .min(output.len());
        let preceding_items = &output[..static_position_index];
        let has_buffered_content = preceding_items.iter().any(|item| match item {
            InlineItem::Word(_) => !inline_item_is_collapsible_space(item),
            InlineItem::Atom(atom) => !inline_layout::inline_atom_is_phantom(atom),
            InlineItem::StaticPositionSourceMarker(_)
            | InlineItem::Float(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => false,
            // A forced line break has no inline advance, but it does create
            // a line box. Its hypothetical block position is therefore part
            // of the static-position rectangle for a following block-level
            // positioned descendant.
            // <https://www.w3.org/TR/css-position-3/#staticpos-rect>
            InlineItem::Break(_) => true,
        });
        if !has_buffered_content {
            return Some(match block_style.writing_mode {
                WritingMode::HorizontalTb => {
                    PageTopRect::new(self.content_left, self.cursor_y, 0.0, 0.0)
                }
                WritingMode::VerticalRl
                | WritingMode::VerticalLr
                | WritingMode::SidewaysRl
                | WritingMode::SidewaysLr => {
                    let axes = WritingModeAxes::new(
                        block_style.writing_mode,
                        block_style.used_direction(),
                    );
                    let block_start_x = match axes.physical_side(LogicalSide::BlockStart) {
                        PhysicalSide::Left => self.content_left,
                        PhysicalSide::Right => self.content_right,
                        PhysicalSide::Top | PhysicalSide::Bottom => {
                            unreachable!("a vertical writing mode has a horizontal block axis")
                        }
                    };
                    let span = geometry.vertical_margin_box_inline_span_at_block_start(
                        block_start_x,
                        block_style.writing_mode,
                    );
                    PageTopRect::new(span.left_x(), self.cursor_y, span.width(), 0.0)
                }
            });
        }
        let available_width = self.current_content_logical_inline_size().max(1.0);
        // CSS Positioned Layout removes the abspos from flow, but CSS 2.2
        // computes auto inset static position from its hypothetical normal-flow
        // box. For a block-level source after inline content, keep a
        // non-painting placeholder in the buffered run on the next line so
        // whitespace, wrapping, and line metrics are measured by the same inline
        // machinery as real content:
        // https://www.w3.org/TR/css-position-3/#absolute-positioning
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        let mut hypothetical_items = Vec::with_capacity(output.len() + 2);
        hypothetical_items.extend_from_slice(preceding_items);
        if !matches!(hypothetical_items.last(), Some(InlineItem::Break(_))) {
            hypothetical_items.push(InlineItem::Break(InlineBreak::default()));
        }
        // This atom chooses the preceding inline line only. It is not the
        // hypothetical block box itself: block-in-inline splitting creates
        // that box in block layout, not as an inline atomic participant.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        hypothetical_items.push(InlineItem::Atom(Box::new(
            self.block_static_position_placeholder_atom(block_style),
        )));
        hypothetical_items.extend_from_slice(&output[static_position_index..]);
        // The placeholder sequence is measurement only. In particular,
        // buffered source floats must not be registered a second time while
        // determining an absolute box's block static position.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);
        let context = sequence.context(block_style);
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let replay_snapshot = self.snapshot();
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        for record in &records {
            let contains_placeholder = record.fragment.as_ref().is_some_and(|fragment| {
                fragment.items().iter().any(|item| {
                    matches!(
                        &item.item,
                        InlineLineItem::Atom(atom)
                            if matches!(atom.content(), InlineAtomContent::StaticPositionHypothetical { .. })
                    )
                })
            });
            if contains_placeholder {
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(record, block_style.writing_mode);
                let split_boundary_cursor = self.cursor_y;
                let placeholder_box =
                    self.prepare_inline_line_record(record, context)
                        .and_then(|prepared| {
                            prepared.find_map_paint_leaf(|item| {
                                let PreparedInlinePaintItem::Atom(atom) = item else {
                                    return None;
                                };
                                matches!(
                                    atom.atom.content(),
                                    InlineAtomContent::StaticPositionHypothetical { .. }
                                )
                                .then_some(
                                    match block_style.writing_mode {
                                        WritingMode::HorizontalTb => {
                                            // A block in an inline sequence splits
                                            // the preceding inline run into an
                                            // anonymous block. Its hypothetical
                                            // block-start is the next line's
                                            // block-start, not the inline atom's
                                            // baseline-aligned content rectangle.
                                            // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
                                            // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>
                                            PageTopRect::new(
                                                self.content_left,
                                                split_boundary_cursor,
                                                0.0,
                                                0.0,
                                            )
                                        }
                                        WritingMode::VerticalRl
                                        | WritingMode::VerticalLr
                                        | WritingMode::SidewaysRl
                                        | WritingMode::SidewaysLr => {
                                            // In vertical flow the block axis is
                                            // physical horizontal. The prepared
                                            // zero-footprint marker supplies the
                                            // logical boundary between the preceding
                                            // anonymous block and the hypothetical
                                            // block. Its width is the source's
                                            // measured hypothetical margin-box
                                            // block extent, not this line's strut.
                                            let margin_box = geometry
                                            .vertical_margin_box_inline_span_from_block_end_marker(
                                                atom.border_box.x(),
                                                block_style.writing_mode,
                                            );
                                            PageTopRect::new(
                                                margin_box.left_x(),
                                                split_boundary_cursor,
                                                margin_box.width(),
                                                0.0,
                                            )
                                        }
                                    },
                                )
                            })
                        });
                self.restore(replay_snapshot);
                return placeholder_box;
            }
            stack.advance(record.block_advance());
        }
        self.restore(replay_snapshot);
        None
    }

    /// Builds a non-painting line-selection atom for a block-level static
    /// source. The real hypothetical block box is laid out separately after
    /// block-in-inline splitting; this atom must not impersonate that box.
    /// <https://www.w3.org/TR/css-position-3/#staticpos-rect>
    pub(in crate::layout) fn block_static_position_placeholder_atom(
        &mut self,
        block_style: &ComputedStyle,
    ) -> InlineAtom {
        self.block_static_position_placeholder_atom_with_inline_size(block_style, 0.0)
    }

    /// Builds a non-painting static-position atom with an explicit inline
    /// footprint. Split inline floats use this to participate in the line
    /// selection that determines their source block position.
    pub(in crate::layout) fn block_static_position_placeholder_atom_with_inline_size(
        &mut self,
        block_style: &ComputedStyle,
        inline_size: f32,
    ) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::StaticPositionHypothetical {
                source: InlineStaticPositionSourceId::Block,
                boundary: StaticPositionHypotheticalBoundary::Transparent,
            },
            block_style.clone(),
            None,
            InlineSize::new(inline_size.max(0.0), block_style.line_height),
            self.font_system
                .rendered_first_line_baseline_offset(block_style)
                .points(),
            0.0,
            None,
            None,
        )
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
        marker: InlineStaticPositionSourceId,
        allow_standalone_source: bool,
        output: &[InlineItem],
    ) -> StaticPositionCapture {
        let mut hypothetical_items = output.to_vec();
        let marker_indices = output
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                matches!(item, InlineItem::StaticPositionSourceMarker(candidate) if *candidate == marker)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        debug_assert!(
            allow_standalone_source || marker_indices.len() == 1,
            "a collected inline static-position source must have exactly one marker"
        );
        let static_position_index = if let Some(&index) = marker_indices.first() {
            let placeholder = self.inline_static_position_placeholder_atom(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                marker,
            );
            hypothetical_items[index] = InlineItem::Atom(Box::new(placeholder));
            index
        } else {
            assert!(
                allow_standalone_source,
                "deferred inline static-position source marker was lost"
            );
            let placeholder = self.inline_static_position_placeholder_atom(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                marker,
            );
            hypothetical_items.push(InlineItem::Atom(Box::new(placeholder)));
            output.len()
        };
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
        let placeholder_capture = self.inline_static_position_from_placeholder_sequence(
            element,
            marker,
            &sequence,
            block_style,
        );
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
        marker: InlineStaticPositionSourceId,
    ) -> InlineAtom {
        let available_width = (self.content_right - self.content_left).max(style.font_size);
        let mut hypothetical =
            StaticHypotheticalBox::from_positioned(self.style_with_current_viewport_lengths(style));
        let placeholder_style = &mut hypothetical.style;
        let is_non_atomic_inline_source =
            !style.abspos_static_source.is_atomic_inline() && !style.display.is_atomic_inline();
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
        if is_non_atomic_inline_source {
            // A non-atomic out-of-flow inline leaves a source boundary, not
            // an in-flow shrink-to-fit payload. Its carrier must select the
            // line at that boundary without migrating across a soft wrap.
            // Inline-axis margins belong to final positioned layout and are
            // therefore excluded here as well; retaining them in both places
            // applies the authored margin twice.
            // <https://drafts.csswg.org/css-position-3/#static-position>
            if placeholder_style.writing_mode.has_vertical_lines() {
                placeholder_style.margin.top = 0.0;
                placeholder_style.margin.bottom = 0.0;
            } else {
                placeholder_style.margin.left = 0.0;
                placeholder_style.margin.right = 0.0;
            }
        }
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
            .block_percentage_context_stack
            .current_percentage_basis();
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
        let mut atom_size = geometry.margin_box_inline_size(placeholder_style);
        if is_non_atomic_inline_source {
            if placeholder_style.writing_mode.has_vertical_lines() {
                atom_size.height = 0.0;
            } else {
                atom_size.width = 0.0;
            }
        }
        if !placeholder_style.writing_mode.has_vertical_lines() {
            // Inline-axis margins reserve advance, but block-axis margins of
            // an inline-level hypothetical box do not enlarge the line box
            // that selects its static-position rectangle. The positioned
            // box applies those margins when resolving its own block offset.
            // <https://www.w3.org/TR/CSS22/visudet.html#line-height>
            // <https://www.w3.org/TR/css-position-3/#staticpos-rect>
            atom_size.height =
                (atom_size.height - placeholder_style.margin.top - placeholder_style.margin.bottom)
                    .max(0.0);
        }
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
            InlineAtomContent::StaticPositionHypothetical {
                source: marker,
                boundary: if is_non_atomic_inline_source {
                    StaticPositionHypotheticalBoundary::Transparent
                } else {
                    StaticPositionHypotheticalBoundary::Atomic
                },
            },
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
        marker: InlineStaticPositionSourceId,
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
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        );
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        for record in &records {
            if let Some(fragment) = &record.fragment
                && fragment.items().iter().any(|item| {
                    matches!(
                        &item.item,
                            InlineLineItem::Atom(atom)
                            if static_position_hypothetical_has_source(atom.content(), marker)
                    )
                })
            {
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
                    .prepare_inline_line_record(record, context)
                    .and_then(|prepared| {
                        // The prepared line owns the canonical static
                        // baseline. Do not reconstruct it from the atom's
                        // border geometry: leading and font metrics can make
                        // that a different coordinate.
                        let baseline_y = self.cursor_y - prepared.metrics.baseline_offset;
                        prepared.find_map_paint_leaf(|item| {
                            let PreparedInlinePaintItem::Atom(atom) = item else {
                                return None;
                            };
                            let horizontal_rtl = !block_style.writing_mode.has_vertical_lines()
                                && block_style.used_direction() == Direction::Rtl;
                            let logical_inline_start_margin =
                                inline_atom_logical_inline_start_margin(&atom.atom, block_style);
                            let logical_inline_start_x = if horizontal_rtl {
                                atom.border_box.x()
                                    + atom.border_box.width()
                                    + logical_inline_start_margin
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
                                    - logical_inline_start_margin
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
                            let is_static_placeholder = static_position_hypothetical_has_source(
                                atom.atom.content(),
                                marker,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_position_marker_selects_its_source_without_source_order() {
        let first = InlineStaticPositionSourceId::Element {
            element: crate::dom::ElementId::next(),
            source: InlineStaticPositionBoxSource::Principal,
        };
        let second = InlineStaticPositionSourceId::Element {
            element: crate::dom::ElementId::next(),
            source: InlineStaticPositionBoxSource::Principal,
        };

        assert!(static_position_hypothetical_has_source(
            &InlineAtomContent::StaticPositionHypothetical {
                source: first,
                boundary: StaticPositionHypotheticalBoundary::Transparent,
            },
            first,
        ));
        assert!(!static_position_hypothetical_has_source(
            &InlineAtomContent::StaticPositionHypothetical {
                source: second,
                boundary: StaticPositionHypotheticalBoundary::Transparent,
            },
            first,
        ));
    }

    #[test]
    fn static_position_sources_distinguish_principal_and_generated_boxes() {
        let element = crate::dom::ElementId::next();
        let principal = InlineStaticPositionSourceId::Element {
            element,
            source: InlineStaticPositionBoxSource::Principal,
        };
        let before = InlineStaticPositionSourceId::Element {
            element,
            source: InlineStaticPositionBoxSource::GeneratedPseudo(
                box_tree::GeneratedPseudoKind::Before,
            ),
        };
        let after = InlineStaticPositionSourceId::Element {
            element,
            source: InlineStaticPositionBoxSource::GeneratedPseudo(
                box_tree::GeneratedPseudoKind::After,
            ),
        };

        assert_ne!(principal, before);
        assert_ne!(principal, after);
        assert_ne!(before, after);
    }

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
}
