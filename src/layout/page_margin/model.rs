use super::*;
use crate::layout::page_generated::ResolvedPageContent;

pub(in crate::layout) struct PageMarginBoxLayout<'a> {
    pub(in crate::layout) spec: &'a PageMarginBoxSpec,
    pub(in crate::layout) content: ResolvedPageContent,
    /// Border box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// CSS Paged Media defines generated page-margin boxes around the page
    /// area. At this point their used rectangles have already been projected
    /// into Spindrift paint space: origin at the page bottom-left, `x` increasing
    /// rightward, and `y` increasing upward:
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>.
    pub(in crate::layout) border_rect: PaintRect,
    /// Content box of a CSS page-margin box in page-local paint coordinates.
    ///
    /// This is the containing area for generated margin-box inline content
    /// after margin, border, and padding have been applied according to the CSS
    /// box model:
    /// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
    pub(in crate::layout) content_rect: PaintRect,
}

impl PageMarginBoxLayout<'_> {
    pub(in crate::layout) fn border_clip(&self) -> PaintClip {
        PaintClip::from_paint_rect(self.border_rect)
    }

    pub(in crate::layout) fn content_x(&self) -> f32 {
        self.content_rect.min_x()
    }

    pub(in crate::layout) fn content_y(&self) -> f32 {
        self.content_rect.min_y()
    }

    pub(in crate::layout) fn content_width(&self) -> f32 {
        self.content_rect.width()
    }

    pub(in crate::layout) fn content_height(&self) -> f32 {
        self.content_rect.height()
    }
}

pub(in crate::layout) struct PageMarginPaintedBox {
    pub(in crate::layout) z_index: i32,
    pub(in crate::layout) order: usize,
    pub(in crate::layout) effects: PaintEffects,
    pub(in crate::layout) bounds: PaintClip,
    pub(in crate::layout) fragment: PaintFragment,
}
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginLogicalInlineSize(f32);

impl PageMarginLogicalInlineSize {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginLogicalBlockSize(f32);

impl PageMarginLogicalBlockSize {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginPhysicalX(f32);

impl PageMarginPhysicalX {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginPhysicalY(f32);

impl PageMarginPhysicalY {
    pub(in crate::layout) fn new(points: f32) -> Self {
        Self(points)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginFixedBoxGeometry {
    line_block_start_x: PageMarginPhysicalX,
    line_inline_start_y: PageMarginPhysicalY,
    inline_size: PageMarginLogicalInlineSize,
    block_size: PageMarginLogicalBlockSize,
}

impl PageMarginFixedBoxGeometry {
    /// Convert a page-margin content rectangle into logical fixed-box axes.
    ///
    /// CSS Paged Media gives margin boxes fixed physical rectangles, while CSS
    /// Writing Modes maps inline layout through logical inline and block axes.
    /// Keeping this conversion typed makes it explicit that vertical writing
    /// uses the content box's physical height as logical inline size and its
    /// physical width as logical block size:
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) fn from_layout(layout: &PageMarginBoxLayout<'_>) -> Self {
        let style = &layout.spec.style;
        let physical_left = PageMarginPhysicalX(layout.content_x());
        let physical_right = PageMarginPhysicalX(layout.content_x() + layout.content_width());
        let physical_top = PageMarginPhysicalY(layout.content_y() + layout.content_height());
        let (inline_size, block_size) = match style.writing_mode {
            WritingMode::HorizontalTb => (
                PageMarginLogicalInlineSize(layout.content_width().max(1.0)),
                PageMarginLogicalBlockSize(layout.content_height().max(0.0)),
            ),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => (
                PageMarginLogicalInlineSize(layout.content_height().max(1.0)),
                PageMarginLogicalBlockSize(layout.content_width().max(0.0)),
            ),
        };
        let line_block_start_x = match style.writing_mode {
            // The block-start edge of a right-to-left vertical writing mode
            // is the physical right edge. Inline line layout subtracts each
            // line's block size from this origin as it advances leftward.
            WritingMode::VerticalRl | WritingMode::SidewaysRl => physical_right,
            WritingMode::HorizontalTb | WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                physical_left
            }
        };
        Self {
            line_block_start_x,
            line_inline_start_y: physical_top,
            inline_size,
            block_size,
        }
    }

    pub(in crate::layout) fn inline_size(self) -> PageMarginLogicalInlineSize {
        self.inline_size
    }

    pub(in crate::layout) fn block_size(self) -> PageMarginLogicalBlockSize {
        self.block_size
    }

    pub(in crate::layout) fn line_block_start_x(self) -> PageMarginPhysicalX {
        self.line_block_start_x
    }

    pub(in crate::layout) fn line_inline_start_y(self) -> PageMarginPhysicalY {
        self.line_inline_start_y
    }

    pub(in crate::layout) fn with_line_inline_start(mut self, y: PageMarginPhysicalY) -> Self {
        self.line_inline_start_y = y;
        self
    }

    pub(in crate::layout) fn with_line_block_alignment(
        mut self,
        _line_stack_block_size: f32,
        first_line_block_size: f32,
        _vertical_align: VerticalAlign,
        writing_mode: WritingMode,
    ) -> Self {
        // Vertical right-to-left line layout starts a column by painting to
        // the left of its block cursor. Place that first cursor one line
        // block-size inside the physical right edge; subsequent columns then
        // advance left without escaping the margin-box content rectangle.
        // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
        if matches!(
            writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        ) {
            self.line_block_start_x.0 -= first_line_block_size;
        }
        self
    }
}
