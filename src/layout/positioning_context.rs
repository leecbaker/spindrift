use super::*;

/// Page-local containing block for positioned descendants.
///
/// CSS Positioned Layout resolves absolute and fixed offsets against a
/// containing block. Spindrift stores that box in physical page coordinates using
/// a top edge (`top_y`) because layout cursors advance downward, while the
/// `height` remains the physical block extent used for percentage resolution:
/// <https://www.w3.org/TR/css-position-3/#def-cb>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ContainingBlock {
    pub(in crate::layout) rect: PageTopRect,
    /// Page containing the block-start fragment when known.
    ///
    /// Page ownership is required for nested positioned fragmentation. Normal
    /// formatting contexts that have not yet exported durable fragment
    /// metadata leave this unknown and use their source page as a fallback.
    pub(in crate::layout) origin_page_index: Option<usize>,
}

/// Identity of one atomic inline's temporary paint coordinate space.
///
/// Positioned containing blocks established inside the scratch layout retain
/// this identity even when their rectangles differ from the atomic root.
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>
/// <https://drafts.csswg.org/css-position-3/#def-cb>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct AtomicInlineCoordinateSpaceId(u64);

impl AtomicInlineCoordinateSpaceId {
    pub(in crate::layout) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Coordinate space owning positioned containing-block geometry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) enum PositionedCoordinateSpace {
    #[default]
    Page,
    AtomicInline(AtomicInlineCoordinateSpaceId),
}

/// Positioned containing-block geometry together with its coordinate owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PositionedContainingBlockContext {
    pub(in crate::layout) geometry: ContainingBlock,
    pub(in crate::layout) coordinate_space: PositionedCoordinateSpace,
}

impl PositionedContainingBlockContext {
    pub(in crate::layout) const fn page(geometry: ContainingBlock) -> Self {
        Self {
            geometry,
            coordinate_space: PositionedCoordinateSpace::Page,
        }
    }

    pub(in crate::layout) const fn in_space(
        geometry: ContainingBlock,
        coordinate_space: PositionedCoordinateSpace,
    ) -> Self {
        Self {
            geometry,
            coordinate_space,
        }
    }
}

impl std::ops::Deref for PositionedContainingBlockContext {
    type Target = ContainingBlock;

    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}

/// Positioning state retained while an atomic inline is laid out on its
/// temporary page.
///
/// A static inline-block does not establish an absolute-position containing
/// block. Its out-of-flow descendant therefore resolves insets and percentages
/// against the nearest outer positioned ancestor (or the initial containing
/// block), while automatic insets use the hypothetical normal-flow position
/// inside the inline-block's independent formatting context. Keeping those
/// rectangles together makes the scratch-page boundary explicit instead of
/// accidentally treating the temporary page as the initial containing block:
/// <https://www.w3.org/TR/css-position-3/#containing-block> and
/// <https://www.w3.org/TR/css-position-3/#static-position>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct EscapedAtomPositioningContext {
    pub(in crate::layout) actual_containing_block: ContainingBlock,
    pub(in crate::layout) static_position: AtomicInlineStaticPosition,
}

/// Atom-local source geometry for a hypothetical block-level positioned box.
///
/// The physical rectangle belongs to the atomic inline's scratch page, while
/// its logical start edges belong to the atom's own writing mode. Keeping
/// those values together prevents an enclosing page flow from retagging an
/// RTL or sideways static position as horizontal LTR.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct AtomicInlineStaticPosition {
    pub(in crate::layout) content_rect: PageTopRect,
    pub(in crate::layout) axes: WritingModeAxes,
}

impl AtomicInlineStaticPosition {
    pub(in crate::layout) fn new(content_rect: PageTopRect, axes: WritingModeAxes) -> Self {
        Self { content_rect, axes }
    }

    fn static_position_rectangle(self) -> StaticPositionRectangle {
        let area = match self.axes.physical_axis(LogicalAxis::Inline) {
            PhysicalAxis::Horizontal => PageTopRect::new(
                self.content_rect.x(),
                self.content_rect.top_y(),
                self.content_rect.width(),
                0.0,
            ),
            PhysicalAxis::Vertical => {
                let x = match self.axes.physical_side(LogicalSide::BlockStart) {
                    PhysicalSide::Left => self.content_rect.x(),
                    PhysicalSide::Right => self.content_rect.x() + self.content_rect.width(),
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("a vertical writing mode has a horizontal block axis")
                    }
                };
                PageTopRect::new(
                    x,
                    self.content_rect.top_y(),
                    0.0,
                    self.content_rect.height(),
                )
            }
        };
        StaticPositionRectangle {
            area,
            writing_mode: self.axes.writing_mode(),
            direction: self.axes.direction(),
            justify_items: css::SelfAlignment::NORMAL,
            align_items: css::SelfAlignment::NORMAL,
        }
    }

    pub(in crate::layout) fn in_atomic_space(self) -> AbsoluteStaticPosition {
        AbsoluteStaticPosition::from_page_rect(
            self.content_rect.x(),
            self.content_rect.x() + self.content_rect.width(),
            self.content_rect.top_y(),
        )
        .with_static_position_rectangle(self.static_position_rectangle())
    }

    pub(in crate::layout) fn in_page_owned_containing_block(
        self,
        containing_block: ContainingBlock,
    ) -> AbsoluteStaticPosition {
        let horizontal_offset = containing_block.x();
        let mut rectangle = self.static_position_rectangle();
        rectangle.area = PageTopRect::new(
            rectangle.area.x() + horizontal_offset,
            rectangle.area.top_y(),
            rectangle.area.width(),
            rectangle.area.height(),
        );
        AbsoluteStaticPosition::from_page_rect(
            self.content_rect.x() + horizontal_offset,
            self.content_rect.x() + self.content_rect.width() + horizontal_offset,
            self.content_rect.top_y(),
        )
        .with_static_position_rectangle(rectangle)
    }
}

/// Physical content-box geometry of a normal-flow containing block.
///
/// Every in-flow formatting context supplies its used content box while its
/// children lay out. Relatively positioned descendants resolve percentage
/// insets against that box, even though it does not create an absolute
/// positioning containing-block scope:
/// <https://www.w3.org/TR/css-position-3/#relative-positioning>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct NormalFlowRelativeContainingBlock {
    pub(in crate::layout) physical_content_width: PhysicalContentWidth,
    pub(in crate::layout) physical_content_height: Option<PhysicalContentHeight>,
}

impl ContainingBlock {
    pub(in crate::layout) fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self {
            rect,
            origin_page_index: None,
        }
    }

    pub(in crate::layout) fn on_page(mut self, page_index: usize) -> Self {
        self.origin_page_index = Some(page_index);
        self
    }

    /// Moves a page-local containing block with its committed fragmentainer.
    ///
    /// A positioned descendant captured on a temporary multicolumn page must
    /// resolve its insets against the matching destination fragmentainer when
    /// that page is replayed:
    /// <https://www.w3.org/TR/css-position-3/#def-cb> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn translated(mut self, translation: PaintTranslation) -> Self {
        self.rect = PageTopRect::new(
            self.rect.x() + translation.x,
            self.rect.top_y() + translation.y,
            self.rect.width(),
            self.rect.height(),
        );
        self
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.x()
    }

    pub(in crate::layout) fn top_y(self) -> f32 {
        self.rect.top_y()
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.width()
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.height()
    }
}

/// The layout-only physical-height fallback used to choose the available
/// inline size of an orthogonal descendant.
///
/// This remains distinct from [`PercentageBasis`]: CSS Writing Modes can use
/// a nearest-scroll-container or initial-containing-block fallback to fit
/// lines while CSS Sizing keeps percentage resolution indefinite.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum OrthogonalAvailableHeight {
    /// No nearer scroll container establishes a usable constraint.
    InitialContainingBlock(PhysicalContentHeight),
    /// The nearest scroll container establishes the available line measure.
    ///
    /// The stored value is already capped by the initial containing block.
    NearestScrollContainer(PhysicalContentHeight),
}

impl OrthogonalAvailableHeight {
    pub(in crate::layout) fn initial_containing_block(value: PhysicalContentHeight) -> Self {
        Self::InitialContainingBlock(value.non_negative())
    }

    pub(in crate::layout) fn nearest_scroll_container(value: PhysicalContentHeight) -> Self {
        Self::NearestScrollContainer(value.non_negative())
    }

    pub(in crate::layout) fn value(self) -> PhysicalContentHeight {
        match self {
            Self::InitialContainingBlock(value) | Self::NearestScrollContainer(value) => value,
        }
    }
}

/// Direct non-scrolling available-height constraint for an orthogonal child.
///
/// A fixed maximum or minimum floor selects the final line-fitting measure.
/// That used inline measure also determines an auto vertical box's physical
/// block-size contribution: the box must reserve every column produced by
/// the wrapped lines, rather than a max-content single-column contribution.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum DirectOrthogonalAvailableHeight {
    /// An authored definite physical height on the immediate containing
    /// block. Unlike an auto-size min/max fallback, this is the direct
    /// containing block's used size and is not capped by the ICB.
    Definite(PhysicalContentHeight),
    Maximum(PhysicalContentHeight),
    MinimumFloor(PhysicalContentHeight),
}

impl DirectOrthogonalAvailableHeight {
    pub(in crate::layout) fn value(self) -> PhysicalContentHeight {
        match self {
            Self::Definite(value) | Self::Maximum(value) | Self::MinimumFloor(value) => value,
        }
    }

    pub(in crate::layout) fn capped_by_initial_containing_block(
        self,
        initial: PhysicalContentHeight,
    ) -> Option<Self> {
        let value = self.value().points();
        match self {
            Self::Definite(_) => Some(self),
            _ => (value < initial.points()).then(|| match self {
                Self::Maximum(_) => {
                    Self::Maximum(PhysicalContentHeight::new(content_box_pt(value.max(0.0))))
                }
                Self::MinimumFloor(_) => {
                    Self::MinimumFloor(PhysicalContentHeight::new(content_box_pt(value.max(0.0))))
                }
                Self::Definite(_) => unreachable!("definite direct height returns before capping"),
            }),
        }
    }
}

/// The available inline-size measure selected for an automatic orthogonal
/// formatting context.
///
/// This is intentionally distinct from a CSS percentage basis. CSS Writing
/// Modes uses the selected measure to fit lines, while CSS Sizing still treats
/// the corresponding axis as indefinite unless the containing block has an
/// actual definite used size.
/// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum OrthogonalInlineMeasure {
    /// The immediate containing block has an actual definite used height.
    DefiniteContainingBlock(PhysicalContentHeight),
    /// The immediate auto-height containing block supplies a direct
    /// height/min-height/max-height constraint.
    DirectContainingBlock(DirectOrthogonalAvailableHeight),
    /// The nearest scroll container supplies the fallback measure.
    NearestScrollContainer(PhysicalContentHeight),
    /// No nearer source applies, so the initial containing block is used.
    InitialContainingBlock(PhysicalContentHeight),
}

impl OrthogonalInlineMeasure {
    pub(in crate::layout) fn value(self) -> PhysicalContentHeight {
        match self {
            Self::DefiniteContainingBlock(value)
            | Self::NearestScrollContainer(value)
            | Self::InitialContainingBlock(value) => value,
            Self::DirectContainingBlock(value) => value.value(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ChildAvailableSpace {
    pub(in crate::layout) writing_mode: WritingMode,
    /// Used physical width of this formatting context's content box.
    ///
    /// This is deliberately not its logical inline size: vertical descendants
    /// use the physical height instead. Keeping the physical projection here
    /// prevents callers from accidentally reusing a logical scalar across an
    /// orthogonal writing-mode boundary.
    pub(in crate::layout) physical_content_width: PhysicalContentWidth,
    /// Whether this physical width is definite for a direct orthogonal child.
    /// An auto-sized vertical block has a used physical width after layout,
    /// but its logical block-size remains indefinite while that child chooses
    /// its available inline space.
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) physical_width_is_definite: bool,
    /// Physical-width percentage basis exported only to an orthogonal child.
    ///
    /// An auto-sized vertical formatting context can obtain its used physical
    /// width from such a child. The child's physical `width` percentage must
    /// retain the containing context's available physical-width basis instead
    /// of resolving cyclically against that newly determined used width.
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) orthogonal_physical_width_percentage_basis: PhysicalContentWidth,
    /// Physical height of this formatting context's content box when it is a
    /// definite percentage basis. An orthogonal-flow fallback is deliberately
    /// not stored here: CSS Writing Modes can use that fallback to choose an
    /// available line measure without making percentages definite.
    ///
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-auto>
    pub(in crate::layout) physical_content_height: PercentageBasis<PhysicalContentHeight>,
    /// Scoped physical-height fallback used only for CSS Writing Modes
    /// orthogonal-flow layout when the actual physical height is indefinite.
    /// This tracks the nearest scroll container independently of percentage
    /// definiteness.
    pub(in crate::layout) orthogonal_available_height: OrthogonalAvailableHeight,
    /// A constrained auto-height formatting context can provide an available
    /// measure to its *direct* orthogonal child even when it is not a scroll
    /// container. Unlike [`Self::orthogonal_available_height`], this is not an
    /// ancestor-lookup policy and must not escape through an intervening
    /// formatting context.
    ///
    /// CSS Writing Modes selects a direct containing block's available size
    /// before it falls back to the nearest scroll container or ICB:
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
    pub(in crate::layout) direct_orthogonal_available_height:
        Option<DirectOrthogonalAvailableHeight>,
}

/// Provenance for the inline percentage basis used while collecting an
/// intrinsic inline contribution.
///
/// The line-breaking width remains available as a geometric constraint, but
/// it is not necessarily a definite percentage basis: intrinsic sizing may be
/// determining that very width. CSS Sizing requires cyclic percentages to act
/// as `auto` in that case.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum IntrinsicInlinePercentageBasisSource {
    MeasurementAvailableWidth,
}

pub(in crate::layout) type IntrinsicInlinePercentageBasis =
    PercentageBasis<ContentBoxLength, IntrinsicInlinePercentageBasisSource>;

impl ChildAvailableSpace {
    pub(in crate::layout) fn new(
        writing_mode: WritingMode,
        physical_content_width: PhysicalContentWidth,
        physical_width_is_definite: bool,
        physical_content_height: Option<PhysicalContentHeight>,
        fallback_physical_content_height: PhysicalContentHeight,
    ) -> Self {
        Self {
            writing_mode,
            physical_content_width: PhysicalContentWidth::new(content_box_pt(
                physical_content_width.points().max(0.0),
            )),
            physical_width_is_definite,
            orthogonal_physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                physical_content_width.points().max(0.0),
            )),
            physical_content_height: physical_content_height.map_or_else(
                PercentageBasis::indefinite,
                |height| {
                    PercentageBasis::definite(PhysicalContentHeight::new(content_box_pt(
                        height.points().max(0.0),
                    )))
                },
            ),
            orthogonal_available_height: OrthogonalAvailableHeight::initial_containing_block(
                fallback_physical_content_height,
            ),
            direct_orthogonal_available_height: None,
        }
    }

    /// Add the current formatting context's direct-only orthogonal measure.
    ///
    /// A non-scrolling `height`/`min-height`/`max-height` affects an immediate
    /// orthogonal child, but is deliberately not carried as an ancestor
    /// fallback through a same-writing-mode descendant.
    pub(in crate::layout) fn with_direct_orthogonal_available_height(
        mut self,
        height: Option<DirectOrthogonalAvailableHeight>,
    ) -> Self {
        self.direct_orthogonal_available_height = height;
        self
    }

    /// Replace the physical-width percentage basis for the immediate
    /// orthogonal child without changing this context's used content width.
    pub(in crate::layout) fn with_orthogonal_physical_width_percentage_basis(
        mut self,
        basis: PhysicalContentWidth,
    ) -> Self {
        self.orthogonal_physical_width_percentage_basis = basis;
        self
    }

    /// Replace the inherited orthogonal-flow fallback without changing the
    /// physical percentage basis.
    pub(in crate::layout) fn with_orthogonal_available_height(
        mut self,
        height: OrthogonalAvailableHeight,
    ) -> Self {
        self.orthogonal_available_height = match height {
            OrthogonalAvailableHeight::InitialContainingBlock(value) => {
                OrthogonalAvailableHeight::initial_containing_block(value)
            }
            OrthogonalAvailableHeight::NearestScrollContainer(value) => {
                OrthogonalAvailableHeight::nearest_scroll_container(value)
            }
        };
        self
    }

    /// Select the line-fitting measure for an automatic orthogonal child.
    ///
    /// The direct containing block wins over the nearest scroll container,
    /// which in turn wins over the initial containing block. A definite used
    /// containing-block height wins over all fallback sources, but remains
    /// separately represented by `physical_height_percentage_basis` for CSS
    /// percentage resolution.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn orthogonal_inline_measure(self) -> OrthogonalInlineMeasure {
        if let Some(value) = self.physical_content_height.value() {
            return OrthogonalInlineMeasure::DefiniteContainingBlock(value);
        }
        if let Some(value) = self.direct_orthogonal_available_height {
            return OrthogonalInlineMeasure::DirectContainingBlock(value);
        }
        match self.orthogonal_available_height {
            OrthogonalAvailableHeight::NearestScrollContainer(value) => {
                OrthogonalInlineMeasure::NearestScrollContainer(value)
            }
            OrthogonalAvailableHeight::InitialContainingBlock(value) => {
                OrthogonalInlineMeasure::InitialContainingBlock(value)
            }
        }
    }

    /// Return the physical-height component of the selected orthogonal line
    /// measure for legacy consumers that need a numeric available height.
    pub(in crate::layout) fn available_physical_height(self) -> PhysicalContentHeight {
        self.orthogonal_inline_measure().value()
    }

    /// The physical height percentage basis exported to descendants.
    ///
    /// This intentionally differs from [`Self::available_physical_height`]:
    /// fallback available space is not a definite CSS percentage basis.
    pub(in crate::layout) fn physical_height_percentage_basis(
        self,
    ) -> PercentageBasis<PhysicalContentHeight> {
        self.physical_content_height
    }

    pub(in crate::layout) fn logical_inline_size_for(
        self,
        writing_mode: WritingMode,
    ) -> LogicalInlineContentSize {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            LogicalInlineContentSize::new(
                self.orthogonal_inline_measure()
                    .value()
                    .content_box_length(),
            )
        } else {
            LogicalInlineContentSize::new(self.physical_content_width.content_box_length())
        }
    }

    /// Return the definite logical inline percentage basis for a descendant.
    ///
    /// Percentage resolution follows the descendant's logical inline axis,
    /// while line fitting may use a non-definite orthogonal fallback. Keeping
    /// these paths separate prevents intrinsic layout from resolving cyclic
    /// percentages against an initial-containing-block fallback.
    /// <https://drafts.csswg.org/css-sizing-3/#definite>
    pub(in crate::layout) fn logical_inline_percentage_basis_for(
        self,
        writing_mode: WritingMode,
    ) -> LogicalInlinePercentageBasis {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            self.physical_height_percentage_basis()
                .map_value(|height| LogicalInlineContentSize::new(height.content_box_length()))
        } else if self.physical_width_is_definite {
            PercentageBasis::definite(LogicalInlineContentSize::new(
                self.physical_content_width.content_box_length(),
            ))
        } else {
            PercentageBasis::indefinite()
        }
    }
}

/// Active axis-aligned overflow clipping rectangle.
///
/// CSS Overflow clips non-visible overflow to the box's overflow clip edge,
/// which defaults to the padding box for `overflow: hidden`:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct OverflowClip {
    pub(in crate::layout) rect: PaintRect,
    pub(in crate::layout) clips_x: bool,
    pub(in crate::layout) clips_y: bool,
    /// A `clip` axis has no scrollable overflow. This is distinct from
    /// `hidden`, `auto`, and `scroll`: static PDF layout can prove that paint
    /// beyond this edge is unreachable only for the non-scrollable case.
    pub(in crate::layout) non_scrollable_x: bool,
    pub(in crate::layout) non_scrollable_y: bool,
}

impl OverflowClip {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self {
            rect,
            clips_x: true,
            clips_y: true,
            non_scrollable_x: false,
            non_scrollable_y: false,
        }
    }

    /// Construct a clip while retaining whether each clipped axis is the
    /// non-scrollable CSS `clip` value rather than a scroll container.
    /// <https://drafts.csswg.org/css-overflow-3/#valdef-overflow-clip>
    pub(in crate::layout) fn from_paint_rect_with_axes_and_non_scrollable(
        rect: PaintRect,
        clips_x: bool,
        clips_y: bool,
        non_scrollable_x: bool,
        non_scrollable_y: bool,
    ) -> Self {
        Self {
            rect,
            clips_x,
            clips_y,
            non_scrollable_x: clips_x && non_scrollable_x,
            non_scrollable_y: clips_y && non_scrollable_y,
        }
    }

    /// Apply axis and scrollability metadata to an already resolved clip
    /// rectangle.
    pub(in crate::layout) fn with_axes_and_non_scrollable(
        mut self,
        clips_x: bool,
        clips_y: bool,
        non_scrollable_x: bool,
        non_scrollable_y: bool,
    ) -> Self {
        self.clips_x = clips_x;
        self.clips_y = clips_y;
        self.non_scrollable_x = clips_x && non_scrollable_x;
        self.non_scrollable_y = clips_y && non_scrollable_y;
        self
    }

    pub(in crate::layout) fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self::from_paint_rect(rect.paint_rect())
    }

    pub(in crate::layout) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    pub(in crate::layout) fn intersect(self, other: Self) -> Option<Self> {
        let rect = self.rect.intersection(&other.rect)?;
        Some(Self {
            rect,
            clips_x: self.clips_x || other.clips_x,
            clips_y: self.clips_y || other.clips_y,
            non_scrollable_x: self.non_scrollable_x || other.non_scrollable_x,
            non_scrollable_y: self.non_scrollable_y || other.non_scrollable_y,
        })
    }
}
