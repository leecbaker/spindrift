use super::*;
use std::rc::Rc;

impl PhysicalInlineTextBounds {
    pub(in crate::layout) fn new(x: f32, y: f32, inline_size: f32) -> Self {
        Self {
            baseline_origin: InlinePoint::new(x, y),
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
        let inline_size = (context.available_width - context.line_indent).max(1.0);
        let content_inline_start = content_left + context.padding_left;
        let inline_start = match (style.writing_mode, direction) {
            (WritingMode::HorizontalTb, Direction::Ltr) => {
                content_inline_start + context.line_indent
            }
            (WritingMode::HorizontalTb, Direction::Rtl) => content_inline_start + inline_size,
            (_, Direction::Ltr) => cursor_y - context.line_indent,
            (_, Direction::Rtl) => cursor_y - inline_size,
        };
        let block_start = match style.writing_mode {
            WritingMode::HorizontalTb => cursor_y,
            WritingMode::VerticalRl | WritingMode::VerticalLr => content_inline_start,
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
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalInlineRect::new(
                line_physical_origin + visual_inline_start,
                horizontal_y,
                inline_size,
                block_size,
            ),
            WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalInlineRect::new(
                self.block_start,
                self.physical_inline_origin(
                    line_logical_inline_start + visual_inline_start,
                    inline_size,
                ),
                block_size,
                inline_size,
            ),
        }
    }

    pub(in crate::layout) fn position_visual_text_group(
        self,
        group: &mut PreparedInlineTextGroup,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
    ) {
        match self.writing_mode {
            WritingMode::HorizontalTb => {
                group.set_x(line_physical_origin + visual_inline_start);
            }
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                group.set_x(self.block_start);
                group.set_y(self.vertical_text_inline_origin(
                    line_logical_inline_start + visual_inline_start,
                    group.width(),
                ));
            }
        }
    }

    pub(in crate::layout) fn physical_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        match inline_start_side(self.writing_mode, self.direction) {
            PhysicalSide::Left | PhysicalSide::Bottom => self.inline_start + logical_inline_start,
            PhysicalSide::Right | PhysicalSide::Top => {
                self.inline_start - logical_inline_start - inline_size
            }
        }
    }

    pub(in crate::layout) fn vertical_text_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        let origin = self.physical_inline_origin(logical_inline_start, inline_size);
        if inline_start_side(self.writing_mode, self.direction) == PhysicalSide::Top {
            origin + inline_size
        } else {
            origin
        }
    }

    pub(in crate::layout) fn physical_left_is_inline_end(self) -> bool {
        inline_end_side(self.writing_mode, self.direction) == PhysicalSide::Left
    }

    pub(in crate::layout) fn physical_right_is_inline_end(self) -> bool {
        inline_end_side(self.writing_mode, self.direction) == PhysicalSide::Right
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
    pub(in crate::layout) escaped_positioned_layers: Option<Rc<[PositionedPaintLayer]>>,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) alt_text: Option<Rc<str>>,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtom {
    pub(in crate::layout) data: Rc<InlineAtomData>,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) baseline_offset: f32,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

impl InlineAtom {
    #[allow(clippy::arc_with_non_send_sync, clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        content: InlineAtomContent,
        style: ComputedStyle,
        escaped_positioned_layers: Option<Box<[PositionedPaintLayer]>>,
        width: f32,
        height: f32,
        baseline_offset: f32,
        baseline_shift: f32,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            data: Rc::new(InlineAtomData {
                content,
                style: Rc::new(style),
                escaped_positioned_layers: escaped_positioned_layers.map(Rc::from),
                link_target: link_target.map(Rc::from),
                alt_text: alt_text.map(Rc::from),
            }),
            width,
            height,
            baseline_offset,
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

    pub(in crate::layout) fn content(&self) -> &InlineAtomContent {
        &self.data.content
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
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
    pub(in crate::layout) positioning_containing_block_style: Option<ComputedStyle>,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFloat {
    pub(in crate::layout) data: Rc<InlineFloatData>,
}

impl InlineFloat {
    pub(in crate::layout) fn new(
        element: Element,
        signature: ElementSignature,
        style: ComputedStyle,
        positioning_containing_block_style: Option<ComputedStyle>,
    ) -> Self {
        Self {
            data: Rc::new(InlineFloatData {
                element,
                signature,
                style,
                positioning_containing_block_style,
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

    pub(in crate::layout) fn positioning_containing_block_style(&self) -> Option<&ComputedStyle> {
        self.data.positioning_containing_block_style.as_ref()
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum InlineAtomContent {
    Canvas,
    Image(DecodedPngImage),
    Svg {
        fill: Color,
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
    InlineFragment(PaintFragment),
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
    pub(in crate::layout) advance: f32,
    pub(in crate::layout) paint_extent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Default for InlineBreak {
    fn default() -> Self {
        Self { clear: Clear::None }
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
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) document_canvas_background: Option<ComputedStyle>,
    pub(in crate::layout) root_canvas_background_defined: bool,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout) inline_static_position: Option<InlineStaticPosition>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) escaped_atom_containing_block: Option<ContainingBlock>,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    pub(in crate::layout) fragment_top_offsets: Vec<f32>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout) definite_block_size_stack: Vec<Option<f32>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    pub(in crate::layout) containing_blocks: Vec<ContainingBlock>,
    pub(in crate::layout) list_stack: Vec<ListState>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) current_page_running_elements:
        HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) next_assignment_id: usize,
    pub(in crate::layout) assignment_capture_stack: Vec<Vec<AssignmentId>>,
    pub(in crate::layout) quote_depth: usize,
    pub(in crate::layout) ancestors: Vec<ElementSignature>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_float_fragments: Vec<PendingFloatPaintFragment>,
    pub(in crate::layout) pending_float_side_effects: Vec<PendingFloatSideEffects>,
    pub(in crate::layout) applied_clearance_count: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct AnchorText {
    pub(in crate::layout) content: String,
    pub(in crate::layout) before: String,
    pub(in crate::layout) after: String,
}
