use super::*;
use std::collections::HashSet;
use std::rc::Rc;

impl PhysicalInlineTextBounds {
    pub(in crate::layout) fn new(baseline_origin: InlinePoint, inline_size: f32) -> Self {
        Self {
            baseline_origin,
            inline_size: inline_size.max(0.0),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.baseline_origin.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.baseline_origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.inline_size
    }

    pub(in crate::layout) fn set_x(&mut self, x: f32) {
        self.baseline_origin.x = x;
    }

    pub(in crate::layout) fn set_y(&mut self, y: f32) {
        self.baseline_origin.y = y;
    }

    pub(in crate::layout) fn set_width(&mut self, width: f32) {
        self.inline_size = width.max(0.0);
    }

    pub(in crate::layout) fn text_origin(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y())
    }

    pub(in crate::layout) fn link_paint_rect(self, font_size: f32) -> PaintRect {
        paint_space_rect(self.x(), self.y() - 2.0, self.width(), font_size + 4.0)
    }
}

impl InlineLineGeometry {
    pub(in crate::layout) fn new(
        content_left: f32,
        content_right: f32,
        cursor_y: f32,
        line_block_size: f32,
        context: InlinePaintContext<'_>,
    ) -> Self {
        let style = context.block_style;
        let direction = context.direction;
        let inline_size = (context.available_width - context.line_indent).max(0.0);
        let content_inline_start = content_left + context.padding_left;
        let axes = WritingModeAxes::new(style.writing_mode, direction);
        let inline_start = match axes.physical_side(LogicalSide::InlineStart) {
            PhysicalSide::Left => content_inline_start + context.line_indent,
            PhysicalSide::Right => content_inline_start + inline_size,
            PhysicalSide::Top => cursor_y - context.line_indent,
            PhysicalSide::Bottom => cursor_y - inline_size,
        };
        let block_start = match axes.physical_side(LogicalSide::BlockStart) {
            PhysicalSide::Top | PhysicalSide::Bottom => cursor_y,
            PhysicalSide::Left => content_inline_start,
            // A vertical right-to-left line's logical block-start is the
            // physical right edge of its containing block.  Place the line's
            // own block extent immediately before that edge; subsequent lines
            // advance left through the physical line stack.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            PhysicalSide::Right => content_right - line_block_size.max(0.0),
        };
        Self {
            writing_mode: style.writing_mode,
            direction,
            inline_start_offset: context.line_indent,
            inline_start,
            inline_size,
            block_start,
        }
    }

    pub(in crate::layout) fn alignment_offset(
        self,
        content_inline_size: f32,
        align: TextAlign,
    ) -> f32 {
        let free_space = (self.inline_size - content_inline_size).max(0.0);
        match align {
            TextAlign::Left if self.line_left_is_inline_end() => free_space,
            TextAlign::Right if self.line_right_is_inline_end() => free_space,
            TextAlign::Center => free_space / 2.0,
            TextAlign::End => free_space,
            TextAlign::Left
            | TextAlign::Right
            | TextAlign::Start
            | TextAlign::Justify
            | TextAlign::JustifyAll => 0.0,
        }
    }

    pub(in crate::layout) fn hanging_punctuation_offset(
        self,
        hanging_widths: HangingPunctuationWidths,
    ) -> f32 {
        match self.direction {
            Direction::Ltr => -hanging_widths.start,
            Direction::Rtl => hanging_widths.end,
        }
    }

    pub(in crate::layout) fn visual_line_origin(
        self,
        logical_inline_start: f32,
        line_inline_size: f32,
    ) -> f32 {
        self.physical_inline_origin(logical_inline_start, line_inline_size)
    }

    pub(in crate::layout) fn visual_line_item_rect(
        self,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        inline_size: f32,
        horizontal_y: f32,
        block_size: f32,
    ) -> PhysicalInlineRect {
        let axes = WritingModeAxes::new(self.writing_mode, self.direction);
        if !axes.swaps_physical_axes() {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(line_physical_origin + visual_inline_start, horizontal_y),
                InlineSize::new(inline_size, block_size),
            ))
        } else {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(
                    self.block_start,
                    self.physical_inline_origin(
                        line_logical_inline_start + visual_inline_start,
                        inline_size,
                    ),
                ),
                InlineSize::new(block_size, inline_size),
            ))
        }
    }

    pub(in crate::layout) fn position_visual_text_group(
        self,
        group: &mut PreparedInlineTextGroup,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
    ) {
        if !WritingModeAxes::new(self.writing_mode, self.direction).swaps_physical_axes() {
            group.set_x(line_physical_origin + visual_inline_start);
        } else {
            group.set_x(self.block_start);
            group.set_y(self.vertical_text_inline_origin(
                line_logical_inline_start + visual_inline_start,
                group.width(),
            ));
        }
    }

    pub(in crate::layout) fn physical_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        match WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineStart)
        {
            // Page-top inline coordinates increase upward, unlike CSS physical
            // Y coordinates. Preserve that local coordinate convention at the
            // paint boundary while obtaining the side from the shared map.
            PhysicalSide::Right | PhysicalSide::Top => {
                self.inline_start - logical_inline_start - inline_size
            }
            PhysicalSide::Left | PhysicalSide::Bottom => self.inline_start + logical_inline_start,
        }
    }

    pub(in crate::layout) fn vertical_text_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        // `sideways-lr` fixes the line's physical orientation bottom-to-top.
        // `direction` still selects bidi ordering and logical alignment, but
        // must not reverse the PDF run origin a second time after the
        // sideways-left text matrix has established that physical line.
        // <https://drafts.csswg.org/css-writing-modes-4/#valdef-writing-mode-sideways-lr>
        let (placement, placement_logical_inline_start) =
            if self.writing_mode == WritingMode::SidewaysLr && self.direction == Direction::Rtl {
                // The line geometry was collected with RTL's physical top edge
                // as its inline start. `sideways-lr` instead has a fixed
                // bottom-to-top physical line, so changing only `direction`
                // would reinterpret that top coordinate as a bottom coordinate
                // and replay the atom outside its cell. Reproject the retained
                // logical line span to the writing-mode-defined bottom edge at
                // the same boundary.
                // <https://www.w3.org/TR/css-writing-modes-4/#valdef-writing-mode-sideways-lr>
                (
                    Self {
                        direction: Direction::Ltr,
                        inline_start: self.inline_start - self.inline_size,
                        ..self
                    },
                    self.inline_size - logical_inline_start - inline_size,
                )
            } else {
                (self, logical_inline_start)
            };
        let origin = placement.physical_inline_origin(placement_logical_inline_start, inline_size);
        if WritingModeAxes::new(self.writing_mode, placement.direction)
            .physical_side(LogicalSide::InlineStart)
            == PhysicalSide::Top
        {
            origin + inline_size
        } else {
            origin
        }
    }

    /// CSS Writing Modes maps `text-align: left` and `right` through the
    /// line-left and line-right sides, which are not always physical left and
    /// right.  In vertical writing, the two sides are physical top/bottom;
    /// `sideways-lr` reverses their order.
    /// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>
    fn line_left_side(self) -> PhysicalSide {
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalSide::Left,
            WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => {
                PhysicalSide::Top
            }
            WritingMode::SidewaysLr => PhysicalSide::Bottom,
        }
    }

    fn line_right_side(self) -> PhysicalSide {
        match self.line_left_side() {
            PhysicalSide::Left => PhysicalSide::Right,
            PhysicalSide::Right => PhysicalSide::Left,
            PhysicalSide::Top => PhysicalSide::Bottom,
            PhysicalSide::Bottom => PhysicalSide::Top,
        }
    }

    fn line_left_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == self.line_left_side()
    }

    fn line_right_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == self.line_right_side()
    }
}

#[derive(Debug, Clone, Copy)]
/// Shared inputs for laying out one inline paragraph run.
///
/// CSS Inline Layout forms line boxes from consecutive inline-level content
/// inside a block container; these values are the containing line box measure
/// and block style used by the word and mixed inline layout paths:
/// <https://www.w3.org/TR/css-inline-3/#line-layout>.
pub(in crate::layout) struct InlineParagraphContext<'a> {
    pub(in crate::layout) block_style: &'a ComputedStyle,
    /// The resolved layout clamp for this block formatting context. This is
    /// distinct from the immutable computed `line-clamp` declaration.
    pub(in crate::layout) line_clamp: Option<css::InlineLineClamp<'a>>,
    /// Whether an enclosing block-flow traversal has established that source
    /// continues after this inline graph. Preserved breaks and block children
    /// split one clamped stream into several independent inline graphs.
    /// <https://drafts.csswg.org/css-overflow-4/#line-clamp>
    pub(in crate::layout) clamp_continuation: css::ClampContinuation,
    pub(in crate::layout) stylesheets: &'a Stylesheets<'a>,
    /// Whether the first formatted line of the originating block container
    /// is still available to this anonymous inline run.
    ///
    /// Anonymous blocks produced by mixed inline/block content do not restart
    /// their parent's first formatted line for `text-indent`, even though a
    /// real descendant block establishes its own formatting context.
    pub(in crate::layout) initial_first_formatted_line: bool,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) padding_left: f32,
    pub(in crate::layout) hanging_indent: f32,
    pub(in crate::layout) hanging_punctuation_reserve: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlinePaintContext<'a> {
    pub(in crate::layout) block_style: &'a ComputedStyle,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) padding_left: f32,
    pub(in crate::layout) line_indent: f32,
    pub(in crate::layout) text_align: TextAlign,
    pub(in crate::layout) is_first_line: bool,
    /// Used logical block advance selected for this line record. This can
    /// differ from the glyph/baseline metrics used for vertical alignment.
    pub(in crate::layout) line_block_size: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtomData {
    pub(in crate::layout) content: InlineAtomContent,
    pub(in crate::layout) style: Rc<ComputedStyle>,
    pub(in crate::layout) outside_marker: Option<ListMarker>,
    pub(in crate::layout) content_inline_offset: f32,
    pub(in crate::layout) content_inline_paint_width: Option<f32>,
    pub(in crate::layout) escaped_positioned_layers: Option<Rc<[PositionedPaintLayer]>>,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) alt_text: Option<Rc<str>>,
    /// The containing inline scope. Atomic boxes use their parent scope so an
    /// atom's own style cannot own tracking on its outside boundaries.
    pub(in crate::layout) tracking_scope: Option<Rc<InlineTrackingScope>>,
    /// Paintless advance inserted before this item after visual ordering.
    pub(in crate::layout) leading_tracking: LayoutLength,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtom {
    pub(in crate::layout) data: Rc<InlineAtomData>,
    pub(in crate::layout) size: InlineSize,
    pub(in crate::layout) baseline: InlineAtomBaseline,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

/// The baseline an atomic inline contributes to its containing line.
///
/// Atomic inline boxes carry physical margin-box dimensions for paint. Most
/// exported baselines are measured from the principal border box's logical
/// block-start edge, but CSS 2.2 makes an `inline-table` align using its table
/// box rather than its wrapper. Keeping that reference distinct prevents the
/// enclosing line from applying the wrapper's block-start margin a second
/// time. Keeping the fallback distinct prevents callers from treating a
/// synthesized block-end baseline as a descendant text baseline:
/// <https://www.w3.org/TR/CSS22/tables.html#table-display>
/// <https://drafts.csswg.org/css-inline-3/#inline-block-baseline>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum InlineAtomBaseline {
    Exported {
        offset_from_border_box_block_start: f32,
    },
    ExportedTableBox {
        offset_from_table_box_block_start: f32,
    },
    SynthesizedBorderBoxBlockEnd,
}

impl InlineAtom {
    #[allow(clippy::arc_with_non_send_sync, clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        content: InlineAtomContent,
        style: ComputedStyle,
        escaped_positioned_layers: Option<Box<[PositionedPaintLayer]>>,
        size: InlineSize,
        baseline_offset: f32,
        baseline_shift: f32,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            data: Rc::new(InlineAtomData {
                content,
                style: Rc::new(style),
                outside_marker: None,
                content_inline_offset: 0.0,
                content_inline_paint_width: None,
                escaped_positioned_layers: escaped_positioned_layers.map(Rc::from),
                link_target: link_target.map(Rc::from),
                alt_text: alt_text.map(Rc::from),
                tracking_scope: None,
                leading_tracking: layout_pt(0.0),
            }),
            size,
            baseline: InlineAtomBaseline::Exported {
                offset_from_border_box_block_start: baseline_offset,
            },
            baseline_shift,
            visual_offset: InlineVisualOffset::zero(),
        }
    }

    pub(in crate::layout) fn with_visual_offset(
        mut self,
        visual_offset: InlineVisualOffset,
    ) -> Self {
        self.visual_offset = visual_offset;
        self
    }

    pub(in crate::layout) fn with_tracking_scope(mut self, scope: Rc<InlineTrackingScope>) -> Self {
        Rc::make_mut(&mut self.data).tracking_scope = Some(scope);
        self
    }

    pub(in crate::layout) fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>> {
        self.data.tracking_scope.as_ref()
    }

    pub(in crate::layout) fn leading_tracking(&self) -> LayoutLength {
        self.data.leading_tracking
    }

    pub(in crate::layout) fn set_leading_tracking(&mut self, advance: LayoutLength) {
        Rc::make_mut(&mut self.data).leading_tracking = advance;
    }

    pub(in crate::layout) fn with_outside_marker(mut self, marker: Option<ListMarker>) -> Self {
        Rc::make_mut(&mut self.data).outside_marker = marker;
        self
    }

    pub(in crate::layout) fn with_content_inline_offset(mut self, offset: f32) -> Self {
        Rc::make_mut(&mut self.data).content_inline_offset = offset;
        self
    }

    pub(in crate::layout) fn with_content_inline_paint_width(mut self, width: f32) -> Self {
        Rc::make_mut(&mut self.data).content_inline_paint_width = Some(width);
        self
    }

    pub(in crate::layout) fn content(&self) -> &InlineAtomContent {
        &self.data.content
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
    }

    /// Resolve this atom's baseline from its border-box logical block start.
    ///
    /// The caller supplies the border-box block size because atoms retain
    /// physical dimensions and their containing inline formatting context
    /// owns the logical-axis projection.
    pub(in crate::layout) fn baseline_offset_from_border_box_block_start(
        &self,
        border_box_block_size: f32,
    ) -> f32 {
        match self.baseline {
            InlineAtomBaseline::Exported {
                offset_from_border_box_block_start,
            } => offset_from_border_box_block_start,
            InlineAtomBaseline::ExportedTableBox {
                offset_from_table_box_block_start,
            } => offset_from_table_box_block_start,
            InlineAtomBaseline::SynthesizedBorderBoxBlockEnd => border_box_block_size,
        }
    }

    /// Resolve this atom's baseline from the containing line's logical
    /// margin-box block-start edge.
    ///
    /// For ordinary atomic inlines, the principal border box begins after its
    /// block-start margin. `inline-table` is the CSS 2.2 exception: its table
    /// box, rather than the wrapper carrying its margins, supplies the
    /// baseline used for inline vertical alignment.
    /// <https://www.w3.org/TR/CSS22/tables.html#table-display>
    pub(in crate::layout) fn baseline_offset_from_margin_box_block_start(
        &self,
        border_box_block_size: f32,
        block_start_margin: f32,
    ) -> f32 {
        match self.baseline {
            InlineAtomBaseline::Exported {
                offset_from_border_box_block_start,
            } => block_start_margin + offset_from_border_box_block_start,
            InlineAtomBaseline::ExportedTableBox {
                offset_from_table_box_block_start,
            } => offset_from_table_box_block_start,
            InlineAtomBaseline::SynthesizedBorderBoxBlockEnd => {
                block_start_margin + border_box_block_size
            }
        }
    }

    /// Mark an exported baseline as originating from an inline-table's table
    /// box rather than its wrapper box.
    ///
    /// CSS 2.2 requires `vertical-align` on `inline-table` to use the table
    /// box. Its wrapper margins remain part of the atomic inline's outer
    /// geometry, but do not move the exported first-row baseline.
    /// <https://www.w3.org/TR/CSS22/tables.html#table-display>
    pub(in crate::layout) fn with_exported_table_box_baseline(mut self) -> Self {
        let InlineAtomBaseline::Exported {
            offset_from_border_box_block_start,
        } = self.baseline
        else {
            unreachable!("only an exported baseline can originate from a table box");
        };
        self.baseline = InlineAtomBaseline::ExportedTableBox {
            offset_from_table_box_block_start: offset_from_border_box_block_start,
        };
        self
    }

    pub(in crate::layout) fn with_synthesized_border_box_block_end_baseline(mut self) -> Self {
        self.baseline = InlineAtomBaseline::SynthesizedBorderBoxBlockEnd;
        self
    }

    pub(in crate::layout) fn outside_marker(&self) -> Option<&ListMarker> {
        self.data.outside_marker.as_ref()
    }

    pub(in crate::layout) fn content_inline_offset(&self) -> f32 {
        self.data.content_inline_offset
    }

    pub(in crate::layout) fn content_inline_paint_width(&self) -> Option<f32> {
        self.data.content_inline_paint_width
    }

    pub(in crate::layout) fn escaped_positioned_layers(&self) -> Option<&[PositionedPaintLayer]> {
        self.data.escaped_positioned_layers.as_deref()
    }

    /// Keep a positioned descendant with the inline source atom that owns
    /// its containing block.  The atom replays these layers only if its
    /// source range survives line selection, which makes a discarded clamp
    /// continuation discard the descendant as well.
    pub(in crate::layout) fn append_escaped_positioned_layers(
        &mut self,
        layers: Vec<PositionedPaintLayer>,
    ) {
        if layers.is_empty() {
            return;
        }
        let data = Rc::make_mut(&mut self.data);
        let mut retained = data
            .escaped_positioned_layers
            .as_deref()
            .unwrap_or_default()
            .to_vec();
        retained.extend(layers);
        data.escaped_positioned_layers = Some(Rc::from(retained));
    }

    pub(in crate::layout) fn link_target(&self) -> Option<&str> {
        self.data.link_target.as_deref()
    }

    pub(in crate::layout) fn alt_text(&self) -> Option<&str> {
        self.data.alt_text.as_deref()
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFloatData {
    pub(in crate::layout) element: Element,
    pub(in crate::layout) signature: ElementSignature,
    pub(in crate::layout) style: ComputedStyle,
    /// Generated pseudo-elements are tree-abiding boxes with their own
    /// generated-content children.  They must not be rehydrated from the
    /// originating element's DOM descendants when the inline float is laid
    /// out later.
    /// <https://www.w3.org/TR/css-pseudo-4/#generated-content>
    pub(in crate::layout) generated_content: bool,
    pub(in crate::layout) positioning_containing_block:
        Option<InlinePositioningContainingBlockSource>,
}

/// Source inline box that establishes a containing block for abspos descendants.
///
/// CSS 2.2 defines an absolutely positioned descendant's containing block from
/// the padding boxes of its nearest positioned inline ancestor:
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct InlinePositioningContainingBlockSource {
    pub(in crate::layout) id: InlinePositioningContainingBlockId,
    pub(in crate::layout) style: ComputedStyle,
}

/// Stable identity for one collected inline containing-block source.
///
/// This distinguishes nested positioned inline ancestors that share identical
/// computed styles, matching the nearest-ancestor rule from CSS 2.2:
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct InlinePositioningContainingBlockId(pub(in crate::layout) usize);

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFloat {
    pub(in crate::layout) data: Rc<InlineFloatData>,
}

impl InlineFloat {
    pub(in crate::layout) fn new(
        element: Element,
        signature: ElementSignature,
        style: ComputedStyle,
        generated_content: bool,
        positioning_containing_block: Option<InlinePositioningContainingBlockSource>,
    ) -> Self {
        Self {
            data: Rc::new(InlineFloatData {
                element,
                signature,
                style,
                generated_content,
                positioning_containing_block,
            }),
        }
    }

    pub(in crate::layout) fn element(&self) -> &Element {
        &self.data.element
    }

    pub(in crate::layout) fn signature(&self) -> &ElementSignature {
        &self.data.signature
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
    }

    pub(in crate::layout) fn is_generated_content(&self) -> bool {
        self.data.generated_content
    }

    pub(in crate::layout) fn positioning_containing_block(
        &self,
    ) -> Option<&InlinePositioningContainingBlockSource> {
        self.data.positioning_containing_block.as_ref()
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum InlineAtomContent {
    Canvas,
    /// An inline iframe keeps its embedding element identity so the final
    /// paint pass can composite the matching isolated browsing context.
    Iframe(crate::dom::ElementId),
    Image(DecodedPngImage),
    /// A generated `content` gradient preserves its CSS image identity until
    /// paint.  The raster is used only when native PDF shading cannot express
    /// the gradient exactly.
    Gradient {
        image: BackgroundImage,
        fallback: DecodedPngImage,
    },
    Svg {
        asset: Option<SharedSvgAsset>,
    },
    /// Non-painting inline-level placeholder for an out-of-flow positioned box.
    ///
    /// CSS Positioned Layout resolves auto insets from the static-position
    /// rectangle, the hypothetical normal-flow position the box would have
    /// occupied before being taken out of flow. Inline layout carries this atom
    /// only through temporary line selection and preparation so forced breaks,
    /// wrapping, line metrics, and inline alignment choose the correct
    /// static-position rectangle; it must never paint:
    /// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height> and
    /// <https://www.w3.org/TR/css-position-3/#staticpos-rect>.
    StaticPositionPlaceholder,
    InlineBox {
        sequence: inline_layout::InlineLineSequence,
    },
    /// A coupled ruby base level and its first-level interlinear annotation.
    ///
    /// The parent inline graph receives one column-group advance, while paint
    /// replays the base and annotation sequences at their separate logical
    /// block positions. This is deliberately distinct from `InlineBox`: ruby
    /// annotations must not become parent-line text or justification
    /// opportunities.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    Ruby {
        /// Source text on the base side of this paired ruby column. The
        /// inline opportunity graph uses it only to apply UAX #14 at the
        /// boundary between adjacent ruby bases; annotation sidecars remain
        /// excluded from the parent text stream.
        base_text: String,
        base: RubyInlineLevel,
        annotations: Vec<RubyInlineLevel>,
        /// Used placement of each annotation level. This remains parallel to
        /// `annotations`, making a missing side a type-visible construction
        /// error rather than an implicit default at paint time.
        annotation_sides: Vec<css::RubyAnnotationSide>,
        base_block_size: f32,
        /// Shared logical block extent of each annotation level in the
        /// normalized ruby column group. Every column uses the same level
        /// stack so empty anonymous counterparts cannot move a sibling
        /// annotation independently.
        annotation_block_sizes: Vec<f32>,
    },
    /// A tate-chu-yoko run shaped in horizontal typographic mode and carried
    /// as one vertical-em atomic inline.  CSS Writing Modes performs this
    /// composition before line breaking, then compresses the horizontal run
    /// to the used em square:
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>.
    TextCombineUpright {
        sequence: inline_layout::InlineLineSequence,
        horizontal_style: Box<ComputedStyle>,
        inline_scale: f32,
    },
    /// Paint captured from an independent formatting context. Table-cell
    /// fragments retain their final content-coordinate context so replay does
    /// not infer writing mode from the enclosing inline line.
    InlineFragment {
        fragment: Box<PaintFragment>,
        table_cell_context: Option<table::TableCellContentCoordinateContext>,
    },
    InlineEdge(InlineEdgeRole),
    Leader(String),
}

/// A measured ruby base or annotation level together with the role that
/// establishes its line-box metrics.
///
/// Ruby annotations are separate inline formatting contexts: their `rt`/
/// `rtc` line-height must not be replaced with the containing ruby box's
/// line-height when they are replayed beside the base level.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
#[derive(Debug, Clone)]
pub(in crate::layout) struct RubyInlineLevel {
    pub(in crate::layout) sequence: inline_layout::InlineLineSequence,
    pub(in crate::layout) style: Box<ComputedStyle>,
    /// Logical inline extent occupied after `ruby-align: space-around`
    /// distribution. This can be narrower than its column: the remaining
    /// space is the equal half-opportunity at each content edge.
    pub(in crate::layout) paint_inline_size: f32,
    /// Logical inline extent available for this level's paint replay. A
    /// spanning annotation owns the combined width of its paired base
    /// columns while the parent line still advances column by column.
    pub(in crate::layout) containing_inline_size: f32,
    /// Whether this level starts its paired base span. Continuation columns
    /// retain an empty sidecar only to preserve annotation-level indexing.
    pub(in crate::layout) starts_span: bool,
    /// Number of adjacent base columns this level covers when it starts a
    /// span. Non-starting continuation entries use zero-width paint.
    pub(in crate::layout) column_span: usize,
}

/// One owned inline box edge fragment carried through inline layout.
///
/// CSS 2.2 includes inline-axis margin, border, and padding at the start and
/// end of inline boxes, and negative margins reduce the advance without
/// removing border or padding paint. Split inline boxes also own only the
/// relevant start/end decoration under `box-decoration-break: slice`:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>,
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>, and
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineBoxEdgeFragment {
    pub(in crate::layout) logical_edge: InlineLogicalEdge,
    pub(in crate::layout) physical_side: PhysicalSide,
    pub(in crate::layout) positioning_containing_block_id:
        Option<InlinePositioningContainingBlockId>,
    pub(in crate::layout) advance: f32,
    pub(in crate::layout) paint_extent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum InlineLogicalEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum InlineEdgeRole {
    BoxEdge(InlineBoxEdgeFragment),
    TextAutospace,
}

impl InlineAtomContent {
    pub(in crate::layout) fn is_inline_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(_))
    }

    pub(in crate::layout) fn is_box_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(InlineEdgeRole::BoxEdge(_)))
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum InlineItem {
    Word(Box<InlineWord>),
    Atom(Box<InlineAtom>),
    Float(Box<InlineFloat>),
    Break(InlineBreak),
    PageScopeStart(Option<String>),
    PageScopeEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineBreak {
    pub(in crate::layout) clear: Clear,
    /// Whether this boundary originated in an explicit inline break element or
    /// in a preserved CSS Text segment break.  Phase II line-edge treatment
    /// must not infer this from visual position after the stream is split.
    /// <https://drafts.csswg.org/css-text-3/#white-space-phase-1>
    pub(in crate::layout) origin: InlineBreakOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::layout) enum InlineBreakOrigin {
    #[default]
    Explicit,
    PreservedSegment,
}

impl Default for InlineBreak {
    fn default() -> Self {
        Self {
            clear: Clear::None,
            origin: InlineBreakOrigin::Explicit,
        }
    }
}

impl AsRef<InlineItem> for InlineItem {
    fn as_ref(&self) -> &InlineItem {
        self
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(in crate::layout) enum InlineLineItem {
    Fragment(InlineFragment),
    Atom(InlineAtom),
    Float(InlineFloat),
}

impl AsRef<InlineLineItem> for InlineLineItem {
    fn as_ref(&self) -> &InlineLineItem {
        self
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct LayoutSnapshot {
    pub(in crate::layout) pages: Vec<Page>,
    pub(in crate::layout) page_names: Vec<Option<String>>,
    pub(in crate::layout) page_blanks: Vec<bool>,
    pub(in crate::layout) page_name_scope_suppression: usize,
    pub(in crate::layout) page_name_element_scope_suppression: usize,
    pub(in crate::layout) page_value_scope_stack: Vec<Option<String>>,
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) suppressed_named_strings_before:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) suppressed_named_strings_after:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) page_anchor_counters: HashMap<String, HashMap<String, Vec<i32>>>,
    pub(in crate::layout) has_normal_flow_target_references: bool,
    pub(in crate::layout) document_canvas_background: Option<DocumentCanvasBackground>,
    pub(in crate::layout) document_canvas_overflow: DocumentCanvasResolution,
    pub(in crate::layout) document_canvas_fragment_insets: Vec<FragmentOffsets>,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    pub(in crate::layout) current_page_has_named_page_flow_content: bool,
    pub(in crate::layout) current_page_selected_name: Option<String>,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    pub(in crate::layout) initial_viewport_context: PageContext,
    pub(in crate::layout) fragmentainer_override: Option<FragmentainerOverride>,
    pub(in crate::layout) footnote_measurements: Vec<FootnoteMeasurement>,
    pub(in crate::layout) rendered_footnote_measurements: Vec<FootnoteMeasurement>,
    pub(in crate::layout) measured_footnotes: HashSet<ElementId>,
    pub(in crate::layout) committed_inline_floats: HashMap<ElementId, CommittedInlineFloat>,
    pub(in crate::layout) rendered_footnotes: HashSet<ElementId>,
    pub(in crate::layout) footnote_measurement_depth: usize,
    pub(in crate::layout) fragmentation_suppression_depth: usize,
    pub(in crate::layout) multicol_spanner_fragmentation_depth: usize,
    pub(in crate::layout) multicol_spanner_speculation_depth: usize,
    pub(in crate::layout) multicol_balance_probe_depth: usize,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    pub(in crate::layout) table_cell_content_coordinate_contexts:
        Vec<table::TableCellContentCoordinateContext>,
    pub(in crate::layout) principal_body_block_end_inset: LayoutLength,
    pub(in crate::layout) root_principal_flow_context: RootPrincipalFlowContext,
    pub(in crate::layout) root_pseudo_block_projection: Option<RootPseudoBlockProjection>,
    pub(in crate::layout) inline_split_float_exclusion_query_offset: RelativeOffset,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout) multicol_column_containing_blocks: Vec<MulticolColumnContainingBlock>,
    pub(in crate::layout) intrinsic_inline_percentage_basis_stack:
        Vec<IntrinsicInlinePercentageBasis>,
    pub(in crate::layout) inline_static_position: Option<InlineStaticPosition>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    pub(in crate::layout) clamp_line_slot_captures: Vec<usize>,
    pub(in crate::layout) positioned_inline_layout_suppression_depth: usize,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    pub(in crate::layout) pending_outside_marker_anchors: Vec<PendingOutsideMarkerAnchor>,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    pub(in crate::layout) grid_positioning_scopes: Vec<grid::GridPositioningScope>,
    pub(in crate::layout) pending_subgrid_contexts: Vec<Option<grid::ResolvedSubgridContext>>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) escaped_atom_containing_block: Option<ContainingBlock>,
    pub(in crate::layout) escaped_atom_positioning_context: Option<EscapedAtomPositioningContext>,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    pub(in crate::layout) fragment_top_offsets: Vec<f32>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout) normal_flow_relative_containing_blocks:
        Vec<NormalFlowRelativeContainingBlock>,
    pub(in crate::layout) block_static_position_contexts: Vec<BlockStaticPositionContext>,
    pub(in crate::layout) definite_block_size_stack: Vec<BlockSizePercentageBasis>,
    pub(in crate::layout) replayed_flex_item_percentage_height_bases:
        Vec<Option<BlockSizePercentageBasis>>,
    pub(in crate::layout) table_wrapper_block_size_overrides: Vec<Option<BorderBoxLength>>,
    pub(in crate::layout) multicol_text_box_trim_end_child_indices: Option<Vec<usize>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    pub(in crate::layout) containing_blocks: Vec<ContainingBlock>,
    pub(in crate::layout) fixed_containing_blocks: Vec<ContainingBlock>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) counter_plan: CounterPlan,
    pub(in crate::layout) current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) current_page_running_elements:
        HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) next_assignment_id: usize,
    pub(in crate::layout) assignment_capture_stack: Vec<Vec<AssignmentId>>,
    pub(in crate::layout) quote_depth: usize,
    pub(in crate::layout) ancestors: Vec<ElementSignature>,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    pub(in crate::layout) absolute_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) pending_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) active_scroll_snap_scopes: Vec<scroll_snap::ActiveScrollSnapScope>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    pub(in crate::layout) float_fragment_parent_inline_spans: Vec<PageInlineSpan>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_paint_fragments: Vec<PendingPaintFragment>,
    pub(in crate::layout) pending_page_side_effects: Vec<PendingPageSideEffects>,
    pub(in crate::layout) applied_clearance_count: usize,
    pub(in crate::layout) float_paint_capture_depth: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
    pub(in crate::layout) suppress_next_principal_box_decoration: bool,
    pub(in crate::layout) pending_page_footnotes: Vec<ElementId>,
}

/// One successful source-order inline-float placement.
///
/// `exclusion` is the physical float shape registered for later line-band
/// queries.  Its `id` and `source_order` identify the already-captured float
/// paint subtree, so replay can consume this placement without re-laying out
/// the floated DOM element.
#[derive(Debug, Clone)]
pub(in crate::layout) struct CommittedInlineFloat {
    /// `(run_index, byte_offset)` in the inline opportunity graph.  Keeping
    /// the source identity as scalars avoids leaking the graph's private
    /// representation into the layout-wide transaction state.
    pub(in crate::layout) marker: (usize, usize),
    pub(in crate::layout) selected_row: usize,
    pub(in crate::layout) exclusion: FloatShape,
}

impl CommittedInlineFloat {
    /// Validate the durable transaction at the point ordinary child traversal
    /// consumes it instead of replaying the floated subtree.
    pub(in crate::layout) fn is_valid(&self) -> bool {
        self.marker.0 != usize::MAX
            && self.marker.1 != usize::MAX
            && self.selected_row != usize::MAX
            && self.exclusion.is_css_float()
    }
}

/// A temporary fragmentainer materialized through Quire's page cursor.
///
/// Multi-column layout uses isolated page-shaped canvases as anonymous column
/// boxes. This keeps all existing nested fragmentation paths on one cursor
/// model while resolving target-specific breaks as column breaks.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerOverride {
    pub(in crate::layout) kind: FragmentainerKind,
    /// Context used by the initial anonymous fragmentainers in a partial row.
    pub(in crate::layout) initial_context: PageContext,
    /// Number of fragmentainers that share `initial_context` before later
    /// rows use `context`.
    pub(in crate::layout) initial_fragmentainer_count: usize,
    pub(in crate::layout) context: PageContext,
    pub(in crate::layout) relax_widows_orphans: bool,
}

impl FragmentainerOverride {
    pub(in crate::layout) fn context_for_fragmentainer(self, index: usize) -> PageContext {
        if index < self.initial_fragmentainer_count {
            self.initial_context
        } else {
            self.context
        }
    }
}

/// The typed content-box containing block of one anonymous multicol column.
///
/// A nested multicol formatting context can be measured in a temporary page
/// whose physical coordinates do not describe its parent column. Retaining
/// both the semantic inline size and source paint origin prevents its
/// percentages and committed paint geometry from diverging:
/// <https://www.w3.org/TR/css-multicol-1/#column-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MulticolColumnContainingBlock {
    pub(in crate::layout) inline_size: LogicalInlineContentSize,
    pub(in crate::layout) content_left: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct AnchorText {
    pub(in crate::layout) content: String,
    pub(in crate::layout) before: String,
    pub(in crate::layout) after: String,
}

/// Target data captured at the target element's first fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct TargetAnchor {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) text: AnchorText,
    pub(in crate::layout) counters: HashMap<String, Vec<i32>>,
}

/// Immutable target state exchanged between complete fresh layout passes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::layout) struct TargetReferenceSnapshot {
    pub(in crate::layout) anchors: HashMap<String, TargetAnchor>,
    pub(in crate::layout) total_pages: usize,
}
