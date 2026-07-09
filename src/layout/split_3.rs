use super::*;
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
        cursor_y: f32,
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
            PhysicalSide::Left | PhysicalSide::Right => content_inline_start,
        };
        Self {
            writing_mode: style.writing_mode,
            direction,
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
            TextAlign::Left if self.physical_left_is_inline_end() => free_space,
            TextAlign::Right if self.physical_right_is_inline_end() => free_space,
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
        let origin = self.physical_inline_origin(logical_inline_start, inline_size);
        if WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineStart)
            == PhysicalSide::Top
        {
            origin + inline_size
        } else {
            origin
        }
    }

    pub(in crate::layout) fn physical_left_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == PhysicalSide::Left
    }

    pub(in crate::layout) fn physical_right_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == PhysicalSide::Right
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
    pub(in crate::layout) stylesheets: &'a [Stylesheet],
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
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtom {
    pub(in crate::layout) data: Rc<InlineAtomData>,
    pub(in crate::layout) size: InlineSize,
    pub(in crate::layout) baseline_offset: f32,
    /// Whether this atomic inline exports a baseline produced by its own
    /// formatting context. Layout containment suppresses that export; its
    /// inline parent then aligns the principal box at its block-end edge.
    /// <https://www.w3.org/TR/css-contain-1/#containment-layout>
    pub(in crate::layout) exports_internal_baseline: bool,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
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
        // An empty atomic inline already uses the CSS inline-block fallback
        // baseline at its block-end edge. Layout containment suppresses only
        // a descendant-provided baseline, so preserve that identical empty
        // fallback instead of promoting the atom to a separate line-relative
        // participant.
        // <https://www.w3.org/TR/css-inline-3/#inline-block-baseline>
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        let exports_internal_baseline = !style.contain.layout
            || (matches!(&content, InlineAtomContent::InlineBox { sequence } if sequence.records.is_empty())
                && !style.box_values.width.is_auto()
                && !style.box_values.height.is_auto());
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
            }),
            size,
            baseline_offset,
            exports_internal_baseline,
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

    pub(in crate::layout) fn exports_internal_baseline(&self) -> bool {
        self.exports_internal_baseline
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
    InlineFragment(Box<PaintFragment>),
    InlineEdge(InlineEdgeRole),
    Leader(String),
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
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) document_canvas_background: Option<DocumentCanvasBackground>,
    pub(in crate::layout) document_canvas_overflow: DocumentCanvasOverflowContext,
    pub(in crate::layout) document_canvas_fragment_insets: Vec<FragmentOffsets>,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    pub(in crate::layout) current_page_has_named_page_flow_content: bool,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    pub(in crate::layout) initial_viewport_context: PageContext,
    pub(in crate::layout) fragmentainer_override: Option<FragmentainerOverride>,
    pub(in crate::layout) fragmentation_suppression_depth: usize,
    pub(in crate::layout) multicol_spanner_fragmentation_depth: usize,
    pub(in crate::layout) multicol_spanner_speculation_depth: usize,
    pub(in crate::layout) multicol_balance_probe_depth: usize,
    pub(in crate::layout) forced_break_containment_scopes: Vec<Option<FragmentainerOverride>>,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
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
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    pub(in crate::layout) grid_positioning_scopes: Vec<grid::GridPositioningScope>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) escaped_atom_containing_block: Option<ContainingBlock>,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    pub(in crate::layout) fragment_top_offsets: Vec<f32>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout) normal_flow_relative_containing_blocks:
        Vec<NormalFlowRelativeContainingBlock>,
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
    pub(in crate::layout) pending_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) active_scroll_snap_scopes: Vec<scroll_snap::ActiveScrollSnapScope>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_paint_fragments: Vec<PendingPaintFragment>,
    pub(in crate::layout) pending_page_side_effects: Vec<PendingPageSideEffects>,
    pub(in crate::layout) applied_clearance_count: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
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
