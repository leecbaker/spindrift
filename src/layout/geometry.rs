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
//! 2. Formatting-context geometry: [`InlineRect`], [`BlockRect`], [`GridRect`],
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

/// Physical coordinates inside a CSS grid container's content box.
///
/// CSS Grid places resolved item border boxes relative to the grid content box;
/// its physical block coordinate increases from the content box's top edge
/// before fragment replay projects it into page-top geometry:
/// <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridSpace {}

/// Physical coordinates spanning the assembled document canvas before one
/// canvas slice is projected onto its destination page.
///
/// Root backgrounds can position and repeat across the document canvas, while
/// PDF paint primitives remain page-local. This marker prevents those
/// intermediate coordinates from being passed to page-local paint APIs before
/// the page-projection boundary:
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentCanvasSpace {}

/// A point in physical container coordinates.
pub(super) type ContainerPoint = euclid::Point2D<f32, ContainerSpace>;
/// A size in physical container coordinates.
pub(super) type ContainerSize = euclid::Size2D<f32, ContainerSpace>;
/// A displacement in physical container coordinates.
pub(super) type ContainerVector = euclid::Vector2D<f32, ContainerSpace>;
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
/// A point in grid-container-local physical coordinates.
pub(super) type GridPoint = euclid::Point2D<f32, GridSpace>;
/// A size in grid-container-local physical coordinates.
pub(super) type GridSize = euclid::Size2D<f32, GridSpace>;
/// A rectangle in grid-container-local physical coordinates.
pub(super) type GridRect = euclid::Rect<f32, GridSpace>;
/// A point in raw Taffy output coordinates.
pub(super) type TaffyPoint = euclid::Point2D<f32, TaffySpace>;
/// A size in raw Taffy output coordinates.
pub(super) type TaffySize = euclid::Size2D<f32, TaffySpace>;
/// An axis-aligned rectangle in raw Taffy output coordinates.
pub(super) type TaffyRect = euclid::Rect<f32, TaffySpace>;
/// A point in the document canvas before page projection.
pub(super) type DocumentCanvasPoint = euclid::Point2D<f32, DocumentCanvasSpace>;
/// A size in the document canvas before page projection.
pub(super) type DocumentCanvasSize = euclid::Size2D<f32, DocumentCanvasSpace>;
/// An axis-aligned rectangle in the document canvas before page projection.
pub(super) type DocumentCanvasRect = euclid::Rect<f32, DocumentCanvasSpace>;

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

/// An absolute vertical coordinate in the CSS page box's top-edge layout
/// convention.
///
/// Unlike a block extent, this is a position: moving toward block-end
/// subtracts a layout length because page-top coordinates decrease downward.
/// Float clearance and continuation handling use it so a float bottom cannot
/// be accidentally supplied where an occupied height is required:
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::layout) struct PageTopBlockPosition(f32);

impl PageTopBlockPosition {
    pub(in crate::layout) const fn new(top_y: f32) -> Self {
        Self(top_y)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }

    pub(in crate::layout) fn toward_block_end(self, distance: LayoutLength) -> Self {
        Self(self.0 - distance.points())
    }

    /// Return the page position moved toward physical block-start by `distance`.
    pub(in crate::layout) fn toward_block_start(self, distance: LayoutLength) -> Self {
        Self(self.0 + distance.points())
    }

    /// Return the non-negative block-axis extent from this page position to a
    /// later block-end edge in page-top coordinates.
    pub(in crate::layout) fn block_extent_to(self, block_end: Self) -> LayoutLength {
        layout_pt((self.0 - block_end.0).max(0.0))
    }

    /// Return the position closest to page block-end.
    pub(in crate::layout) fn min(self, other: Self) -> Self {
        if self.0 <= other.0 { self } else { other }
    }
}

/// An absolute horizontal coordinate in the CSS page box.
///
/// Horizontal writing uses page `x` for inline placement, while vertical
/// writing uses the same physical axis for block progression. Keeping it a
/// position rather than a bare scalar prevents a vertical exclusion retry
/// from confusing a page coordinate with a line-width displacement.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::layout) struct PageInlinePosition(f32);

impl PageInlinePosition {
    pub(in crate::layout) const fn new(x: f32) -> Self {
        Self(x)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
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

    pub(super) fn from_inline_x_and_block_position(x: f32, top: PageTopBlockPosition) -> Self {
        Self::new(x, top.points())
    }

    pub(super) fn x(self) -> f32 {
        self.x
    }

    pub(super) fn top_y(self) -> f32 {
        self.top_y
    }
}

/// Euclid coordinate marker for page layout geometry described from its
/// physical top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PageTopSpace {}

type PageTopEuclidRect = euclid::Rect<f32, PageTopSpace>;
/// A page-space rectangle described by its physical top edge.
///
/// CSS layout code commonly knows a box's physical top edge and block size
/// because block layout starts at the page-area top and advances downward. This
/// type is the explicit bridge from that top-edge layout convention to
/// bottom-left paint/PDF rectangles:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(super) struct PageTopRect(PageTopEuclidRect);

/// Largest visual primitive extent retained in paint space. Layout continues
/// to use full CSS dimensions; this only prevents an enormous `f32` rectangle
/// from losing its nearby top edge when `top - height` is formed for painting.
/// A million points is far beyond a printable page while keeping sub-point
/// precision at ordinary page coordinates.
const MAX_PAINT_GEOMETRY_EXTENT: f32 = 1_000_000.0;

impl PageTopRect {
    pub(super) fn new(x: f32, top_y: f32, width: f32, height: f32) -> Self {
        Self(PageTopEuclidRect::new(
            euclid::Point2D::new(x, top_y),
            euclid::Size2D::new(width.max(0.0), height.max(0.0)),
        ))
    }

    pub(super) fn x(self) -> f32 {
        self.0.origin.x
    }

    pub(super) fn top_y(self) -> f32 {
        self.0.origin.y
    }

    pub(super) fn width(self) -> f32 {
        self.0.size.width
    }

    pub(super) fn height(self) -> f32 {
        self.0.size.height
    }

    pub(super) fn bottom_y(self) -> f32 {
        self.top_y() - self.height()
    }

    pub(super) fn paint_rect(self) -> PaintRect {
        let height = self.height().min(MAX_PAINT_GEOMETRY_EXTENT);
        PaintRect::new(
            PaintPoint::new(self.x(), self.top_y() - height),
            PaintSize::new(self.width().min(MAX_PAINT_GEOMETRY_EXTENT), height),
        )
    }

    pub(super) fn paint_clip(self) -> PaintClip {
        paint_rect_to_clip(self.paint_rect())
    }

    pub(super) fn overflow_clip(self) -> OverflowClip {
        OverflowClip::from_paint_rect(self.paint_rect())
    }

    pub(super) fn rendered_rect(self, fill: Option<CssColor>) -> RenderedRect {
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

/// Inset a bottom-left-origin paint rectangle by physical CSS box edges.
///
/// `euclid::Rect::inner_rect` assumes a downward-pointing y axis, while paint
/// space follows PDF's upward y axis. Keep this conversion at the paint
/// geometry boundary so CSS top and bottom edges remain explicit:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.iso.org/standard/75839.html>.
pub(super) fn inset_paint_rect(rect: PaintRect, edges: css::Edges) -> PaintRect {
    PaintRect::new(
        PaintPoint::new(rect.origin.x + edges.left, rect.origin.y + edges.bottom),
        PaintSize::new(
            (rect.size.width - edges.left - edges.right).max(0.0),
            (rect.size.height - edges.top - edges.bottom).max(0.0),
        ),
    )
}

/// Intersect two paint rectangles, preserving an empty rectangle on disjoint
/// inputs.
///
/// CSS background clipping can produce an empty used paint area. Returning a
/// zero-sized rectangle keeps that result explicit without forcing callers to
/// special-case `Option` before their normal nonpositive-area checks:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
pub(super) fn intersect_paint_rect_or_empty(left: PaintRect, right: PaintRect) -> PaintRect {
    let origin = PaintPoint::new(
        left.origin.x.max(right.origin.x),
        left.origin.y.max(right.origin.y),
    );
    left.intersection(&right)
        .unwrap_or_else(|| PaintRect::new(origin, PaintSize::new(0.0, 0.0)))
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

/// Project a grid-content-local rectangle into page top-edge geometry.
///
/// Grid item `y` coordinates advance downward from the container content top,
/// whereas page layout records physical top edges. Keep that inversion at the
/// grid replay boundary so grid-local geometry cannot leak into paint APIs:
/// <https://www.w3.org/TR/css-grid-1/#grid-item-placement> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
pub(super) fn grid_rect_to_page_top_rect(
    rect: GridRect,
    container_origin: PageTopPoint,
) -> PageTopRect {
    PageTopRect::new(
        container_origin.x() + rect.origin.x,
        container_origin.top_y() - rect.origin.y,
        rect.size.width.max(0.0),
        rect.size.height.max(0.0),
    )
}

/// A point in CSS logical coordinates.
///
/// `inline` advances along the inline axis and `block` advances along the
/// block axis as defined by CSS Writing Modes. The physical axis and direction
/// are intentionally absent; callers must use [`FlowAxes`] to convert into a
/// physical coordinate system:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
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
pub(super) struct LogicalSize {
    pub(super) inline: f32,
    pub(super) block: f32,
}

/// A CSS content-box size on the logical inline axis.
///
/// CSS Writing Modes maps the logical inline axis to physical width in
/// horizontal writing modes and physical height in vertical writing modes.
/// Wrapping the existing content-box semantic length keeps box-model space
/// distinct from axis identity:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-box-3/#content-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalInlineContentSize(ContentBoxLength);

impl LogicalInlineContentSize {
    pub(in crate::layout) fn new(value: ContentBoxLength) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }

    /// Keep the larger of two contributions measured on the same logical
    /// inline content-box axis.
    pub(in crate::layout) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(in crate::layout) fn content_box_length(self) -> ContentBoxLength {
        self.0
    }
}

impl SemanticLengthExt for LogicalInlineContentSize {
    fn points(self) -> f32 {
        self.0.points()
    }
}

impl crate::units::IntoLayoutLength for LogicalInlineContentSize {
    fn into_layout_length(self) -> LayoutLength {
        crate::units::IntoLayoutLength::into_layout_length(self.0)
    }
}

/// A percentage basis whose value is explicitly a logical inline content-box
/// size. CSS Box percentage edges must cross this boundary before becoming
/// physical edges.
pub(in crate::layout) type LogicalInlinePercentageBasis<Source = ()> =
    PercentageBasis<LogicalInlineContentSize, Source>;

/// A CSS content-box size on the logical block axis.
///
/// In vertical writing modes this maps to physical width, which is the key
/// distinction needed when applying physical `width` properties through
/// logical CSS sizing algorithms:
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalBlockContentSize(ContentBoxLength);

impl LogicalBlockContentSize {
    pub(in crate::layout) fn new(value: ContentBoxLength) -> Self {
        Self(value)
    }

    #[allow(
        dead_code,
        reason = "logical block scalar access is staged for broader axis-typed sizing callers"
    )]
    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }

    pub(in crate::layout) fn content_box_length(self) -> ContentBoxLength {
        self.0
    }
}

/// A CSS content-box size projected onto the physical width axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PhysicalContentWidth(ContentBoxLength);

impl PhysicalContentWidth {
    pub(in crate::layout) fn new(value: ContentBoxLength) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }

    pub(in crate::layout) fn non_negative(self) -> Self {
        Self::new(self.0.max(content_box_pt(0.0)))
    }

    pub(in crate::layout) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(in crate::layout) fn content_box_length(self) -> ContentBoxLength {
        self.0
    }
}

/// A CSS content-box size projected onto the physical height axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PhysicalContentHeight(ContentBoxLength);

impl PhysicalContentHeight {
    pub(in crate::layout) fn new(value: ContentBoxLength) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }

    /// Clamp a physical content-box extent at the zero-size boundary.
    ///
    /// Heights that model available layout space cannot be negative, even
    /// when their source constraint is over-constrained.
    pub(in crate::layout) fn non_negative(self) -> Self {
        Self::new(self.0.max(content_box_pt(0.0)))
    }

    pub(in crate::layout) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(in crate::layout) fn content_box_length(self) -> ContentBoxLength {
        self.0
    }
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
    axes: WritingModeAxes,
}

impl FlowAxes {
    pub(super) const fn new(writing_mode: WritingMode, direction: Direction) -> Self {
        Self {
            axes: WritingModeAxes::new(writing_mode, direction),
        }
    }

    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self::new(style.writing_mode, style.used_direction())
    }

    pub(super) fn inline_start_side(self) -> PhysicalSide {
        self.axes.physical_side(LogicalSide::InlineStart)
    }

    /// Return the physical edge at the containing flow's logical block-start.
    ///
    /// Normal-flow static placement uses the containing block's axes, even
    /// when an orthogonal child's sizing algorithm used different axes:
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>.
    pub(super) fn block_start_side(self) -> PhysicalSide {
        self.axes.physical_side(LogicalSide::BlockStart)
    }

    /// Return the physical edge used by `text-align: left`.
    pub(super) fn line_left_side(self) -> PhysicalSide {
        self.axes.line_left_side()
    }

    /// Return the physical edge used by `text-align: right`.
    pub(super) fn line_right_side(self) -> PhysicalSide {
        self.axes.line_right_side()
    }

    /// Project a logical inline/block pair into physical horizontal/vertical
    /// order. Layout adapters use this at their physical backend boundary so
    /// they do not duplicate writing-mode swaps.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    pub(super) fn physical_size<T>(self, inline: T, block: T) -> (T, T) {
        self.axes.physical_size(inline, block)
    }

    /// Translate continuous logical block coordinates into a fragmentainer's
    /// local paint coordinates.
    ///
    /// A later multicolumn source slice restarts its temporary page at logical
    /// block zero.  Paint resolved against the continuous containing block
    /// must therefore move back by the slice's logical block origin before
    /// the source-to-destination fragmentainer projection is applied.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn continuous_block_to_local_paint_translation(
        self,
        source_block_start: LayoutLength,
    ) -> PaintTranslation {
        let offset = source_block_start.points();
        match self.block_start_side() {
            // Logical block progression is downward in page-top coordinates
            // but upward in paint coordinates, so continuous-to-local is its
            // inverse.
            PhysicalSide::Top => PaintTranslation::new(0.0, offset),
            PhysicalSide::Bottom => PaintTranslation::new(0.0, -offset),
            PhysicalSide::Left => PaintTranslation::new(-offset, 0.0),
            PhysicalSide::Right => PaintTranslation::new(offset, 0.0),
        }
    }

    pub(super) const fn writing_mode(self) -> WritingMode {
        self.axes.writing_mode()
    }

    pub(super) fn physical_size_from_logical(self, size: LogicalSize) -> ContainerSize {
        let (width, height) = self.axes.physical_size(size.inline, size.block);
        ContainerSize::new(width, height)
    }

    /// Project logical content-box sizes onto the physical width axis.
    ///
    /// Width is a physical property, but CSS Writing Modes requires box sizing
    /// algorithms to operate in logical axes before projection:
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
    pub(in crate::layout) fn physical_width_from_logical_content_sizes(
        self,
        inline: LogicalInlineContentSize,
        block: LogicalBlockContentSize,
    ) -> PhysicalContentWidth {
        if self.axes.swaps_physical_axes() {
            PhysicalContentWidth::new(block.content_box_length())
        } else {
            PhysicalContentWidth::new(inline.content_box_length())
        }
    }

    /// Project logical content-box sizes onto the physical height axis.
    #[allow(
        dead_code,
        reason = "height projection is added with width projection so both physical axes share one typed model"
    )]
    pub(in crate::layout) fn physical_height_from_logical_content_sizes(
        self,
        inline: LogicalInlineContentSize,
        block: LogicalBlockContentSize,
    ) -> PhysicalContentHeight {
        if self.axes.swaps_physical_axes() {
            PhysicalContentHeight::new(inline.content_box_length())
        } else {
            PhysicalContentHeight::new(block.content_box_length())
        }
    }

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
            self.axes.physical_side(LogicalSide::BlockStart),
            logical.origin.block,
            logical.size.block,
        );
        let (x, y) = self.axes.physical_size(inline_origin, block_origin);
        let origin = ContainerPoint::new(x, y);
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
    fn localizes_continuous_block_offsets_in_each_writing_mode() {
        let offset = layout_pt(12.0);
        for (writing_mode, expected) in [
            (WritingMode::HorizontalTb, PaintTranslation::new(0.0, 12.0)),
            (WritingMode::VerticalLr, PaintTranslation::new(-12.0, 0.0)),
            (WritingMode::VerticalRl, PaintTranslation::new(12.0, 0.0)),
            (WritingMode::SidewaysLr, PaintTranslation::new(-12.0, 0.0)),
            (WritingMode::SidewaysRl, PaintTranslation::new(12.0, 0.0)),
        ] {
            assert_eq!(
                FlowAxes::new(writing_mode, Direction::Ltr)
                    .continuous_block_to_local_paint_translation(offset),
                expected,
                "{writing_mode:?} must restore a later source slice to local paint coordinates",
            );
        }
    }

    #[test]
    fn maps_all_writing_modes_to_physical_rects() {
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

        for (writing_mode, direction, expected) in [
            (
                WritingMode::HorizontalTb,
                Direction::Ltr,
                ContainerRect::new(
                    ContainerPoint::new(10.0, 5.0),
                    ContainerSize::new(30.0, 20.0),
                ),
            ),
            (
                WritingMode::HorizontalTb,
                Direction::Rtl,
                ContainerRect::new(
                    ContainerPoint::new(60.0, 5.0),
                    ContainerSize::new(30.0, 20.0),
                ),
            ),
            (
                WritingMode::VerticalRl,
                Direction::Ltr,
                ContainerRect::new(
                    ContainerPoint::new(75.0, 10.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::VerticalRl,
                Direction::Rtl,
                ContainerRect::new(
                    ContainerPoint::new(75.0, 40.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::VerticalLr,
                Direction::Ltr,
                ContainerRect::new(
                    ContainerPoint::new(5.0, 10.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::VerticalLr,
                Direction::Rtl,
                ContainerRect::new(
                    ContainerPoint::new(5.0, 40.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Ltr,
                ContainerRect::new(
                    ContainerPoint::new(75.0, 10.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Rtl,
                ContainerRect::new(
                    ContainerPoint::new(75.0, 40.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Ltr,
                ContainerRect::new(
                    ContainerPoint::new(5.0, 40.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Rtl,
                ContainerRect::new(
                    ContainerPoint::new(5.0, 10.0),
                    ContainerSize::new(20.0, 30.0),
                ),
            ),
        ] {
            assert_eq!(
                FlowAxes::new(writing_mode, direction)
                    .rect_from_logical(rect(100.0, 80.0), logical),
                expected,
                "{writing_mode:?} {direction:?}"
            );
        }
    }

    #[test]
    fn projects_logical_content_sizes_to_physical_axes() {
        let inline = LogicalInlineContentSize::new(content_box_pt(40.0));
        let block = LogicalBlockContentSize::new(content_box_pt(20.0));

        for (writing_mode, width, height) in [
            (WritingMode::HorizontalTb, 40.0, 20.0),
            (WritingMode::VerticalRl, 20.0, 40.0),
            (WritingMode::VerticalLr, 20.0, 40.0),
            (WritingMode::SidewaysRl, 20.0, 40.0),
            (WritingMode::SidewaysLr, 20.0, 40.0),
        ] {
            let axes = FlowAxes::new(writing_mode, Direction::Ltr);
            assert_eq!(
                axes.physical_width_from_logical_content_sizes(inline, block)
                    .points(),
                width
            );
            assert_eq!(
                axes.physical_height_from_logical_content_sizes(inline, block)
                    .points(),
                height
            );
        }
    }

    #[test]
    fn page_top_rect_projects_to_bottom_left_paint_geometry() {
        let rect = PageTopRect::new(20.0, 180.0, 50.0, 30.0);

        assert_eq!(
            rect.0,
            PageTopEuclidRect::new(
                euclid::Point2D::new(20.0, 180.0),
                euclid::Size2D::new(50.0, 30.0),
            )
        );
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

    #[test]
    fn paint_rect_inset_respects_upward_paint_coordinates() {
        let rect = paint_space_rect(10.0, 20.0, 100.0, 80.0);
        let inset = inset_paint_rect(
            rect,
            css::Edges {
                top: 7.0,
                right: 11.0,
                bottom: 13.0,
                left: 17.0,
            },
        );

        assert_eq!(inset, paint_space_rect(27.0, 33.0, 72.0, 60.0));
    }

    #[test]
    fn disjoint_paint_rect_intersection_is_empty_at_shared_corner() {
        let intersection = intersect_paint_rect_or_empty(
            paint_space_rect(10.0, 20.0, 5.0, 5.0),
            paint_space_rect(30.0, 40.0, 5.0, 5.0),
        );

        assert_eq!(intersection, paint_space_rect(30.0, 40.0, 0.0, 0.0));
    }

    #[test]
    fn grid_rect_projects_local_downward_y_to_page_top_geometry() {
        let rect = GridRect::new(GridPoint::new(15.0, 40.0), GridSize::new(60.0, 25.0));

        assert_eq!(
            grid_rect_to_page_top_rect(
                rect,
                PageTopPoint::from_inline_x_and_block_position(
                    100.0,
                    PageTopBlockPosition::new(300.0),
                ),
            ),
            PageTopRect::new(115.0, 260.0, 60.0, 25.0)
        );
    }

    #[test]
    fn page_top_block_position_keeps_block_end_motion_explicit() {
        let top = PageTopBlockPosition::new(300.0);
        let bottom = top.toward_block_end(layout_pt(25.0));

        assert_eq!(bottom, PageTopBlockPosition::new(275.0));
        assert_eq!(top.block_extent_to(bottom), layout_pt(25.0));
    }
}
