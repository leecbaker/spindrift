use super::super::super::*;

/// Tracks a legacy immediate float row view for callers that need the first
/// line's already-placed exclusions.
///
/// CSS 2.2 floats are shifted to the line's left or right edge and subsequent
/// floats are placed beside previous floats when space permits:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatRunState {
    /// Full physical row span before same-row floats shorten it.
    ///
    /// CSS 2.2 places consecutive floats beside earlier floats when possible.
    /// This span is page physical `x` in the current block formatting context:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) row_span: PageInlineSpan,
    /// Remaining physical row span after same-row floats have been included.
    ///
    /// This is the immediate line-box availability for legacy float placement
    /// callers; durable later exclusions are stored as [`FloatShape`] entries
    /// in [`FloatContext`].
    pub(in crate::layout) available_span: PageInlineSpan,
    /// Physical block interval occupied by same-row floats.
    ///
    /// The span uses Quire's page top-edge convention: `top_y` is the row top
    /// and `bottom_y` moves downward as floats are added. CSS floats shorten
    /// later line boxes until the lowest same-row float bottom:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) occupied_block_span: PageBlockSpan,
    pub(in crate::layout) active: bool,
}

/// Durable float exclusion list for one block formatting context.
///
/// CSS 2.2 keeps floated margin boxes out of normal flow but shortens later
/// line boxes and formatting contexts around them in the same block formatting
/// context:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatContext {
    pub(in crate::layout) shapes: Vec<FloatShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct FloatId(pub(in crate::layout) usize);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatShape {
    pub(in crate::layout) id: FloatId,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) rect: PageTopRect,
}

impl FloatShape {
    pub(in crate::layout) fn from_rect(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        page_index: usize,
        rect: PageTopRect,
    ) -> Self {
        Self {
            id,
            specified_side,
            side,
            source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            page_index,
            rect,
        }
    }

    pub(in crate::layout) fn from_fragment(fragment: &FloatPaintFragment) -> Self {
        Self {
            id: fragment.id,
            specified_side: fragment.specified_side,
            side: fragment.side,
            source_order: fragment.source_order,
            fragment_index: fragment.fragment_index,
            starts_on_previous_page: fragment.starts_on_previous_page,
            continues_on_next_page: fragment.continues_on_next_page,
            page_index: fragment.page_index,
            rect: fragment.rect,
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn from_edges(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        fragment_index: usize,
        starts_on_previous_page: bool,
        continues_on_next_page: bool,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Self {
        Self {
            id,
            specified_side,
            side,
            source_order,
            fragment_index,
            starts_on_previous_page,
            continues_on_next_page,
            page_index,
            rect: PageTopRect::new(left, top, (right - left).max(0.0), (top - bottom).max(0.0)),
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.rect.x()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.rect.x() + self.rect.width()
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.rect.top_y()
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.rect.bottom_y()
    }

    pub(in crate::layout) fn translated_block(self, delta_y: f32) -> Self {
        Self {
            rect: PageTopRect::new(
                self.rect.x(),
                self.rect.top_y() + delta_y,
                self.rect.width(),
                self.rect.height(),
            ),
            ..self
        }
    }
}

/// Durable page-local representation of one floated box fragment.
///
/// CSS 2.2 floats exclude later content using their margin boxes, while CSS
/// Fragmentation can split a floated box across page fragmentainers. Each
/// visible fragment therefore needs both a paint-tree context and a page-local
/// exclusion shape:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatPaintFragment {
    pub(in crate::layout) id: FloatId,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) rect: PageTopRect,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) context: PaintStackingContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum UsedFloatSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl UsedFloatSide {
    pub(in crate::layout) fn from_float(
        float: Float,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> Option<Self> {
        match float {
            Float::None => None,
            Float::Left => Some(Self::Left),
            Float::Right => Some(Self::Right),
            Float::InlineStart => Some(Self::from_physical_side(inline_start_side(
                writing_mode,
                direction,
            ))),
            Float::InlineEnd => Some(Self::from_physical_side(inline_end_side(
                writing_mode,
                direction,
            ))),
        }
    }

    pub(in crate::layout) fn from_physical_side(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Left => Self::Left,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Bottom => Self::Bottom,
        }
    }

    pub(in crate::layout) fn matches_clear(
        self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> bool {
        let clear_side = match clear {
            Clear::None => return false,
            Clear::Both => return true,
            Clear::Left => Self::Left,
            Clear::Right => Self::Right,
            Clear::InlineStart => {
                Self::from_physical_side(inline_start_side(writing_mode, direction))
            }
            Clear::InlineEnd => Self::from_physical_side(inline_end_side(writing_mode, direction)),
        };
        self == clear_side
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatBand {
    /// The remaining physical line-box span in page coordinates after active
    /// CSS floats have shortened the row.
    ///
    /// CSS 2.2 defines floats as shortening line boxes in the same block
    /// formatting context. The span is physical page `x`, not logical inline
    /// coordinates; vertical writing modes must use [`LogicalFloatBand`]:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) span: PageInlineSpan,
}

impl FloatBand {
    pub(in crate::layout) fn from_edges(left: f32, right: f32) -> Self {
        Self {
            span: PageInlineSpan::from_edges(left, right),
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.span.left_x()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.span.right_x()
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.span.width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalFloatBand {
    /// Available logical inline interval after float exclusions.
    ///
    /// CSS Writing Modes defines inline coordinates independently from the
    /// physical page axis. This span is logical inline progress inside the
    /// queried line/slab, after active CSS floats have shortened it:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) inline_span: LogicalInlineSpan,
    /// Physical page-y interval that corresponds to the available inline slab.
    ///
    /// Vertical writing modes can shorten the physical top or bottom of the
    /// slab while still reporting a logical inline span to inline layout.
    pub(in crate::layout) block_span: PageBlockSpan,
}

impl LogicalFloatBand {
    pub(in crate::layout) fn new(
        inline_start: f32,
        inline_size: f32,
        physical_top: f32,
        physical_bottom: f32,
    ) -> Self {
        Self {
            inline_span: LogicalInlineSpan::new(inline_start, inline_size),
            block_span: PageBlockSpan::from_edges(physical_top, physical_bottom),
        }
    }

    pub(in crate::layout) fn inline_start(self) -> f32 {
        self.inline_span.start()
    }

    pub(in crate::layout) fn inline_end(self) -> f32 {
        self.inline_span.end()
    }

    pub(in crate::layout) fn available_inline_size(self) -> f32 {
        self.inline_span.size()
    }

    pub(in crate::layout) fn physical_top(self) -> f32 {
        self.block_span.top_y()
    }

    pub(in crate::layout) fn physical_bottom(self) -> f32 {
        self.block_span.bottom_y()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatPlacement {
    /// Physical top-left placement of the float margin box in page-top space.
    ///
    /// CSS 2.2 places a float as far left or right as possible while its top
    /// edge is at or below the current line, after `clear` and active float
    /// exclusions are applied. The top-edge convention matches block layout's
    /// downward cursor before paint conversion:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) origin: PageTopPoint,
    /// Physical line-box span available at this float's block position.
    ///
    /// CSS floats shorten later line boxes in the same block formatting
    /// context. This span is the page-local horizontal band that accepted the
    /// float, not a CSS logical inline interval; vertical-writing float
    /// avoidance maps its logical inline availability into this typed result.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) available_span: PageInlineSpan,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatAvoidingBfcMeasurement {
    /// Physical inline-start edge of the BFC root's border box before its
    /// relative-position offset. Float collision is defined against this
    /// normal-flow border box, not merely its width.
    pub(in crate::layout) border_box_left: f32,
    pub(in crate::layout) border_box_width: f32,
    pub(in crate::layout) border_box_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatAvoidingBfcPlacement {
    /// Physical border-box inline-start selected by the fixed-point
    /// measurement. This differs from `left`, the residual float-band edge,
    /// whenever the normal block-width equation gives the BFC root a used
    /// start margin.
    pub(in crate::layout) border_box_left: f32,
    pub(in crate::layout) left: f32,
    pub(in crate::layout) top: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) border_box_width: f32,
    pub(in crate::layout) border_box_height: f32,
}

impl FloatPlacement {
    pub(in crate::layout) fn new(left: f32, top: f32, available_width: f32) -> Self {
        Self {
            origin: PageTopPoint::new(left, top),
            available_span: PageInlineSpan::new(left, available_width),
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.origin.x()
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.origin.top_y()
    }

    pub(in crate::layout) fn available_width(self) -> f32 {
        self.available_span.width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatClearanceResolution {
    pub(in crate::layout) top: f32,
    pub(in crate::layout) continued_float: Option<FloatId>,
}
