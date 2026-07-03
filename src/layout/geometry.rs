//! Typed coordinate spaces used by layout before paint and PDF serialization.
//!
//! Quire has several necessary coordinate systems because CSS layout is not one
//! global `x/y` plane. Formatting contexts first resolve logical spec terms
//! such as inline/block axes, grid slots, table grid cells, and flex main/cross
//! axes; only later do those values become physical page geometry and finally
//! PDF user-space coordinates.
//!
//! The intended conversion pipeline is:
//!
//! 1. CSS logical geometry: [`LogicalPoint`], [`LogicalSize`],
//!    [`LogicalRect`], and [`LogicalInlineSpan`] model spec-language inline and
//!    block dimensions before a writing mode or `direction` has chosen physical
//!    sides.
//! 2. Formatting-context geometry: [`InlineRect`], [`BlockRect`],
//!    [`ContainerRect`], and table-grid rectangles represent
//!    resolved physical geometry local to the formatting context that produced
//!    it.
//! 3. Page layout geometry: [`PageTopRect`] represents boxes whose physical
//!    top edge is known in the CSS page box. This matches block layout's
//!    downward cursor while making the top-edge convention explicit.
//! 4. Paint geometry: [`PaintRect`], [`PaintPoint`], and [`PaintSize`] are
//!    page-local bottom-left-origin coordinates used by display-list items.
//! 5. PDF geometry: PDF writer code converts paint geometry into PDF user
//!    space only at serialization boundaries.
//!
//! Flex and table each add domain-specific adapters around this shared layer:
//! flex quarantines raw Taffy output in [`TaffyRect`], while table code
//! distinguishes logical slot-grid areas from physical table-grid boxes.
//!
//! This module intentionally does not define one universal rectangle type for
//! all layout. The common unification point is the conversion vocabulary:
//! logical axes go through [`FlowAxes`], formatting contexts produce typed
//! local physical rectangles, page-top rectangles bridge downward CSS layout to
//! bottom-left paint space, and the PDF backend owns the final user-space
//! projection.
//!
//! CSS references:
//! - CSS Writing Modes logical/physical axes:
//!   <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
//! - CSS Box model rectangles:
//!   <https://www.w3.org/TR/css-box-3/#box-model>
//! - CSS Paged Media page box:
//!   <https://www.w3.org/TR/css-page-3/#page-model>
//! - PDF user space:
//!   <https://www.iso.org/standard/75839.html>

use super::*;

/// Physical coordinates inside one layout container.
///
/// This is the post-writing-mode coordinate system used by layout and paint
/// builders before page offsets are applied. Its axes are physical x/y, not
/// CSS logical inline/block axes. CSS Writing Modes maps logical coordinates
/// into this space:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerSpace {}

/// Physical coordinates returned by the Taffy flex layout engine.
///
/// Taffy exposes layout locations in its own physical row/column model, using
/// `direction` and `flex-direction` inputs supplied by Quire's adapter. Values
/// in this space must be converted at the flex/Taffy boundary before being
/// stored as Quire layout geometry:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaffySpace {}

/// Physical coordinates inside an inline formatting context.
///
/// CSS Inline Layout positions text runs and atomic inline boxes in line boxes
/// after CSS Writing Modes has mapped logical inline/block axes to physical
/// directions. This space is for resolved inline line-fragment geometry before
/// final page paint projection:
/// <https://www.w3.org/TR/css-inline-3/#line-layout>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineSpace {}

/// Physical coordinates inside a block formatting context.
///
/// CSS 2.2 block formatting places block boxes in normal flow by resolving
/// physical inline offsets and advancing the block cursor downward. This space
/// names block-layout-local geometry before it is projected through
/// [`PageTopRect`] for painting:
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockSpace {}

/// A point in physical container coordinates.
pub(super) type ContainerPoint = euclid::Point2D<f32, ContainerSpace>;
/// A size in physical container coordinates.
pub(super) type ContainerSize = euclid::Size2D<f32, ContainerSpace>;
/// An axis-aligned rectangle in physical container coordinates.
pub(super) type ContainerRect = euclid::Rect<f32, ContainerSpace>;
/// A point in resolved inline formatting-context coordinates.
pub(super) type InlinePoint = euclid::Point2D<f32, InlineSpace>;
/// A vector in resolved inline formatting-context coordinates.
pub(super) type InlineVector = euclid::Vector2D<f32, InlineSpace>;
/// A size in resolved inline formatting-context coordinates.
pub(super) type InlineSize = euclid::Size2D<f32, InlineSpace>;
/// A rectangle in resolved inline formatting-context coordinates.
pub(super) type InlineRect = euclid::Rect<f32, InlineSpace>;
/// A point in block formatting-context coordinates.
pub(super) type BlockPoint = euclid::Point2D<f32, BlockSpace>;
/// A size in block formatting-context coordinates.
pub(super) type BlockSize = euclid::Size2D<f32, BlockSpace>;
/// A rectangle in block formatting-context coordinates.
pub(super) type BlockRect = euclid::Rect<f32, BlockSpace>;
/// A point in raw Taffy output coordinates.
pub(super) type TaffyPoint = euclid::Point2D<f32, TaffySpace>;
/// A size in raw Taffy output coordinates.
pub(super) type TaffySize = euclid::Size2D<f32, TaffySpace>;
/// An axis-aligned rectangle in raw Taffy output coordinates.
pub(super) type TaffyRect = euclid::Rect<f32, TaffySpace>;

/// A physical horizontal span in the CSS page box.
///
/// This represents a left-to-right interval along the page `x` axis after CSS
/// Writing Modes has already projected logical inline coordinates into
/// physical page coordinates. It is useful for float exclusion bands and line
/// box availability, where CSS 2.2 talks about line boxes being shortened by
/// floats inside the block formatting context:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PageInlineSpan {
    start_x: f32,
    width: f32,
}

impl PageInlineSpan {
    pub(super) fn new(start_x: f32, width: f32) -> Self {
        Self {
            start_x,
            width: width.max(0.0),
        }
    }

    pub(super) fn from_edges(left_x: f32, right_x: f32) -> Self {
        Self::new(left_x, (right_x - left_x).max(0.0))
    }

    pub(super) fn left_x(self) -> f32 {
        self.start_x
    }

    pub(super) fn right_x(self) -> f32 {
        self.start_x + self.width
    }

    pub(super) fn width(self) -> f32 {
        self.width
    }
}

/// A physical vertical span in the CSS page box.
///
/// This represents a top-to-bottom interval along the page `y` axis using the
/// top-edge convention common in CSS block layout. It bridges algorithms that
/// reason about rows, slabs, or fragment ranges before those ranges become
/// bottom-left-origin paint rectangles:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PageBlockSpan {
    top_y: f32,
    height: f32,
}

impl PageBlockSpan {
    pub(super) fn new(top_y: f32, height: f32) -> Self {
        Self {
            top_y,
            height: height.max(0.0),
        }
    }

    pub(super) fn from_edges(top_y: f32, bottom_y: f32) -> Self {
        Self::new(top_y, (top_y - bottom_y).max(0.0))
    }

    pub(super) fn top_y(self) -> f32 {
        self.top_y
    }

    pub(super) fn bottom_y(self) -> f32 {
        self.top_y - self.height
    }
}

/// A point in the CSS page box using the top-edge layout convention.
///
/// This point stores a physical `top_y` value. Use it for origins of formatting
/// contexts whose local coordinates increase downward, such as table grid boxes
/// and future grid containers, before converting their rectangles through
/// [`PageTopRect`]:
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PageTopPoint {
    x: f32,
    top_y: f32,
}

impl PageTopPoint {
    pub(super) const fn new(x: f32, top_y: f32) -> Self {
        Self { x, top_y }
    }

    pub(super) fn x(self) -> f32 {
        self.x
    }

    pub(super) fn top_y(self) -> f32 {
        self.top_y
    }
}

/// A page-space rectangle described by its physical top edge.
///
/// CSS layout code commonly knows a box's physical top edge and block size
/// because block layout starts at the page-area top and advances downward. This
/// type is the explicit bridge from that top-edge layout convention to
/// bottom-left paint/PDF rectangles:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PageTopRect {
    pub(super) x: f32,
    pub(super) top_y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl PageTopRect {
    pub(super) fn new(x: f32, top_y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            top_y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    pub(super) fn x(self) -> f32 {
        self.x
    }

    pub(super) fn top_y(self) -> f32 {
        self.top_y
    }

    pub(super) fn width(self) -> f32 {
        self.width
    }

    pub(super) fn height(self) -> f32 {
        self.height
    }

    pub(super) fn bottom_y(self) -> f32 {
        self.top_y - self.height
    }

    pub(super) fn paint_rect(self) -> PaintRect {
        PaintRect::new(
            PaintPoint::new(self.x, self.bottom_y()),
            PaintSize::new(self.width, self.height),
        )
    }

    pub(super) fn paint_clip(self) -> PaintClip {
        paint_rect_to_clip(self.paint_rect())
    }

    pub(super) fn overflow_clip(self) -> OverflowClip {
        OverflowClip::from_paint_rect(self.paint_rect())
    }

    pub(super) fn rendered_rect(self, fill: Option<Color>) -> RenderedRect {
        RenderedRect::from_paint_rect(self.paint_rect(), fill)
    }
}

/// Convert a paint-space rectangle into Quire's current paint clip primitive.
pub(super) fn paint_rect_to_clip(rect: PaintRect) -> PaintClip {
    PaintClip::from_paint_rect(rect)
}

/// Build a page-local paint rectangle from bottom-left physical coordinates.
///
/// This is the shared constructor for layout code that already has CSS paint
/// coordinates, not top-edge layout coordinates. Callers with a physical top
/// edge should use [`PageTopRect`] instead:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order>.
pub(super) fn paint_space_rect(x: f32, y: f32, width: f32, height: f32) -> PaintRect {
    PaintRect::new(
        PaintPoint::new(x, y),
        PaintSize::new(width.max(0.0), height.max(0.0)),
    )
}

/// Build a point in page-local paint coordinates.
///
/// Use this for text baselines, path points, and other coordinates that are
/// already in CSS paint space. Layout top-edge coordinates should first go
/// through [`PageTopRect`]:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order>.
pub(super) fn paint_space_point(x: f32, y: f32) -> PaintPoint {
    PaintPoint::new(x, y)
}

/// A point in CSS logical coordinates.
///
/// `inline` advances along the inline axis and `block` advances along the
/// block axis as defined by CSS Writing Modes. The physical axis and direction
/// are intentionally absent; callers must use [`FlowAxes`] to convert into a
/// physical coordinate system:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
// Retained as the spec-backed logical/physical conversion boundary for
// writing-mode work; current production callers still mostly use physical
// geometry directly, while unit tests lock down the conversion behavior.
#[allow(dead_code)]
pub(super) struct LogicalPoint {
    pub(super) inline: f32,
    pub(super) block: f32,
}

/// A size in CSS logical coordinates.
///
/// `inline` is the logical inline size and `block` is the logical block size.
/// In vertical writing modes this maps to physical height/width respectively:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
// See the `LogicalPoint` rationale above.
#[allow(dead_code)]
pub(super) struct LogicalSize {
    pub(super) inline: f32,
    pub(super) block: f32,
}

/// A one-dimensional interval on the CSS logical inline axis.
///
/// `start` is measured from logical inline-start and `size` is the available
/// inline measure. CSS Inline and CSS Writing Modes define line box contents
/// in this logical axis before mapping to physical x/y for painting:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LogicalInlineSpan {
    start: f32,
    size: f32,
}

impl LogicalInlineSpan {
    pub(super) fn new(start: f32, size: f32) -> Self {
        Self {
            start: start.max(0.0),
            size: size.max(0.0),
        }
    }

    pub(super) fn start(self) -> f32 {
        self.start
    }

    pub(super) fn size(self) -> f32 {
        self.size
    }

    pub(super) fn end(self) -> f32 {
        self.start + self.size
    }
}

/// An axis-aligned rectangle in CSS logical coordinates.
///
/// The origin is measured from logical inline-start and block-start. Mapping
/// to physical coordinates depends on writing mode and direction, so conversion
/// must go through [`FlowAxes`]:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
// See the `LogicalPoint` rationale above.
#[allow(dead_code)]
pub(super) struct LogicalRect {
    pub(super) origin: LogicalPoint,
    pub(super) size: LogicalSize,
}

/// Maps CSS logical inline/block coordinates into a physical container space.
///
/// CSS Writing Modes defines logical inline and block axes independently of
/// physical x/y axes. Keeping this conversion centralized prevents flex, grid,
/// table, and inline layout code from each open-coding `direction: rtl` and
/// vertical-writing-mode coordinate flips:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FlowAxes {
    pub(super) writing_mode: WritingMode,
    pub(super) direction: Direction,
}

impl FlowAxes {
    pub(super) const fn new(writing_mode: WritingMode, direction: Direction) -> Self {
        Self {
            writing_mode,
            direction,
        }
    }

    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self::new(style.writing_mode, style.direction)
    }

    // Retained with the logical geometry types as the single place for
    // writing-mode coordinate projection.
    #[allow(dead_code)]
    pub(super) fn inline_start_side(self) -> PhysicalSide {
        inline_start_side(self.writing_mode, self.direction)
    }

    // Retained with the logical geometry types as the single place for
    // writing-mode coordinate projection.
    #[allow(dead_code)]
    pub(super) fn block_start_side(self) -> PhysicalSide {
        block_start_side(self.writing_mode)
    }

    // Retained with the logical geometry types as the single place for
    // writing-mode coordinate projection.
    #[allow(dead_code)]
    pub(super) fn physical_size_from_logical(self, size: LogicalSize) -> ContainerSize {
        match self.writing_mode {
            WritingMode::HorizontalTb => ContainerSize::new(size.inline, size.block),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                ContainerSize::new(size.block, size.inline)
            }
        }
    }

    // Retained with the logical geometry types as the single place for
    // writing-mode coordinate projection.
    #[allow(dead_code)]
    pub(super) fn logical_size_from_physical(self, size: ContainerSize) -> LogicalSize {
        match self.writing_mode {
            WritingMode::HorizontalTb => LogicalSize {
                inline: size.width,
                block: size.height,
            },
            WritingMode::VerticalRl | WritingMode::VerticalLr => LogicalSize {
                inline: size.height,
                block: size.width,
            },
        }
    }

    // Retained with the logical geometry types as the single place for
    // writing-mode coordinate projection.
    #[allow(dead_code)]
    pub(super) fn rect_from_logical(
        self,
        containing: ContainerRect,
        logical: LogicalRect,
    ) -> ContainerRect {
        let size = self.physical_size_from_logical(logical.size);
        let inline_origin = self.physical_axis_origin(
            containing,
            self.inline_start_side(),
            logical.origin.inline,
            logical.size.inline,
        );
        let block_origin = self.physical_axis_origin(
            containing,
            self.block_start_side(),
            logical.origin.block,
            logical.size.block,
        );
        let origin = match self.writing_mode {
            WritingMode::HorizontalTb => ContainerPoint::new(inline_origin, block_origin),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                ContainerPoint::new(block_origin, inline_origin)
            }
        };
        ContainerRect::new(origin, size)
    }

    fn physical_axis_origin(
        self,
        containing: ContainerRect,
        side: PhysicalSide,
        start: f32,
        size: f32,
    ) -> f32 {
        match side {
            PhysicalSide::Left => containing.origin.x + start,
            PhysicalSide::Right => containing.origin.x + containing.size.width - start - size,
            PhysicalSide::Top => containing.origin.y + start,
            PhysicalSide::Bottom => containing.origin.y + containing.size.height - start - size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(width: f32, height: f32) -> ContainerRect {
        ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(width, height),
        )
    }

    #[test]
    fn maps_horizontal_ltr_and_rtl_inline_coordinates() {
        let ltr = FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr);
        let rtl = FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let logical = LogicalRect {
            origin: LogicalPoint {
                inline: 10.0,
                block: 5.0,
            },
            size: LogicalSize {
                inline: 30.0,
                block: 20.0,
            },
        };

        assert_eq!(
            ltr.rect_from_logical(rect(100.0, 80.0), logical),
            ContainerRect::new(
                ContainerPoint::new(10.0, 5.0),
                ContainerSize::new(30.0, 20.0)
            )
        );
        assert_eq!(
            rtl.rect_from_logical(rect(100.0, 80.0), logical),
            ContainerRect::new(
                ContainerPoint::new(60.0, 5.0),
                ContainerSize::new(30.0, 20.0)
            )
        );
    }

    #[test]
    fn maps_vertical_writing_modes() {
        let vertical_rl = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let vertical_lr = FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr);
        let logical = LogicalRect {
            origin: LogicalPoint {
                inline: 10.0,
                block: 5.0,
            },
            size: LogicalSize {
                inline: 30.0,
                block: 20.0,
            },
        };

        assert_eq!(
            vertical_rl.rect_from_logical(rect(100.0, 80.0), logical),
            ContainerRect::new(
                ContainerPoint::new(75.0, 10.0),
                ContainerSize::new(20.0, 30.0)
            )
        );
        assert_eq!(
            vertical_lr.rect_from_logical(rect(100.0, 80.0), logical),
            ContainerRect::new(
                ContainerPoint::new(5.0, 10.0),
                ContainerSize::new(20.0, 30.0)
            )
        );
    }

    #[test]
    fn page_top_rect_projects_to_bottom_left_paint_geometry() {
        let rect = PageTopRect::new(20.0, 180.0, 50.0, 30.0);

        assert_eq!(rect.bottom_y(), 150.0);
        assert_eq!(
            rect.paint_rect(),
            PaintRect::new(PaintPoint::new(20.0, 150.0), PaintSize::new(50.0, 30.0))
        );
        assert_eq!(
            rect.paint_clip(),
            PaintClip::from_paint_rect(paint_space_rect(20.0, 150.0, 50.0, 30.0))
        );
    }
}
