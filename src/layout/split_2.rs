use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::document::paint::patterns::RenderedImageSourceRect;
use crate::dom::ElementId;
use crate::image_store::ImageId;
use crate::layout::assets::DocumentPageIndex;
use crate::layout::text_paint::TextInlineSpan;

/// Page-local containing block for positioned descendants.
///
/// CSS Positioned Layout resolves absolute and fixed offsets against a
/// containing block. Quire stores that box in physical page coordinates using
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
    pub(in crate::layout) static_position: AbsoluteStaticPosition,
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

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DecodedPngImage {
    pub(in crate::layout) image_id: Option<ImageId>,
    /// Source samples used for image paint and PDF resource emission.
    pub(in crate::layout) pixel_size: RasterPixelSize,
    /// Optional source crop in the image's original pixel grid. The cached
    /// resource stays whole; PDF emission applies this crop at draw time.
    pub(in crate::layout) source_rect: Option<RenderedImageSourceRect>,
    /// Preferred natural dimensions used by CSS sizing algorithms.
    pub(in crate::layout) natural_size: crate::units::CssPixelSize,
    /// The depth shared by the RGB and optional alpha sample planes.
    pub(in crate::layout) sample_depth: crate::image_store::RasterSampleDepth,
    /// Byte-encoded raster samples, never CSS coordinates. Generated CSS
    /// images construct this only after their explicit output-space encoding.
    pub(in crate::layout) rgb: EncodedRasterRgbSamples,
    pub(in crate::layout) alpha: Option<Rc<[u8]>>,
    pub(in crate::layout) color_space: crate::color::RasterColorSpace,
}

/// Encoded RGB samples paired with `DecodedPngImage::color_space`.
///
/// The wrapper prevents image payloads from being confused with CSS component
/// triples, which may be wide-gamut, unbounded, or D50 PCS coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct EncodedRasterRgbSamples(Rc<[u8]>);

impl EncodedRasterRgbSamples {
    pub(in crate::layout) fn new(samples: Vec<u8>) -> Self {
        Self(Rc::from(samples.into_boxed_slice()))
    }

    pub(in crate::layout) fn from_shared(samples: Rc<[u8]>) -> Self {
        Self(samples)
    }

    pub(in crate::layout) fn shared(&self) -> Rc<[u8]> {
        Rc::clone(&self.0)
    }
}

impl AsRef<[u8]> for EncodedRasterRgbSamples {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::ops::Deref for EncodedRasterRgbSamples {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<Vec<u8>> for EncodedRasterRgbSamples {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_ref() == other.as_slice()
    }
}

impl DecodedPngImage {
    pub(in crate::layout) fn new(
        pixel_width: u32,
        pixel_height: u32,
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    ) -> Self {
        Self {
            image_id: None,
            pixel_size: RasterPixelSize::new(pixel_width, pixel_height),
            source_rect: None,
            natural_size: crate::units::CssPixelSize::new(pixel_width, pixel_height),
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            rgb: EncodedRasterRgbSamples::new(rgb),
            alpha: alpha.map(|alpha| Rc::from(alpha.into_boxed_slice())),
            color_space: crate::color::RasterColorSpace::SRGB,
        }
    }

    pub(in crate::layout) fn in_color_space(
        mut self,
        color_space: crate::css::CssColorSpace,
    ) -> Self {
        self.color_space = crate::color::RasterColorSpace::BuiltIn(color_space);
        self
    }

    pub(in crate::layout) fn natural_layout_size(&self) -> crate::units::LayoutSize {
        crate::units::css_pixel_natural_layout_size(self.natural_size)
    }

    pub(in crate::layout) fn with_source_rect(
        mut self,
        source_rect: RenderedImageSourceRect,
    ) -> Self {
        self.natural_size = crate::units::CssPixelSize::new(source_rect.width, source_rect.height);
        self.source_rect = Some(source_rect);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CounterSet {
    pub(in crate::layout) values: HashMap<String, Vec<CounterInstance>>,
    pub(in crate::layout) frames: Vec<CounterFrame>,
    pub(in crate::layout) next_scope_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct CounterInstance {
    pub(in crate::layout) value: CounterValue,
    pub(in crate::layout) reversed: bool,
    /// The element or tree-abiding pseudo-element that instantiated this
    /// counter, when the counter set originates from source-order planning.
    ///
    /// CSS Lists defines counter identity in terms of both its name and
    /// originating element. See <https://drafts.csswg.org/css-lists-3/#creating-counters>.
    pub(in crate::layout) creator: Option<CounterOriginKey>,
    /// The traversal scope that owns this counter instance for style
    /// containment. This is deliberately distinct from `creator`: traversal
    /// scopes are not CSS counter identity.
    pub(in crate::layout) creator_scope: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct CounterOriginKey {
    pub(in crate::layout) element_id: crate::dom::ElementId,
    pub(in crate::layout) source: box_tree::CounterEventSource,
}

impl CounterOriginKey {
    pub(in crate::layout) fn new(element: &Element, source: box_tree::CounterEventSource) -> Self {
        Self {
            element_id: element.id,
            source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct CounterResetKey {
    pub(in crate::layout) origin: CounterOriginKey,
    pub(in crate::layout) declaration_index: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::layout) struct CounterPlan {
    pub(in crate::layout) reversed_initial_values: HashMap<CounterResetKey, CounterValue>,
    pub(in crate::layout) values_at_origin: HashMap<CounterOriginKey, HashMap<String, Vec<i32>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct CounterFrame {
    pub(in crate::layout) base_lengths: HashMap<String, usize>,
    pub(in crate::layout) scope_id: usize,
    /// The innermost style-containment boundary visible to this scope.
    ///
    /// `counter-increment` and `counter-set` must not mutate instances
    /// created outside this scope. See CSS Containment 2 § 3.3:
    /// <https://drafts.csswg.org/css-contain-2/#containment-style>.
    pub(in crate::layout) counter_mutation_floor: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CounterScopeState {
    /// Generated pseudo-content is evaluated against the durable source-order
    /// counter snapshot and restores the caller's transient layout state when
    /// complete.
    pub(in crate::layout) previous_counter_set: Option<CounterSet>,
    /// Style containment also scopes generated-quote nesting.  The enclosing
    /// quote depth remains observable inside the scope, but quote operations
    /// performed there must not affect following siblings outside it.
    /// <https://drafts.csswg.org/css-contain-2/#containment-style>
    pub(in crate::layout) previous_quote_depth: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GeneratedPseudoCounterMode {
    Commit,
    Rollback,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PositionedPaintLayer {
    pub(in crate::layout) page_index: usize,
    /// The positioned-paint transaction that still owns this layer's scratch
    /// fragmentainers. A nonzero depth may not be flushed into a document
    /// page until the owning transaction restores its parent state.
    pub(in crate::layout) transaction_depth: usize,
    /// Source element for layers built by the ordinary positioned-box path.
    /// Re-entering that path replaces stale provisional layout for the same
    /// element and final page.
    pub(in crate::layout) source_element: Option<crate::dom::ElementId>,
    /// The originating box's computed style is retained with the final
    /// page-space stacking context so enclosing scroll containers can form
    /// snap areas only after positioned remapping is complete.
    pub(in crate::layout) source_style: ComputedStyle,
    /// Pointer identity of the originating generated-box style. The style is
    /// also retained above for paint effects; this compact identity is used
    /// only by the final-commit duplicate assertion.
    pub(in crate::layout) source_style_identity: usize,
    /// Distinguishes independently clipped continuations of one positioned
    /// principal in a multicolumn fragmentainer sequence.
    pub(in crate::layout) multicol_fragment_index: Option<usize>,
    pub(in crate::layout) source_is_target: bool,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context: PaintStackingContext,
    pub(in crate::layout) links: Vec<RenderedLink>,
    pub(in crate::layout) escaped_atom_translation: EscapedAtomTranslation,
}

/// Runtime identity for one page-local positioned principal. Rust ownership
/// proves a captured paint record is consumed once; speculative layout can
/// still independently construct the same logical record, so final commit
/// asserts this key is unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct PositionedPaintCommitKey {
    source_element: crate::dom::ElementId,
    source_style_identity: usize,
    multicol_fragment_index: Option<usize>,
}

impl PositionedPaintLayer {
    pub(in crate::layout) fn commit_key(&self) -> Option<PositionedPaintCommitKey> {
        self.source_element
            .map(|source_element| PositionedPaintCommitKey {
                source_element,
                source_style_identity: self.source_style_identity,
                multicol_fragment_index: self.multicol_fragment_index,
            })
    }

    pub(in crate::layout) fn with_multicol_fragment_index(mut self, index: usize) -> Self {
        self.multicol_fragment_index = Some(index);
        self
    }

    pub(in crate::layout) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.context = self.context.translated(offset);
        self.links = self
            .links
            .into_iter()
            .map(|link| link.translated(offset))
            .collect();
        self
    }
}

/// How an escaped positioned layer from an atomic inline should be translated.
///
/// CSS 2.2 lays out `inline-block` contents in a separate formatting context,
/// but positioned descendants whose containing block is outside that
/// inline-block still escape to the ancestor stacking context. Auto insets use
/// the inline-block-local static-position rectangle, while explicit insets are
/// already resolved in page coordinates and must not be translated by the
/// atom's final line position:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks> and
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct EscapedAtomTranslation {
    pub(in crate::layout) translate_x_with_atom: bool,
    pub(in crate::layout) translate_y_with_atom: bool,
    pub(in crate::layout) normalize_x: f32,
}

impl EscapedAtomTranslation {
    pub(in crate::layout) fn none() -> Self {
        Self::default()
    }

    /// Translate a deferred normal-flow fragment out of an atomic inline's
    /// temporary formatting context and into the final inline line box.
    ///
    /// Relative positioning does not change a box's layout-space origin, but
    /// its retained paint still uses that origin. Both physical axes must
    /// therefore follow the atom when the fragment escapes the temporary
    /// inline-block page.
    /// <https://www.w3.org/TR/css-position-3/#relative-positioning>
    pub(in crate::layout) fn normal_flow_fragment() -> Self {
        Self {
            translate_x_with_atom: true,
            translate_y_with_atom: true,
            normalize_x: 0.0,
        }
    }

    pub(in crate::layout) fn from_positioned_static_axes(
        containing_block: ContainingBlock,
        uses_static_x: bool,
        uses_static_y: bool,
        normalize_static_x: bool,
    ) -> Self {
        Self {
            translate_x_with_atom: uses_static_x,
            translate_y_with_atom: uses_static_y,
            normalize_x: if uses_static_x && normalize_static_x {
                -containing_block.x()
            } else {
                0.0
            },
        }
    }

    pub(in crate::layout) fn escape_offset(self, atom_local_y_offset: f32) -> PaintTranslation {
        PaintTranslation::new(
            self.normalize_x,
            if self.translate_y_with_atom {
                atom_local_y_offset
            } else {
                0.0
            },
        )
    }

    pub(in crate::layout) fn atom_offset(self, atom_x: f32, atom_y: f32) -> PaintTranslation {
        PaintTranslation::new(
            if self.translate_x_with_atom {
                atom_x
            } else {
                0.0
            },
            if self.translate_y_with_atom {
                atom_y
            } else {
                0.0
            },
        )
    }

    /// Select the page that owns a replayed escaped layer.
    ///
    /// A static block-axis position follows the atom to its selected line and
    /// consequently takes that line's fragmentainer. An explicit block-axis
    /// inset is already in outer page coordinates and retains the page chosen
    /// by positioned layout:
    /// <https://www.w3.org/TR/css-position-3/#static-position>.
    pub(in crate::layout) fn replay_page_index(
        self,
        atom_page_index: usize,
        positioned_page_index: usize,
    ) -> usize {
        if self.translate_y_with_atom {
            atom_page_index
        } else {
            positioned_page_index
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FixedPaintLayer {
    /// The originating DOM element identifies the fixed formatting tree.
    pub(in crate::layout) source_element: crate::dom::ElementId,
    /// The originating computed style distinguishes separately generated
    /// boxes such as `::before` and `::after` on one DOM element while still
    /// allowing a speculative replay of the same fixed box to be replaced.
    pub(in crate::layout) source_style: ComputedStyle,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context: PaintStackingContext,
    pub(in crate::layout) links: Vec<RenderedLink>,
}

/// Internal CSS stacking-context decision for one laid-out box fragment.
///
/// CSS Positioned Layout and CSS 2.2 Appendix E decide paint placement from
/// stack level, while CSS Transforms, CSS CssColor opacity, and CSS Overflow add
/// group effects. Keeping this classification in one value prevents layout
/// paths from independently deciding which positioned descendants are captured:
/// <https://www.w3.org/TR/css-position-3/#painting-order>,
/// <https://www.w3.org/TR/CSS22/zindex.html>,
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct StackingContextPolicy {
    pub(in crate::layout) parent_band: PaintBand,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context_kind: StackingContextKind,
    pub(in crate::layout) child_layer_policy: ChildLayerPolicy,
    pub(in crate::layout) is_real_stacking_context: bool,
    pub(in crate::layout) is_fake_context: bool,
    pub(in crate::layout) creates_compositing_group: bool,
    pub(in crate::layout) establishes_containing_block: bool,
    pub(in crate::layout) captures_positioned_descendants: bool,
    pub(in crate::layout) effects: PaintEffects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum StackingContextKind {
    None,
    Real,
    FakeAtomic,
}

/// The paint-order context for flex or grid item replay.
///
/// CSS Flexbox and Grid give same-page unfragmented static items
/// inline-block-like atomic painting. Fragmented and column-nested replay
/// still uses the in-flow band while the fragmented/nested Appendix E ordering
/// model is incomplete.
/// <https://drafts.csswg.org/css-flexbox/#painting>
/// <https://www.w3.org/TR/css-grid-1/#z-order>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::layout) enum FlexGridItemPaintContext {
    SamePageUnfragmented,
    FragmentedOrNested,
}

impl FlexGridItemPaintContext {
    fn static_parent_band(self) -> PaintBand {
        match self {
            Self::SamePageUnfragmented => PaintBand::Inline,
            Self::FragmentedOrNested => PaintBand::InFlowBlock,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum ChildLayerPolicy {
    CaptureAll,
    CaptureAutoLevel,
    EscapeAll,
}

impl StackingContextPolicy {
    pub(in crate::layout) fn for_positioned(
        element: &Element,
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        let effects = assets::paint_effects_for_element_box(element, style, bounds);
        let is_real_stacking_context =
            matches!(&style.position, Position::Fixed | Position::Sticky)
                || style.z_index.establishes_stacking_context()
                || style_creates_effect_stacking_context_with_containment(
                    style,
                    &effects,
                    property_containment_applies_to_element(element, style),
                );
        let is_fake_context = !is_real_stacking_context
            && matches!(&style.position, Position::Relative | Position::Absolute);
        Self {
            parent_band: StackLevel::from_optional_z_index(style.z_index.stack_level())
                .paint_band(),
            stack_level: StackLevel::from_optional_z_index(style.z_index.stack_level()),
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else if is_fake_context {
                StackingContextKind::FakeAtomic
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: matches!(
                &style.position,
                Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
            ) || style.has_transform_or_preserve_3d(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_non_positioned_effect(
        element: &Element,
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        Self::for_non_positioned_effect_with_geometry(
            element,
            style,
            assets::PrincipalPaintGeometry::css_layout(bounds),
        )
    }

    pub(in crate::layout) fn for_non_positioned_effect_with_geometry(
        element: &Element,
        style: &ComputedStyle,
        geometry: assets::PrincipalPaintGeometry,
    ) -> Self {
        let effects = assets::paint_effects_for_principal_box(style, geometry);
        Self::for_non_positioned_effect_with_effects(
            style,
            effects,
            property_containment_applies_to_element(element, style),
        )
    }

    pub(in crate::layout) fn for_non_positioned_style_effect(
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
        // This constructor is used for retained inline and anonymous-box
        // fragments, which have no `Element` to query.  Containment is only
        // effective when that fragment has a containment-capable principal
        // box; treating every style as applicable recreates a stacking
        // context for non-atomic inlines and ruby internals.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        Self::for_non_positioned_effect_with_effects(
            style,
            effects,
            property_containment_applies_to_style(style),
        )
    }

    pub(in crate::layout) fn for_non_positioned_effect_with_effects(
        style: &ComputedStyle,
        effects: PaintEffects,
        containment_applies: bool,
    ) -> Self {
        let in_flow_positioned = matches!(&style.position, Position::Relative | Position::Sticky);
        let stack_level = if in_flow_positioned {
            StackLevel::from_optional_z_index(style.z_index.stack_level())
        } else {
            StackLevel::Auto
        };
        let is_real_stacking_context = matches!(&style.position, Position::Sticky)
            || (style.position == Position::Relative
                && style.z_index.establishes_stacking_context())
            || style_creates_effect_stacking_context_with_containment(
                style,
                &effects,
                containment_applies,
            );
        let is_fake_context = style.position == Position::Relative && !is_real_stacking_context;
        Self {
            parent_band: if in_flow_positioned {
                stack_level.paint_band()
            } else {
                PaintBand::InFlowBlock
            },
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else if is_fake_context {
                StackingContextKind::FakeAtomic
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else if is_fake_context {
                ChildLayerPolicy::CaptureAutoLevel
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: style.has_transform_or_preserve_3d(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_atomic(
        style: &ComputedStyle,
        parent_band: PaintBand,
        bounds: PaintClip,
    ) -> Self {
        Self::for_atomic_with_geometry(
            style,
            parent_band,
            assets::PrincipalPaintGeometry::css_layout(bounds),
        )
    }

    pub(in crate::layout) fn for_atomic_with_geometry(
        style: &ComputedStyle,
        parent_band: PaintBand,
        geometry: assets::PrincipalPaintGeometry,
    ) -> Self {
        let effects = assets::paint_effects_for_principal_box(style, geometry);
        // Atomic inline/replaced boxes still participate in positioned
        // stacking. In particular, a relatively positioned inline image with
        // a negative z-index belongs below later in-flow inline content, even
        // though its contents are otherwise painted as one atomic unit.
        // <https://www.w3.org/TR/CSS22/zindex.html#painting-order>
        let in_flow_positioned = matches!(&style.position, Position::Relative | Position::Sticky);
        let stack_level = if in_flow_positioned {
            StackLevel::from_optional_z_index(style.z_index.stack_level())
        } else {
            StackLevel::Auto
        };
        let is_real_stacking_context = matches!(&style.position, Position::Sticky)
            || (style.position == Position::Relative
                && style.z_index.establishes_stacking_context())
            || style_creates_effect_stacking_context(style, &effects);
        Self {
            parent_band: if is_real_stacking_context || in_flow_positioned {
                stack_level.paint_band()
            } else {
                parent_band
            },
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else {
                StackingContextKind::FakeAtomic
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context: true,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: in_flow_positioned
                || style.has_transform_or_preserve_3d(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    /// Build the stacking policy for an embedded SVG root.
    ///
    /// An outermost SVG element is an atomic stacking context whose own
    /// background and border paint before its rendered descendants. SVG also
    /// requires the root element to establish an isolated compositing group,
    /// independently of CSS `opacity`, blending, or filters. Keeping this as
    /// a distinct policy prevents the generic replaced-element path from
    /// treating the group as an optional PDF optimization.
    /// <https://www.w3.org/TR/SVG2/render.html#EstablishingANewStackingContext>
    /// <https://www.w3.org/TR/SVG2/render.html#ParentCompositing>
    pub(in crate::layout) fn for_inline_svg_root(
        style: &ComputedStyle,
        parent_band: PaintBand,
        bounds: PaintClip,
    ) -> Self {
        Self::for_inline_svg_root_with_geometry(
            style,
            parent_band,
            assets::PrincipalPaintGeometry::css_layout(bounds),
        )
    }

    pub(in crate::layout) fn for_inline_svg_root_with_geometry(
        style: &ComputedStyle,
        parent_band: PaintBand,
        geometry: assets::PrincipalPaintGeometry,
    ) -> Self {
        let mut policy = Self::for_atomic_with_geometry(style, parent_band, geometry);
        policy.context_kind = StackingContextKind::Real;
        policy.child_layer_policy = ChildLayerPolicy::CaptureAll;
        policy.is_real_stacking_context = true;
        policy.is_fake_context = false;
        policy.creates_compositing_group = true;
        policy.captures_positioned_descendants = true;
        policy.effects.isolation = true;
        policy
    }

    pub(in crate::layout) fn for_flex_item(style: &ComputedStyle, bounds: PaintClip) -> Self {
        Self::for_flex_or_grid_item(
            style,
            bounds,
            FlexGridItemPaintContext::SamePageUnfragmented,
        )
    }

    /// Return the stacking policy for a fragment of a split flex item.
    pub(in crate::layout) fn for_fragmented_flex_item(
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        Self::for_flex_or_grid_item(style, bounds, FlexGridItemPaintContext::FragmentedOrNested)
    }

    fn for_flex_or_grid_item(
        style: &ComputedStyle,
        bounds: PaintClip,
        paint_context: FlexGridItemPaintContext,
    ) -> Self {
        let stack_level = StackLevel::from_optional_z_index(style.z_index.stack_level());
        // The grid/flex item's independent formatting context owns the used
        // padding-box clip, and its replayed paint is captured as one atomic
        // item context. Keep that clip on the item policy so descendants
        // whose geometry was intentionally retained for positioning and
        // compositing cannot escape when the context is serialized.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        let effects = assets::paint_effects_for_box(style, bounds);
        // The replay checkpoint contains the independently formatted item
        // contents and captured descendants; its principal decoration is
        // emitted by the owning flex/grid formatter outside this context.
        // Keep the shared used overflow effect here so static and fragmented
        // items use the same descendant-only clipping path.
        let in_flow_positioned = matches!(&style.position, Position::Relative | Position::Sticky);
        let is_real_stacking_context = matches!(&style.position, Position::Sticky)
            || style.z_index.establishes_stacking_context()
            || style_creates_effect_stacking_context(style, &effects);
        let is_fake_context = style.position == Position::Relative && !is_real_stacking_context;
        Self {
            // Complete flex/grid items paint as inline-block-like atomic
            // units in order-modified document order. A static `z-index:
            // auto` item therefore belongs to the parent's inline phase,
            // after its in-flow block backgrounds and borders. Fragmented
            // replay remains in the in-flow phase until the fragmented
            // Appendix E model can express that atomic ordering. A relatively
            // positioned item with `z-index: auto` always paints atomically
            // in the parent's auto/zero positioned phase without becoming a
            // real stacking context. Its positioned descendants still belong
            // to that parent context.
            // <https://drafts.csswg.org/css-flexbox/#painting>
            // <https://www.w3.org/TR/css-position-3/#painting-order>
            // <https://www.w3.org/TR/CSS22/zindex.html#painting-order>
            parent_band: if is_real_stacking_context || in_flow_positioned {
                stack_level.paint_band()
            } else {
                paint_context.static_parent_band()
            },
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else if is_fake_context {
                StackingContextKind::FakeAtomic
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: in_flow_positioned
                || style.has_transform_or_preserve_3d(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    /// Return the stacking policy for a grid item fragment.
    ///
    /// CSS Grid items, like flex items, paint as stacking-context-capable
    /// formatting-context items even when `position` is `static`; a non-auto
    /// `z-index` therefore creates a stacking context:
    /// <https://www.w3.org/TR/css-grid-1/#z-order> and
    /// <https://www.w3.org/TR/css-position-3/#painting-order>.
    pub(in crate::layout) fn for_grid_item(style: &ComputedStyle, bounds: PaintClip) -> Self {
        Self::for_flex_item(style, bounds)
    }

    /// Return the stacking policy for a fragment of a split grid item.
    pub(in crate::layout) fn for_fragmented_grid_item(
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        Self::for_fragmented_flex_item(style, bounds)
    }

    pub(in crate::layout) fn style_needs_non_positioned_scope(
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        matches!(&style.position, Position::Relative | Position::Sticky)
            || style_creates_effect_stacking_context_with_containment(
                style,
                &assets::paint_effects_for_element_box(
                    element,
                    style,
                    PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
                ),
                property_containment_applies_to_element(element, style),
            )
            || used_overflow_clips_element(element, style)
    }
}

pub(in crate::layout) fn style_creates_effect_stacking_context(
    style: &ComputedStyle,
    effects: &PaintEffects,
) -> bool {
    style_creates_effect_stacking_context_with_containment(
        style,
        effects,
        property_containment_applies_to_style(style),
    )
}

fn style_creates_effect_stacking_context_with_containment(
    style: &ComputedStyle,
    effects: &PaintEffects,
    containment_applies: bool,
) -> bool {
    effects.opacity < 1.0
        || effects.transform.is_some()
        // `perspective` applies to descendants, but it nevertheless
        // establishes a stacking context and therefore needs an owning paint
        // subtree even when the element has no transform of its own.
        // <https://drafts.csswg.org/css-transforms-2/#perspective-property>
        || effects.descendant_projective_3d_transform.is_some()
        || effects.three_d_participation
            != crate::document::paint::effects::ThreeDParticipation::Flat
        || effects.clip_path.is_active()
        || effects.mask.is_active()
        || effects.filter.is_active()
        || effects.blend_mode != PaintBlendMode::Normal
        || effects.isolation
        || style.isolation == Isolation::Isolate
        || style.mix_blend_mode != MixBlendMode::Normal
        || !matches!(style.filter, FilterValue::None)
        || style.clip_path != ClipPath::None
        || !matches!(style.mask, MaskValue::None)
        // Layout containment establishes an independent formatting context,
        // containing blocks, and a stacking context. Paint containment adds
        // the corresponding paint isolation and clipping.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        || (containment_applies && (style.contain.layout || style.contain.paint))
        || matches!(
            style.content_visibility,
            ContentVisibility::Auto | ContentVisibility::Hidden
        )
        || style.will_change.opacity
        || style.will_change.transform
        || style.will_change.filter
        || style.will_change.clip_path
        || style.will_change.mask
        || style.will_change.mix_blend_mode
        || style.will_change.isolation
        || style.will_change.contain
}

pub(in crate::layout) type NamedStringAssignment = PageAssignment<PageAssignmentValue>;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CapturedPageAssignment {
    pub(in crate::layout) name: String,
    pub(in crate::layout) value: PageAssignmentValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct AssignmentId(pub(in crate::layout) usize);

/// Page-local captured value for named strings and running elements.
///
/// CSS GCPM resolves `string()` and `element()` against assignments made by
/// source elements during pagination. Keeping placement with the value lets
/// page-margin resolution distinguish `first`/`last` from exact page-start
/// lookups:
/// <https://www.w3.org/TR/css-gcpm-3/#named-strings> and
/// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PageAssignment<T> {
    pub(in crate::layout) id: AssignmentId,
    pub(in crate::layout) value: T,
    pub(in crate::layout) placement: AssignmentPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct AssignmentPlacement {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) starts_page_fragment: bool,
    pub(in crate::layout) border_box: Option<PaintClip>,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FragmentPageValue {
    pub(in crate::layout) page_name: Option<String>,
    pub(in crate::layout) specified: bool,
}

impl FragmentPageValue {
    pub(in crate::layout) fn unspecified() -> Self {
        Self {
            page_name: None,
            specified: false,
        }
    }
}

/// Final page-local metadata for a visible layout fragment.
///
/// CSS Fragmentation defines fragments as the durable pieces of a source box,
/// while CSS Paged Media and GCPM resolve named pages, named strings, and
/// running elements from the page fragment that actually contains the source:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>,
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>, and
/// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FragmentPageMetadata {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) source_border_box: Option<PaintClip>,
    pub(in crate::layout) starts_page_fragment: bool,
    pub(in crate::layout) continues_from_previous_page: bool,
    pub(in crate::layout) continues_to_next_page: bool,
    pub(in crate::layout) first_page_value: FragmentPageValue,
    pub(in crate::layout) last_page_value: FragmentPageValue,
    pub(in crate::layout) assignment_ids: Vec<AssignmentId>,
}

impl FragmentPageMetadata {
    pub(in crate::layout) fn new(
        page_index: usize,
        source_border_box: Option<PaintClip>,
        starts_page_fragment: bool,
    ) -> Self {
        Self {
            page_index,
            source_border_box,
            starts_page_fragment,
            continues_from_previous_page: false,
            continues_to_next_page: false,
            first_page_value: FragmentPageValue::unspecified(),
            last_page_value: FragmentPageValue::unspecified(),
            assignment_ids: Vec::new(),
        }
    }

    pub(in crate::layout) fn empty(page_index: usize) -> Self {
        Self::new(page_index, None, false)
    }

    pub(in crate::layout) fn assignment_placement(&self) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: self.page_index,
            starts_page_fragment: self.starts_page_fragment,
            border_box: self.source_border_box,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) enum PageAssignmentValue {
    GeneratedContent(Vec<page_generated::PageMarginContentItem>),
    RunningElement(Box<RunningElementCapture>),
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct RunningElementCapture {
    pub(in crate::layout) fallback_text: String,
    pub(in crate::layout) content_parts: Vec<GeneratedContentPart>,
    pub(in crate::layout) element: Element,
    pub(in crate::layout) style: Box<ComputedStyle>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) quote_depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct ListMarker {
    /// Stable source identity used while an outside marker waits for its
    /// principal-line anchor. Recomputed marker styles are not a reliable
    /// identity because inline collection may normalize their style state.
    pub(in crate::layout) source_element: Option<ElementId>,
    pub(in crate::layout) text: String,
    pub(in crate::layout) image: Option<MarkerImage>,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) position: ListStylePosition,
    pub(in crate::layout) positioning_direction: Direction,
    pub(in crate::layout) suffix_space: bool,
}

/// The physical line geometry used to place an outside list marker.
///
/// CSS Lists intentionally leaves the exact outside-marker position
/// undefined.  Quire follows interoperable line-box behavior: textual
/// markers align their alphabetic baseline with the first in-flow formatted
/// line, while image markers use that line's block-start edge.
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct OutsideMarkerAnchor {
    /// The final physical inline span of the principal line. Float-adjacent
    /// fallbacks resolve their containing span before constructing this
    /// paint-only geometry.
    pub(in crate::layout) principal_line_inline_span: PageInlineSpan,
    pub(in crate::layout) formatted_line_block_start: PageTopBlockPosition,
    pub(in crate::layout) alphabetic_baseline: PageTopBlockPosition,
}

/// Unresolved fallback geometry for an outside marker with no principal line.
///
/// CSS Lists leaves the float-adjacent placement of such markers undefined.
/// Keeping the containing span distinct from [`OutsideMarkerAnchor`] makes
/// the compatibility float-band resolution explicit at the paint boundary.
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct OutsideMarkerFallbackCandidate {
    pub(in crate::layout) containing_inline_span: PageInlineSpan,
    pub(in crate::layout) fallback_line_block_span: PageBlockSpan,
    pub(in crate::layout) alphabetic_baseline: PageTopBlockPosition,
}

/// A resolved outside-marker paint operation retained until its list-item
/// owner can commit it outside a descendant paint scope.
///
/// The marker is the list item's first generated child, so its paint belongs
/// to the list item rather than to whichever nested block happened to expose
/// the first eligible line. Retaining a page-local fragment makes that
/// ownership explicit across descendant stacking contexts and fragmentation.
/// <https://drafts.csswg.org/css-lists-3/#markers>
/// <https://www.w3.org/TR/CSS22/zindex.html>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct ResolvedOutsideMarkerPaint {
    /// The generated first child whose paint belongs to the list item.
    pub(in crate::layout) marker: ListMarker,
    /// The originating list-item style used by marker painting.
    pub(in crate::layout) list_item_style: ComputedStyle,
    /// The accepted principal-line geometry that resolved this operation.
    pub(in crate::layout) anchor: OutsideMarkerAnchor,
    /// The page fragment containing the resolved line.
    pub(in crate::layout) page: DocumentPageIndex,
    /// Isolated page-local marker paint, detached from the descendant scope.
    pub(in crate::layout) fragment: PaintFragment,
}

/// Where a resolved outside marker must be painted relative to the line that
/// establishes its geometry.
///
/// A normal-flow relatively positioned block is painted as an auto/zero
/// positioned atom. Its nested principal line can resolve an ancestor list
/// item's marker geometry, but cannot own that marker's paint.
/// <https://www.w3.org/TR/CSS22/zindex.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum OutsideMarkerPaintOwner {
    CurrentPaintScope,
    ListItem,
}

impl OutsideMarkerPaintOwner {
    pub(in crate::layout) fn for_principal_line_block(style: &ComputedStyle) -> Self {
        if matches!(style.position, Position::Relative | Position::Sticky) {
            Self::ListItem
        } else {
            Self::CurrentPaintScope
        }
    }
}

/// Progress of an outside marker whose first principal line is discovered in
/// descendant layout.
///
/// `Capturing` prevents the marker's own generated inline sequence from
/// recursively treating itself as the list item's principal line.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) enum DeferredOutsideMarkerPaintState {
    AwaitingAnchor,
    Capturing,
    Resolved(Box<ResolvedOutsideMarkerPaint>),
    PaintedInPlace,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PendingOutsideMarkerAnchor {
    pub(in crate::layout) marker: ListMarker,
    pub(in crate::layout) list_item_style: ComputedStyle,
    /// Principal-line geometry retained for an item that never produces an
    /// eligible in-flow line. Fragmented descendants may advance the layout
    /// cursor before the pending marker is finalized, but they must not move
    /// this CSS Lists fallback anchor.
    pub(in crate::layout) fallback: OutsideMarkerFallbackCandidate,
    pub(in crate::layout) paint: DeferredOutsideMarkerPaintState,
}

/// Outside-marker anchors that belong to the current principal line-layout
/// coordinate space.
///
/// Off-page measurements and atomic-inline paint captures do lay out lines,
/// but none of those lines is eligible to anchor an ancestor list item's
/// marker. Keeping the collection behind this semantic type makes that
/// boundary explicit instead of allowing a scratch layout to consume an
/// ambient `Vec` by accident.
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct PendingOutsideMarkerAnchors {
    anchors: Vec<PendingOutsideMarkerAnchor>,
}

/// Principal-flow marker anchors suspended while a nested layout owns only
/// scratch-local line geometry.
///
/// This token is deliberately not `Clone`: there is one authoritative set of
/// anchors, and it must be restored exactly once to its owning principal flow.
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
#[must_use = "suspended outside-marker anchors must be restored after scratch layout"]
#[derive(Debug)]
pub(in crate::layout) struct SuspendedOutsideMarkerAnchors(PendingOutsideMarkerAnchors);

impl PendingOutsideMarkerAnchors {
    pub(in crate::layout) fn push(&mut self, anchor: PendingOutsideMarkerAnchor) {
        self.anchors.push(anchor);
    }

    pub(in crate::layout) fn pop(&mut self) -> Option<PendingOutsideMarkerAnchor> {
        self.anchors.pop()
    }

    pub(in crate::layout) fn iter(&self) -> impl Iterator<Item = &PendingOutsideMarkerAnchor> {
        self.anchors.iter()
    }

    pub(in crate::layout) fn begin_paint_capture(&mut self, index: usize) {
        debug_assert!(
            matches!(
                self.anchors[index].paint,
                DeferredOutsideMarkerPaintState::AwaitingAnchor
            ),
            "only an unresolved marker anchor can begin paint capture"
        );
        self.anchors[index].paint = DeferredOutsideMarkerPaintState::Capturing;
    }

    pub(in crate::layout) fn finish_paint_capture(
        &mut self,
        index: usize,
        paint: ResolvedOutsideMarkerPaint,
    ) {
        debug_assert!(
            matches!(
                self.anchors[index].paint,
                DeferredOutsideMarkerPaintState::Capturing
            ),
            "only a marker being captured can receive its paint fragment"
        );
        self.anchors[index].paint = DeferredOutsideMarkerPaintState::Resolved(Box::new(paint));
    }

    pub(in crate::layout) fn mark_painted_in_place(&mut self, index: usize) {
        debug_assert!(
            matches!(
                self.anchors[index].paint,
                DeferredOutsideMarkerPaintState::AwaitingAnchor
            ),
            "only an unresolved marker anchor can paint in its current owner"
        );
        self.anchors[index].paint = DeferredOutsideMarkerPaintState::PaintedInPlace;
    }

    pub(in crate::layout) fn suspend(&mut self) -> SuspendedOutsideMarkerAnchors {
        SuspendedOutsideMarkerAnchors(std::mem::take(self))
    }

    pub(in crate::layout) fn restore(&mut self, suspended: SuspendedOutsideMarkerAnchors) {
        debug_assert!(
            self.anchors.is_empty(),
            "a scratch layout must finalize its local outside-marker anchors"
        );
        *self = suspended.0;
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }
}

/// Whether a nested inline paint replay may query active float exclusions.
///
/// Outside marker geometry is resolved before it enters nested text painting;
/// reapplying the enclosing float band would treat the marker as a second
/// principal line and move it away from that resolved position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum NestedInlinePaintFloatPolicy {
    ReapplyActiveFloatBands,
    PreserveResolvedGeometry,
}

impl ListMarker {
    /// Whether this marker has content that can establish an inside line box.
    ///
    /// A marker made only of collapsible white space disappears during CSS
    /// Text processing, so it must not manufacture a principal line for an
    /// otherwise empty list item. A generated image remains an atomic inline
    /// participant even when its textual representation is empty.
    /// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
    /// <https://drafts.csswg.org/css-inline-3/#line-box>
    pub(in crate::layout) fn has_in_flow_content(&self) -> bool {
        self.image.is_some() || !crate::text::trim_css_collapsible_whitespace(&self.text).is_empty()
    }

    pub(in crate::layout) fn participates_in_first_line(&self) -> bool {
        self.position == ListStylePosition::Inside
    }

    pub(in crate::layout) fn paints_outside(&self) -> bool {
        self.position == ListStylePosition::Outside && !self.participates_in_first_line()
    }

    pub(in crate::layout) fn follows_content_in_first_line(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct MarkerImage {
    pub(in crate::layout) decoded: DecodedPngImage,
    pub(in crate::layout) svg: Option<SharedSvgAsset>,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

impl PartialEq for MarkerImage {
    fn eq(&self, other: &Self) -> bool {
        self.decoded == other.decoded
            && self.width == other.width
            && self.height == other.height
            && self.svg.as_ref().map(Rc::as_ptr) == other.svg.as_ref().map(Rc::as_ptr)
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineWord {
    pub(in crate::layout) text: String,
    pub(in crate::layout) style: InlineStyle,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    pub(in crate::layout) ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
}

pub(in crate::layout) type InlineStyle = Rc<ComputedStyle>;

/// CSS Text wrapping behavior owned by one lexical inline scope.
///
/// A soft-wrap opportunity *between* two typographic units belongs to their
/// nearest common inline ancestor, rather than to either descendant that owns
/// the text or atomic box.  Keep that small, boundary-specific fact separate
/// from paint and shaping styles so graph construction can retain it through
/// transparent inline edges and source slicing:
/// <https://drafts.csswg.org/css-text-3/#line-break-details>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineBoundaryPolicy {
    allows_soft_wrap: bool,
}

impl InlineBoundaryPolicy {
    fn from_style(style: &ComputedStyle) -> Self {
        Self {
            allows_soft_wrap: style.allows_soft_wrap(),
        }
    }

    pub(in crate::layout) fn allows_soft_wrap(self) -> bool {
        self.allows_soft_wrap
    }
}

/// Lexical inline ancestry retained for CSS Text boundary-owned behavior.
///
/// A regular inline box whose aligned subtree is positioned against the final
/// line box rather than the parent baseline.
///
/// CSS 2.2 defines `vertical-align: top` and `bottom` in terms of an inline
/// element's aligned subtree.  This is deliberately distinct from the
/// fragment's own computed style because `vertical-align` is not inherited:
/// descendant text must still retain the enclosing inline box's placement.
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineScopeLineRelativeAlignment {
    Top,
    Bottom,
}

impl InlineScopeLineRelativeAlignment {
    pub(in crate::layout) fn from_baseline_shift(baseline_shift: &BaselineShift) -> Option<Self> {
        match baseline_shift {
            BaselineShift::Top => Some(Self::Top),
            BaselineShift::Bottom => Some(Self::Bottom),
            BaselineShift::LengthPercentage(_)
            | BaselineShift::Super
            | BaselineShift::Sub
            | BaselineShift::Center => None,
        }
    }
}

#[derive(Debug)]
/// Retains the local used tracking value and the ancestry needed for
/// cross-element wrapping and line-relative alignment. This immutable parent
/// chain survives collection, source slicing, and bidi reordering without
/// making layout consumers reconstruct scope from decoration atoms:
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>.
pub(in crate::layout) struct InlineTrackingScope {
    parent: Option<Rc<Self>>,
    depth: usize,
    /// The computed style of this lexical inline scope. CSS Text owns
    /// autospace at the innermost scope shared by the two adjacent
    /// typographic units, rather than at either unit's leaf style.
    autospace_style: InlineStyle,
    letter_spacing: LayoutLength,
    boundary_policy: InlineBoundaryPolicy,
    line_relative_alignment: Option<InlineScopeLineRelativeAlignment>,
    line_relative_style: Option<Rc<ComputedStyle>>,
}

impl InlineTrackingScope {
    pub(in crate::layout) fn root(style: &ComputedStyle) -> Rc<Self> {
        Rc::new(Self {
            parent: None,
            depth: 0,
            autospace_style: inline_style(style),
            letter_spacing: style.used_letter_spacing(),
            boundary_policy: InlineBoundaryPolicy::from_style(style),
            line_relative_alignment: None,
            line_relative_style: None,
        })
    }

    pub(in crate::layout) fn child(parent: Rc<Self>, style: &ComputedStyle) -> Rc<Self> {
        let line_relative_alignment = InlineScopeLineRelativeAlignment::from_baseline_shift(
            &style.vertical_align.baseline_shift,
        );
        Rc::new(Self {
            depth: parent.depth + 1,
            parent: Some(parent),
            autospace_style: inline_style(style),
            letter_spacing: style.used_letter_spacing(),
            boundary_policy: InlineBoundaryPolicy::from_style(style),
            line_relative_alignment,
            line_relative_style: line_relative_alignment
                .is_some()
                .then(|| Rc::new(style.clone())),
        })
    }

    pub(in crate::layout) fn letter_spacing(&self) -> LayoutLength {
        self.letter_spacing
    }

    pub(in crate::layout) fn boundary_policy(&self) -> InlineBoundaryPolicy {
        self.boundary_policy
    }

    pub(in crate::layout) fn autospace_style(&self) -> &ComputedStyle {
        &self.autospace_style
    }

    /// Return the innermost enclosing inline box whose aligned subtree is
    /// positioned relative to the final line box.
    ///
    /// A nested `top`/`bottom` inline establishes a separate subtree, so
    /// selecting the nearest scope also excludes it from an outer scope's
    /// placement as required by CSS 2.2.
    pub(in crate::layout) fn nearest_line_relative_scope(&self) -> Option<&InlineTrackingScope> {
        let mut scope = Some(self);
        while let Some(current) = scope {
            if current.line_relative_alignment.is_some() {
                return Some(current);
            }
            scope = current.parent.as_deref();
        }
        None
    }

    pub(in crate::layout) fn line_relative_alignment(
        &self,
    ) -> Option<InlineScopeLineRelativeAlignment> {
        self.line_relative_alignment
    }

    pub(in crate::layout) fn line_relative_style(&self) -> &ComputedStyle {
        debug_assert!(self.line_relative_alignment.is_some());
        self.line_relative_style
            .as_deref()
            .expect("a line-relative inline scope retains its own style")
    }

    /// Return the nearest shared lexical ancestor without allocating a
    /// temporary ancestor list.
    ///
    /// All scopes for one inline formatting context share a root.  The depth
    /// recorded at construction lets this walk align both immutable parent
    /// chains, then advance them together until their allocation identities
    /// match.
    fn lowest_common<'left>(left: &'left Self, right: &Self) -> &'left Self {
        let mut left = left;
        let mut right = right;
        while left.depth > right.depth {
            left = left
                .parent
                .as_deref()
                .expect("a deeper inline tracking scope has a parent");
        }
        while right.depth > left.depth {
            right = right
                .parent
                .as_deref()
                .expect("a deeper inline tracking scope has a parent");
        }
        while !std::ptr::eq(left, right) {
            left = left
                .parent
                .as_deref()
                .expect("all inline tracking participants share a paragraph root");
            right = right
                .parent
                .as_deref()
                .expect("all inline tracking participants share a paragraph root");
        }
        left
    }

    /// Resolve the shared inline-box policy used by CSS Text line breaking.
    ///
    /// Letter spacing deliberately has no corresponding common-ancestor API:
    /// each adjacent typographic unit contributes half of its own used value.
    pub(in crate::layout) fn common_boundary_policy(
        left: &Self,
        right: &Self,
    ) -> InlineBoundaryPolicy {
        Self::lowest_common(left, right).boundary_policy()
    }

    /// Return the innermost inline scope that owns an autospace boundary.
    ///
    /// CSS Text places inter-script spacing within the innermost element that
    /// contains both directly adjoining typographic units:
    /// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
    pub(in crate::layout) fn common_autospace_style<'left>(
        left: &'left Self,
        right: &Self,
    ) -> &'left ComputedStyle {
        Self::lowest_common(left, right).autospace_style()
    }
}

pub(in crate::layout) fn inline_style(style: &ComputedStyle) -> InlineStyle {
    Rc::new(style.clone())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineVisualOffset {
    pub(in crate::layout) vector: InlineVector,
}

impl InlineVisualOffset {
    pub(in crate::layout) fn zero() -> Self {
        Self {
            vector: InlineVector::new(0.0, 0.0),
        }
    }

    pub(in crate::layout) fn from_relative_offset(offset: RelativeOffset) -> Self {
        Self {
            vector: InlineVector::new(offset.x(), offset.y()),
        }
    }

    pub(in crate::layout) fn plus(self, other: Self) -> Self {
        Self {
            vector: self.vector + other.vector,
        }
    }

    pub(in crate::layout) fn is_zero(self) -> bool {
        self.x().abs() <= 0.01 && self.y().abs() <= 0.01
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.vector.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.vector.y
    }
}

impl Default for InlineVisualOffset {
    fn default() -> Self {
        Self::zero()
    }
}

/// The used-role of a fragment split from a `::first-letter` pseudo-element.
///
/// Preserved whitespace preceding the typographic initial remains styled by
/// the pseudo, but does not itself establish `initial-letter` sizing. Keeping
/// that distinction in the fragment model lets the initial-letter exclusion
/// reserve the complete pseudo span without confusing its tab advance for the
/// glyph's cap-height geometry.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) enum FirstLetterPseudoFragmentRole {
    #[default]
    Ordinary,
    /// Associated punctuation that precedes the typographic initial.
    ///
    /// This text is inside `::first-letter`, but is not the base glyph used
    /// for `initial-letter` sizing and exclusion geometry.
    AssociatedPrefix,
    /// The selected Letter, Number, or Symbol typographic character unit.
    ///
    /// `initial-letter` geometry must be derived from this component rather
    /// than an associated punctuation fragment that happens to precede it in
    /// source order.
    TypographicInitial,
    /// Associated punctuation that follows the typographic initial.
    AssociatedSuffix,
    LeadingPreservedWhitespace,
}

/// Opaque identity shared by every fragment materialized from one
/// `::first-letter` pseudo-element.
///
/// A stream selection can split punctuation and its typographic initial into
/// separate source fragments. Paint-time grouping must retain their common
/// pseudo ownership without making either fragment generally mergeable.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct FirstLetterPseudoGroupId(u64);

impl FirstLetterPseudoGroupId {
    pub(in crate::layout) fn allocate() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragmentData {
    pub(in crate::layout) text: Rc<str>,
    /// Opaque identity for one authored text run. Graph splitting preserves
    /// this across typographic units so paint projection can distinguish
    /// tracking fragments from independently authored adjacent text.
    pub(in crate::layout) source_run: Rc<()>,
    pub(in crate::layout) style: Rc<ComputedStyle>,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) generated_leader: bool,
    /// Whether a typographic pseudo gives this text fragment an inline
    /// decoration even though its originating display type is block-level.
    pub(in crate::layout) force_inline_background_paint: bool,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    /// A contextual source-shape selection that may be reused at paint time.
    ///
    /// This is deliberately an artifact, rather than a flag: the selected
    /// glyphs are only valid together with the full logical source, source
    /// range, and UAX #9 direction that produced them.
    pub(in crate::layout) source_shaped_selection: Option<SourceShapedSelection>,
    /// A full shaped source shared by transparent inline-boundary fragments.
    ///
    /// A CSS inline boundary can fall inside one OpenType cluster (for
    /// example a lam-alef ligature). Such a cluster cannot be represented by
    /// independently slicing glyphs for each lexical fragment, but it must
    /// remain available after line selection and bidi reordering.
    pub(in crate::layout) boundary_shaped_source: Option<Rc<BoundaryShapedSource>>,
    /// This fragment's authored range in [`BoundaryShapedSource::shaped`].
    pub(in crate::layout) boundary_shaped_range: Option<std::ops::Range<usize>>,
    /// UAX #9 direction resolved for a selected visual source slice.
    ///
    /// Logical source fragments leave this unset. It is only carried after
    /// mixed-inline visual reordering so the final paint shaping pass can
    /// preserve the already-selected order and glyph mirroring level.
    pub(in crate::layout) resolved_bidi_direction: Option<ResolvedBidiDirection>,
    pub(in crate::layout) ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
    /// The source inline ancestry used to resolve visual tracking boundaries.
    pub(in crate::layout) tracking_scope: Option<Rc<InlineTrackingScope>>,
    /// Whether this item starts a separate visual fragment after UAX #9
    /// reordering. Tracking must not reconnect text across that fragment
    /// boundary even when the adjacent items share an inline ancestor.
    pub(in crate::layout) starts_visual_fragment: bool,
    /// A `hyphenate-character` materialized at a selected discretionary
    /// boundary. It is painted with its own styled inline item, while the
    /// graph source range remains the owner of extraction and decoration.
    ///
    /// This is semantic used-line metadata, rather than a character test:
    /// markers may be transparent, RTL, or contain bidi-neutral text.
    pub(in crate::layout) selected_discretionary_marker: bool,
    pub(in crate::layout) first_letter_pseudo_role: FirstLetterPseudoFragmentRole,
    pub(in crate::layout) first_letter_pseudo_group_id: Option<FirstLetterPseudoGroupId>,
    /// A visual inline advance retained after an initial-letter exclusion has
    /// removed this pseudo fragment from normal line advancement.
    pub(in crate::layout) out_of_flow_paint_inline_advance: Option<LayoutLength>,
    /// A visual block extent retained with an out-of-flow first-letter prefix
    /// without allowing that prefix to enlarge its ordinary line box.
    pub(in crate::layout) out_of_flow_paint_block_size: Option<LayoutLength>,
}

/// One complete shaped source retained across transparent inline boundaries.
///
/// CSS Text boundary shaping is performed before individual inline fragments
/// are placed in visual order. Keeping the full glyph stream lets paint reuse
/// a ligature or contextual cluster that spans those fragments instead of
/// falling back to independently shaped source slices:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct BoundaryShapedSource {
    pub(in crate::layout) shaped: Rc<ShapedInlineLine>,
}

/// A glyph slice selected from one complete logical shaping artifact.
///
/// CSS Text chooses a soft-wrap range only after shaping has established the
/// source word's contextual forms.  Keeping both artifacts makes it
/// impossible to mark an unrelated independently-shaped fragment as reusable:
/// <https://drafts.csswg.org/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct SourceShapedSelection {
    source: Rc<ShapedInlineLine>,
    source_range: std::ops::Range<usize>,
    selected: Rc<ShapedInlineLine>,
    /// The selected range's final visual UAX #9 text. Before reordering it is
    /// the same logical text held by `selected`.
    visual_text: Rc<str>,
    resolved_bidi_direction: Option<ResolvedBidiDirection>,
}

impl SourceShapedSelection {
    /// Select a cluster-aligned range directly from its complete source.
    pub(in crate::layout) fn from_source(
        source: Rc<ShapedInlineLine>,
        source_range: std::ops::Range<usize>,
    ) -> Option<Self> {
        let selected = Rc::new(source.source_slice(source_range.clone())?);
        Some(Self {
            source,
            source_range,
            visual_text: Rc::clone(&selected.text),
            selected,
            resolved_bidi_direction: None,
        })
    }

    /// Select a child visual range while retaining the original logical
    /// source. `range` is relative to this already-selected fragment.
    pub(in crate::layout) fn subselection(&self, range: std::ops::Range<usize>) -> Option<Self> {
        let source_range =
            (self.source_range.start + range.start)..(self.source_range.start + range.end);
        if range.end > self.selected.text.len()
            || self.selected.text.get(range) != self.source.text.get(source_range.clone())
        {
            return None;
        }
        Self::from_source(Rc::clone(&self.source), source_range)
    }

    pub(in crate::layout) fn selected(&self) -> &ShapedInlineLine {
        &self.selected
    }

    pub(in crate::layout) fn selected_rc(&self) -> Rc<ShapedInlineLine> {
        Rc::clone(&self.selected)
    }

    pub(in crate::layout) fn selected_mut(&mut self) -> &mut ShapedInlineLine {
        Rc::make_mut(&mut self.selected)
    }

    pub(in crate::layout) fn replace_selected(&mut self, selected: ShapedInlineLine) {
        debug_assert_eq!(selected.text, self.selected.text);
        self.selected = Rc::new(selected);
    }

    pub(in crate::layout) fn resolve_bidi_context(
        &mut self,
        direction: ResolvedBidiDirection,
        visual_text: impl Into<Rc<str>>,
    ) {
        self.resolved_bidi_direction = Some(direction);
        self.visual_text = visual_text.into();
    }

    /// A source selection is only paintable when its selected text and UAX #9
    /// context still exactly match the fragment that carries it.
    pub(in crate::layout) fn is_reusable_for(
        &self,
        text: &str,
        direction: Option<ResolvedBidiDirection>,
    ) -> bool {
        self.visual_text.as_ref() == text
            && self.source.text.get(self.source_range.clone()) == Some(self.selected.text.as_ref())
            && self.resolved_bidi_direction == direction
    }

    /// Reassemble adjacent logical source ranges from one visual paint group.
    ///
    /// Individual RTL fragments are already in visual item order, whereas
    /// their glyphs retain positions from the one logical source run. Joining
    /// those fragments by concatenating per-fragment glyph streams would
    /// reverse that geometry. Re-slicing their contiguous logical union keeps
    /// the full source run's visual glyph order and advances intact.
    pub(in crate::layout) fn combine_contiguous(
        selections: &[&SourceShapedSelection],
    ) -> Option<ShapedInlineLine> {
        let first = *selections.first()?;
        if !selections.iter().all(|selection| {
            Rc::ptr_eq(&first.source, &selection.source)
                && first.resolved_bidi_direction == selection.resolved_bidi_direction
        }) {
            return None;
        }
        let mut ranges = selections
            .iter()
            .map(|selection| selection.source_range.clone())
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| range.start);
        let range = ranges
            .iter()
            .skip(1)
            .try_fold(ranges.first()?.clone(), |range, next| {
                (range.end == next.start).then_some(range.start..next.end)
            })?;
        first.source.source_slice(range)
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragment {
    pub(in crate::layout) data: Rc<InlineFragmentData>,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

impl InlineFragment {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        text: impl Into<String>,
        style: ComputedStyle,
        baseline_shift: f32,
        link_target: Option<String>,
        mergeable: bool,
        source: InlineTextSource,
        generated_leader: bool,
        hanging_edges: InlineHangingEdges,
        ancestor_inline_decorations: Vec<InlineAncestorDecoration>,
    ) -> Self {
        Self::new_shared_style(
            Rc::<str>::from(text.into()),
            Rc::new(style),
            baseline_shift,
            link_target.map(Rc::from),
            mergeable,
            source,
            generated_leader,
            hanging_edges,
            Rc::from(ancestor_inline_decorations.into_boxed_slice()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn new_shared_style(
        text: impl Into<Rc<str>>,
        style: InlineStyle,
        baseline_shift: f32,
        link_target: Option<Rc<str>>,
        mergeable: bool,
        source: InlineTextSource,
        generated_leader: bool,
        hanging_edges: InlineHangingEdges,
        ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
    ) -> Self {
        Self {
            data: Rc::new(InlineFragmentData {
                text: text.into(),
                source_run: Rc::new(()),
                style,
                link_target,
                mergeable,
                source,
                generated_leader,
                force_inline_background_paint: false,
                hanging_edges,
                source_shaped_selection: None,
                boundary_shaped_source: None,
                boundary_shaped_range: None,
                resolved_bidi_direction: None,
                ancestor_inline_decorations,
                tracking_scope: None,
                starts_visual_fragment: false,
                selected_discretionary_marker: false,
                first_letter_pseudo_role: FirstLetterPseudoFragmentRole::Ordinary,
                first_letter_pseudo_group_id: None,
                out_of_flow_paint_inline_advance: None,
                out_of_flow_paint_block_size: None,
            }),
            baseline_shift,
            visual_offset: InlineVisualOffset::zero(),
        }
    }

    pub(in crate::layout) fn with_baseline_shift(mut self, baseline_shift: f32) -> Self {
        self.baseline_shift = baseline_shift;
        self
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

    pub(in crate::layout) fn with_source_run(mut self, source_run: Rc<()>) -> Self {
        Rc::make_mut(&mut self.data).source_run = source_run;
        self
    }

    pub(in crate::layout) fn source_run(&self) -> &Rc<()> {
        &self.data.source_run
    }

    pub(in crate::layout) fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>> {
        self.data.tracking_scope.as_ref()
    }

    pub(in crate::layout) fn line_relative_scope(&self) -> Option<&InlineTrackingScope> {
        self.tracking_scope()
            .and_then(|scope| scope.nearest_line_relative_scope())
    }

    pub(in crate::layout) fn line_relative_alignment(
        &self,
    ) -> Option<InlineScopeLineRelativeAlignment> {
        // Text has no principal box of its own. A computed `top`/`bottom`
        // value belongs to the element that established its enclosing inline
        // box, and block boxes do not establish such a scope. Reading the
        // fragment style directly would therefore make a block descendant's
        // inapplicable `vertical-align` reposition its anonymous text.
        self.line_relative_scope()
            .and_then(InlineTrackingScope::line_relative_alignment)
    }

    pub(in crate::layout) fn starts_visual_fragment(&self) -> bool {
        self.data.starts_visual_fragment
    }

    pub(in crate::layout) fn mark_starts_visual_fragment(&mut self) {
        Rc::make_mut(&mut self.data).starts_visual_fragment = true;
    }

    pub(in crate::layout) fn mark_selected_discretionary_marker(&mut self) {
        let data = Rc::make_mut(&mut self.data);
        data.selected_discretionary_marker = true;
        // A used marker is an independently styled, logical inline item. It
        // must not be coalesced back into the source fragment that caused the
        // selected break.
        data.mergeable = false;
    }

    #[cfg(test)]
    pub(in crate::layout) fn is_selected_discretionary_marker(&self) -> bool {
        self.data.selected_discretionary_marker
    }

    pub(in crate::layout) fn set_first_letter_pseudo_role(
        &mut self,
        role: FirstLetterPseudoFragmentRole,
    ) {
        Rc::make_mut(&mut self.data).first_letter_pseudo_role = role;
    }

    pub(in crate::layout) fn first_letter_pseudo_role(&self) -> FirstLetterPseudoFragmentRole {
        self.data.first_letter_pseudo_role
    }

    pub(in crate::layout) fn set_first_letter_pseudo_group_id(
        &mut self,
        group_id: FirstLetterPseudoGroupId,
    ) {
        Rc::make_mut(&mut self.data).first_letter_pseudo_group_id = Some(group_id);
    }

    pub(in crate::layout) fn first_letter_pseudo_group_id(
        &self,
    ) -> Option<FirstLetterPseudoGroupId> {
        self.data.first_letter_pseudo_group_id
    }

    pub(in crate::layout) fn set_out_of_flow_paint_inline_advance(
        &mut self,
        advance: LayoutLength,
    ) {
        Rc::make_mut(&mut self.data).out_of_flow_paint_inline_advance = Some(advance);
    }

    pub(in crate::layout) fn out_of_flow_paint_inline_advance(&self) -> Option<LayoutLength> {
        self.data.out_of_flow_paint_inline_advance
    }

    pub(in crate::layout) fn set_out_of_flow_paint_block_size(&mut self, size: LayoutLength) {
        Rc::make_mut(&mut self.data).out_of_flow_paint_block_size = Some(size);
    }

    pub(in crate::layout) fn out_of_flow_paint_block_size(&self) -> Option<LayoutLength> {
        self.data.out_of_flow_paint_block_size
    }

    pub(in crate::layout) fn with_hanging_edges(
        mut self,
        hanging_edges: InlineHangingEdges,
    ) -> Self {
        Rc::make_mut(&mut self.data).hanging_edges = hanging_edges;
        self
    }

    pub(in crate::layout) fn set_text(&mut self, text: impl Into<Rc<str>>) {
        let data = Rc::make_mut(&mut self.data);
        let text = text.into();
        if data.text != text {
            data.source_shaped_selection = None;
            data.boundary_shaped_source = None;
            data.boundary_shaped_range = None;
        }
        data.text = text;
    }

    pub(in crate::layout) fn set_mergeable(&mut self, mergeable: bool) {
        Rc::make_mut(&mut self.data).mergeable = mergeable;
    }

    pub(in crate::layout) fn set_source_shaped_selection(
        &mut self,
        selection: Option<SourceShapedSelection>,
    ) {
        if let Some(selection) = &selection {
            debug_assert!(
                selection.source.text.get(selection.source_range.clone())
                    == Some(self.data.text.as_ref())
            );
        }
        Rc::make_mut(&mut self.data).source_shaped_selection = selection;
    }

    pub(in crate::layout) fn source_shaped_selection(&self) -> Option<&SourceShapedSelection> {
        self.data.source_shaped_selection.as_ref()
    }

    pub(in crate::layout) fn set_boundary_shaped_source(
        &mut self,
        source: Rc<BoundaryShapedSource>,
        range: std::ops::Range<usize>,
    ) {
        let data = Rc::make_mut(&mut self.data);
        data.boundary_shaped_source = Some(source);
        data.boundary_shaped_range = Some(range);
    }

    pub(in crate::layout) fn boundary_shaped_source(&self) -> Option<&BoundaryShapedSource> {
        self.data.boundary_shaped_source.as_deref()
    }

    pub(in crate::layout) fn boundary_shaped_range(&self) -> Option<&std::ops::Range<usize>> {
        self.data.boundary_shaped_range.as_ref()
    }

    /// Return the complete boundary source identity before a visual-selection
    /// step changes this fragment's paint text.  UAX #9 removes formatting
    /// controls from the selected text, but that must not discard the
    /// logical source which owns their shaping context.
    pub(in crate::layout) fn boundary_shaped_source_and_range(
        &self,
    ) -> Option<(Rc<BoundaryShapedSource>, std::ops::Range<usize>)> {
        Some((
            Rc::clone(self.data.boundary_shaped_source.as_ref()?),
            self.data.boundary_shaped_range.as_ref()?.clone(),
        ))
    }

    pub(in crate::layout) fn set_resolved_bidi_direction(
        &mut self,
        value: Option<ResolvedBidiDirection>,
    ) {
        Rc::make_mut(&mut self.data).resolved_bidi_direction = value;
    }

    /// Preserve a `::first-line` background on text directly owned by a block
    /// formatting context. Such a fragment has a block computed display, but
    /// its pseudo decoration still paints as an inline line fragment.
    pub(in crate::layout) fn set_force_inline_background_paint(&mut self, value: bool) {
        Rc::make_mut(&mut self.data).force_inline_background_paint = value;
    }

    pub(in crate::layout) fn force_inline_background_paint(&self) -> bool {
        self.data.force_inline_background_paint
    }

    #[cfg(test)]
    pub(in crate::layout) fn set_link_target(&mut self, link_target: Option<String>) {
        Rc::make_mut(&mut self.data).link_target = link_target.map(Rc::from);
    }

    #[cfg(test)]
    pub(in crate::layout) fn set_generated_leader(&mut self, generated_leader: bool) {
        Rc::make_mut(&mut self.data).generated_leader = generated_leader;
    }

    pub(in crate::layout) fn style_mut(&mut self) -> &mut ComputedStyle {
        Rc::make_mut(&mut Rc::make_mut(&mut self.data).style)
    }

    /// Apply a line-local inherited-style adjustment to every retained inline
    /// ancestor decoration. These snapshots own cloned background and border
    /// paint; leaving them at the pre-`::first-line` foreground would resolve
    /// their `currentcolor` against the wrong fragment color.
    pub(in crate::layout) fn apply_to_ancestor_inline_decoration_styles(
        &mut self,
        mut apply: impl FnMut(&mut ComputedStyle),
    ) {
        let data = Rc::make_mut(&mut self.data);
        let decorations = Rc::make_mut(&mut data.ancestor_inline_decorations);
        decorations
            .iter_mut()
            .for_each(|decoration| apply(&mut decoration.style));
    }

    pub(in crate::layout) fn text(&self) -> &str {
        &self.data.text
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
    }

    pub(in crate::layout) fn link_target(&self) -> Option<&str> {
        self.data.link_target.as_deref()
    }

    pub(in crate::layout) fn mergeable(&self) -> bool {
        self.data.mergeable
    }

    pub(in crate::layout) fn source(&self) -> InlineTextSource {
        self.data.source
    }

    pub(in crate::layout) fn generated_leader(&self) -> bool {
        self.data.generated_leader
    }

    pub(in crate::layout) fn hanging_edges(&self) -> InlineHangingEdges {
        self.data.hanging_edges
    }

    pub(in crate::layout) fn ancestor_inline_decorations(&self) -> &[InlineAncestorDecoration] {
        &self.data.ancestor_inline_decorations
    }

    /// Retain a paint-only lexical ancestor while materializing a typographic
    /// pseudo.  The graph owns first-letter splitting, so this metadata must
    /// be attached there rather than reconstructed after bidi reordering.
    pub(in crate::layout) fn push_ancestor_inline_decoration(
        &mut self,
        decoration: InlineAncestorDecoration,
    ) {
        let data = Rc::make_mut(&mut self.data);
        let mut decorations = data.ancestor_inline_decorations.to_vec();
        decorations.push(decoration);
        data.ancestor_inline_decorations = Rc::from(decorations.into_boxed_slice());
    }
}

pub(in crate::layout) trait InlineFragmentAccess {
    fn text(&self) -> &str;
    fn style(&self) -> &ComputedStyle;
    fn baseline_shift(&self) -> f32;
    fn visual_offset(&self) -> InlineVisualOffset;
    fn link_target(&self) -> Option<&str>;
    fn mergeable(&self) -> bool;
    fn source(&self) -> InlineTextSource;
    /// Opaque identity for the authored text run that produced this fragment.
    ///
    /// This survives graph-level typographic-unit splitting so paint metadata
    /// can reconstitute one source word without conflating independent text.
    fn source_run(&self) -> &Rc<()>;
    /// The lexical inline-ancestry scope that owns this fragment.
    ///
    /// Reordering can split one scoped source run into visual fragments. The
    /// shared scope distinguishes those internal fragments from text across
    /// an actual `unicode-bidi` isolate boundary.
    fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>>;
    fn generated_leader(&self) -> bool;
    fn first_letter_pseudo_group_id(&self) -> Option<FirstLetterPseudoGroupId> {
        None
    }
    /// A complete source shape shared with adjacent transparent fragments.
    fn boundary_shaped_source(&self) -> Option<&BoundaryShapedSource> {
        None
    }
    /// This fragment's authored range in the shared boundary shape.
    fn boundary_shaped_range(&self) -> Option<&std::ops::Range<usize>> {
        None
    }
    fn source_shaped_selection(&self) -> Option<&SourceShapedSelection> {
        None
    }
    fn resolved_bidi_direction(&self) -> Option<ResolvedBidiDirection>;
    fn ancestor_inline_decorations(&self) -> &[InlineAncestorDecoration];
}

impl InlineFragmentAccess for InlineFragment {
    fn text(&self) -> &str {
        self.text()
    }

    fn style(&self) -> &ComputedStyle {
        self.style()
    }

    fn baseline_shift(&self) -> f32 {
        self.baseline_shift
    }

    fn visual_offset(&self) -> InlineVisualOffset {
        self.visual_offset
    }

    fn link_target(&self) -> Option<&str> {
        self.link_target()
    }

    fn mergeable(&self) -> bool {
        self.mergeable()
    }

    fn source(&self) -> InlineTextSource {
        self.source()
    }

    fn source_run(&self) -> &Rc<()> {
        InlineFragment::source_run(self)
    }

    fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>> {
        InlineFragment::tracking_scope(self)
    }

    fn generated_leader(&self) -> bool {
        self.generated_leader()
    }

    fn first_letter_pseudo_group_id(&self) -> Option<FirstLetterPseudoGroupId> {
        InlineFragment::first_letter_pseudo_group_id(self)
    }

    fn boundary_shaped_source(&self) -> Option<&BoundaryShapedSource> {
        InlineFragment::boundary_shaped_source(self)
    }

    fn boundary_shaped_range(&self) -> Option<&std::ops::Range<usize>> {
        InlineFragment::boundary_shaped_range(self)
    }

    fn source_shaped_selection(&self) -> Option<&SourceShapedSelection> {
        InlineFragment::source_shaped_selection(self)
    }

    fn resolved_bidi_direction(&self) -> Option<ResolvedBidiDirection> {
        self.data.resolved_bidi_direction
    }

    fn ancestor_inline_decorations(&self) -> &[InlineAncestorDecoration] {
        self.ancestor_inline_decorations()
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PendingInlineFragment<'a> {
    fragment: &'a InlineFragment,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
}

impl<'a> PendingInlineFragment<'a> {
    pub(in crate::layout) fn new(fragment: &'a InlineFragment) -> Self {
        Self {
            fragment,
            baseline_shift: fragment.baseline_shift,
            visual_offset: fragment.visual_offset,
        }
    }

    pub(in crate::layout) fn to_owned_fragment(self) -> InlineFragment {
        self.fragment
            .clone()
            .with_baseline_shift(self.baseline_shift())
            .with_visual_offset(self.visual_offset())
    }

    pub(in crate::layout) fn line_relative_alignment(
        self,
    ) -> Option<InlineScopeLineRelativeAlignment> {
        self.fragment.line_relative_alignment()
    }
}

impl InlineFragmentAccess for PendingInlineFragment<'_> {
    fn text(&self) -> &str {
        self.fragment.text()
    }

    fn style(&self) -> &ComputedStyle {
        self.fragment.style()
    }

    fn baseline_shift(&self) -> f32 {
        self.baseline_shift
    }

    fn visual_offset(&self) -> InlineVisualOffset {
        self.visual_offset
    }

    fn link_target(&self) -> Option<&str> {
        self.fragment.link_target()
    }

    fn mergeable(&self) -> bool {
        self.fragment.mergeable()
    }

    fn source(&self) -> InlineTextSource {
        self.fragment.source()
    }

    fn source_run(&self) -> &Rc<()> {
        self.fragment.source_run()
    }

    fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>> {
        self.fragment.tracking_scope()
    }

    fn generated_leader(&self) -> bool {
        self.fragment.generated_leader()
    }

    fn first_letter_pseudo_group_id(&self) -> Option<FirstLetterPseudoGroupId> {
        self.fragment.first_letter_pseudo_group_id()
    }

    fn boundary_shaped_source(&self) -> Option<&BoundaryShapedSource> {
        self.fragment.boundary_shaped_source()
    }

    fn boundary_shaped_range(&self) -> Option<&std::ops::Range<usize>> {
        self.fragment.boundary_shaped_range()
    }

    fn source_shaped_selection(&self) -> Option<&SourceShapedSelection> {
        self.fragment.source_shaped_selection()
    }

    fn resolved_bidi_direction(&self) -> Option<ResolvedBidiDirection> {
        self.fragment.data.resolved_bidi_direction
    }

    fn ancestor_inline_decorations(&self) -> &[InlineAncestorDecoration] {
        self.fragment.ancestor_inline_decorations()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineTextSource {
    Normal,
    Generated,
    /// Generated U+200B emitted by the HTML UA `<wbr>` pseudo-element.
    ///
    /// It remains distinct until `word-space-transform` determines whether
    /// the separator survives its surrounding source boundary.
    GeneratedWbr,
    /// A layout-only replacement introduced by `word-space-transform`.
    ///
    /// The replacement space participates in layout and painting, but its PDF
    /// text mapping retains the authored U+200B or omits an HTML `<wbr>`.
    /// <https://drafts.csswg.org/css-text-4/#word-space-transform>
    WordSpaceTransform(ExplicitWordSeparatorSource),
    /// The anonymous inline generated by CSS Overflow's block ellipsis.
    ///
    /// It remains paintable with the terminal root inline's style, but has no
    /// line-box extent of its own (`line-height: 0`).
    /// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
    BlockEllipsis,
    /// Generated text emitted by a `::footnote-call` pseudo-element.
    ///
    /// The call identity survives inline collection and line selection so a
    /// footnote is assigned only when its selected line is committed to a
    /// fragmentainer, rather than while a speculative measurement traverses
    /// the source tree.
    FootnoteCall(ElementId),
    RunIn,
    Marker,
    /// A directional formatting control synthesized from a CSS `unicode-bidi`
    /// scope. This remains distinct from authored control characters so
    /// line-local bidi resolution can restore only CSS scope boundaries after
    /// soft wrapping.
    BidiControl,
}

/// Source ownership for an explicit CSS Text expandable separator.
///
/// U+200B remains part of document text, while `<wbr>` is an HTML element
/// represented by generated U+200B in the UA stylesheet and contributes no
/// character to plain-text extraction.
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>
/// <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-wbr-element>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum ExplicitWordSeparatorSource {
    AuthoredZeroWidthSpace,
    HtmlWbr,
}

impl ExplicitWordSeparatorSource {
    pub(in crate::layout) const fn extraction_text(self) -> Option<&'static str> {
        match self {
            Self::AuthoredZeroWidthSpace => Some("\u{200b}"),
            Self::HtmlWbr => None,
        }
    }
}

impl InlineTextSource {
    pub(in crate::layout) fn is_generated(self) -> bool {
        matches!(
            self,
            Self::Generated | Self::GeneratedWbr | Self::BlockEllipsis | Self::FootnoteCall(_)
        )
    }

    pub(in crate::layout) fn footnote_call(self) -> Option<ElementId> {
        match self {
            Self::FootnoteCall(element) => Some(element),
            _ => None,
        }
    }
}

/// Inline edge decorations that affect CSS Text hanging punctuation.
///
/// CSS Text prevents hanging punctuation when nonzero inline-axis padding or
/// border separates the punctuation from the line edge, even when that edge
/// belongs to an ancestor inline box rather than the text fragment itself:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) struct InlineHangingEdges {
    pub(in crate::layout) blocks_start: bool,
    pub(in crate::layout) blocks_end: bool,
}

/// Background and border decoration inherited from an ancestor inline box.
///
/// CSS 2.2 paints a non-replaced inline box's backgrounds and borders across
/// its generated inline boxes, including descendant inline text with different
/// computed styles. Keeping ancestor decorations as paint-only fragment
/// metadata avoids duplicating shaped text while preserving the ancestor's
/// inline-box decoration geometry:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-color>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct InlineAncestorDecoration {
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    /// Whether this occurrence paints the ancestor's background and border.
    ///
    /// A positioned inline's own text fragment carries the containing-block
    /// metadata below, but already paints its matching style directly.  Keep
    /// those two responsibilities separate so recording positioning geometry
    /// cannot paint the same inline box twice.
    pub(in crate::layout) paints_background_or_border: bool,
    /// A positioned inline scope whose generated line fragment contains this
    /// source. This is metadata-only when `style` has no paintable decoration.
    /// It lets positioned-layout recover the physical union of fragmented
    /// inline boxes without inferring ownership from visual order.
    pub(in crate::layout) positioning_containing_block_id:
        Option<InlinePositioningContainingBlockId>,
    /// Identity of an opacity-owning lexical inline scope.
    ///
    /// Opacity is non-inherited, but its effect subtree must survive text
    /// splitting and bidi reordering.  Equal opacity values are not a valid
    /// substitute for lexical identity: two sibling inline boxes each form a
    /// separate compositing group.
    /// <https://drafts.csswg.org/css-color-4/#transparency>
    pub(in crate::layout) paint_effect_scope_id: Option<InlinePaintScopeId>,
}

/// Whether an inline style establishes a non-auto positioned stacking context.
///
/// Non-atomic inline descendants are represented by text runs plus lexical
/// edge metadata rather than by one principal paint box.  Retaining this fact
/// on the lexical scope lets the text painter place its captured run in the
/// owning Appendix-E band.
/// <https://www.w3.org/TR/CSS22/zindex.html>
pub(in crate::layout) fn inline_style_establishes_positioned_stacking_context(
    style: &ComputedStyle,
) -> bool {
    matches!(style.position, Position::Relative | Position::Sticky)
        && style.z_index.establishes_stacking_context()
}

/// Opaque identity for a lexical inline paint-effect subtree.
///
/// A monotonically allocated id is intentionally independent of style
/// equality, source text, and visual order.  It is carried by copied inline
/// metadata through typographic-unit splitting and UAX #9 reordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct InlinePaintScopeId(u64);

impl InlinePaintScopeId {
    pub(in crate::layout) fn allocate() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Return whether two ancestor-decoration chains require the same text paint
/// subtree.
///
/// Inline backgrounds and borders are emitted before the line's text and do
/// not alter glyph painting or shaping. Opacity, however, creates a stacking
/// context, so its lexical scope must remain a text-group boundary:
/// <https://www.w3.org/TR/css-color-4/#transparency> and
/// <https://www.w3.org/TR/CSS22/zindex.html#painting-order>.
pub(in crate::layout) fn inline_ancestor_decorations_have_same_text_paint_effect(
    left: &[InlineAncestorDecoration],
    right: &[InlineAncestorDecoration],
) -> bool {
    left.iter()
        .filter_map(|decoration| decoration.paint_effect_scope_id)
        .eq(right
            .iter()
            .filter_map(|decoration| decoration.paint_effect_scope_id))
        && left
            .iter()
            .filter(|decoration| {
                inline_style_establishes_positioned_stacking_context(&decoration.style)
            })
            .map(|decoration| decoration.positioning_containing_block_id)
            .eq(right
                .iter()
                .filter(|decoration| {
                    inline_style_establishes_positioned_stacking_context(&decoration.style)
                })
                .map(|decoration| decoration.positioning_containing_block_id))
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct DefinitionListColumnItem<'a> {
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) signature: ElementSignature,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) children: Option<&'a [box_tree::FormattingBox<'a>]>,
}

#[derive(Debug, Clone, Copy)]
/// Static-position data captured while the hypothetical normal-flow source
/// still participates in its formatting context.
///
/// The rectangle is page-coordinate geometry. Text layout is entered from the
/// resolved positioned content box, not from an alternate static-position
/// baseline.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
pub(in crate::layout) struct StaticPositionCapture {
    /// The sole page-coordinate representation of the static position.
    pub(in crate::layout) rectangle: StaticPositionRectangle,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PhysicalStaticAxisFallback {
    pub(in crate::layout) left: f32,
    pub(in crate::layout) right: f32,
    pub(in crate::layout) can_fall_outside: bool,
}

impl PhysicalStaticAxisFallback {
    pub(in crate::layout) fn new(left: f32, right: f32) -> Self {
        Self {
            left,
            right,
            can_fall_outside: false,
        }
    }

    pub(in crate::layout) fn new_unclamped(left: f32, right: f32) -> Self {
        Self {
            left,
            right,
            can_fall_outside: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsoluteStaticPosition {
    page_left_x: f32,
    page_right_x: f32,
    page_top_y: f32,
    horizontal_can_fall_outside: bool,
    has_vertical_position: bool,
    static_alignment_source: Option<StaticAlignmentSource>,
}

/// The physical content rectangle and applicable inline-axis alignment default
/// owned by one block formatting context for descendants' block-layout static-
/// position rectangles.
///
/// This record is captured when the owner enters child layout and stays
/// immutable through inline collection and deferred positioned replay.
/// Anonymous inline wrappers have no principal box geometry or independent
/// `justify-items` default, so they must use this owner rather than
/// reconstructing state from their own used style. `align-items` is not
/// retained: it does not apply to hypothetical block-level children.
/// <https://www.w3.org/TR/css-position-3/#static-position>
/// <https://drafts.csswg.org/css-align-3/#align-self-property>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct StaticPositionContainingBlock {
    pub(in crate::layout) axes: WritingModeAxes,
    pub(in crate::layout) content_rect: PageTopRect,
    pub(in crate::layout) justify_items: css::SelfAlignment,
}

impl StaticPositionContainingBlock {
    pub(in crate::layout) fn new(
        axes: WritingModeAxes,
        content_rect: PageTopRect,
        justify_items: css::SelfAlignment,
    ) -> Self {
        Self {
            axes,
            content_rect,
            justify_items,
        }
    }

    /// Builds CSS Position's block-layout static-position rectangle from the
    /// hypothetical box's used border box. The static rectangle spans the
    /// static-position containing block in the inline axis and has zero
    /// block-axis extent at the hypothetical box's block-start edge.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    pub(in crate::layout) fn rectangle_at_hypothetical_block_box(
        self,
        hypothetical_border_box: PageTopRect,
    ) -> StaticPositionRectangle {
        let block_start = match self.axes.physical_side(LogicalSide::BlockStart) {
            PhysicalSide::Left => hypothetical_border_box.x(),
            PhysicalSide::Right => hypothetical_border_box.x() + hypothetical_border_box.width(),
            PhysicalSide::Top => hypothetical_border_box.top_y(),
            PhysicalSide::Bottom => {
                hypothetical_border_box.top_y() + hypothetical_border_box.height()
            }
        };
        let area = match self.axes.physical_axis(LogicalAxis::Inline) {
            PhysicalAxis::Horizontal => PageTopRect::new(
                self.content_rect.x(),
                block_start,
                self.content_rect.width(),
                0.0,
            ),
            PhysicalAxis::Vertical => PageTopRect::new(
                block_start,
                self.content_rect.top_y(),
                0.0,
                self.content_rect.height(),
            ),
        };
        StaticPositionRectangle {
            area,
            writing_mode: self.axes.writing_mode(),
            direction: self.axes.direction(),
            justify_items: self.justify_items,
            align_items: css::SelfAlignment::NORMAL,
        }
    }
}

/// The spec-defined alignment container used to resolve an absolutely
/// positioned child's static insets.
///
/// Capture it while normal flow still owns the hypothetical source fragment;
/// deferred positioned replay can otherwise see only an outer formatting
/// context.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct StaticPositionRectangle {
    pub(in crate::layout) area: PageTopRect,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) justify_items: css::SelfAlignment,
    pub(in crate::layout) align_items: css::SelfAlignment,
}

impl StaticPositionRectangle {
    /// CSS 2's both-auto `left`/`right` rule is selected by the direction of
    /// the static-position containing block. In vertical writing this is the
    /// physical horizontal direction of that block axis, not the positioned
    /// subject's inline direction.
    /// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
    /// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) fn css2_horizontal_direction(self) -> Direction {
        crate::layout::assets::physical_horizontal_axis_direction(self.writing_mode, self.direction)
    }
}

/// The formatting context owns the exact source of static-position alignment.
/// Ordinary flow keeps its immutable rectangle separate from the alignment
/// resolved later for the positioned subject; Flexbox and Grid provide an
/// already-resolved alignment rectangle of their own.
#[derive(Debug, Clone, Copy)]
enum StaticAlignmentSource {
    OrdinaryFlow {
        rectangle: StaticPositionRectangle,
        alignment: Option<AbsposStaticAlignment>,
    },
    FormattingContext(AbsposStaticAlignment),
}

/// Static-position alignment data retained until an absolutely positioned
/// child has a used margin-box size.
///
/// The static-position rectangle may come from ordinary flow, Flexbox, or
/// Grid.  Keeping the rectangle, its writing mode, and its already-resolved
/// static-position self-alignment together makes positioned layout independent
/// of the formatting context that supplied it:
/// <https://drafts.csswg.org/css-position-3/#static-position>
/// <https://drafts.csswg.org/css-align-3/#align-abspos>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsposStaticAlignment {
    /// The spec-defined static-position rectangle. Unlike an absolute
    /// positioning containing block this is allowed (and, for ordinary block
    /// and inline layout, required) to be zero-sized in one physical axis.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    pub(in crate::layout) area: PageTopRect,
    /// Axes of the static-position containing block / alignment container.
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    /// Axes of the positioned alignment subject. These differ from the
    /// container axes for `self-start` and `self-end`.
    pub(in crate::layout) subject_writing_mode: WritingMode,
    pub(in crate::layout) subject_direction: Direction,
    /// The effective `justify-self` used while calculating the static
    /// position. `auto` has already inherited the appropriate parent default.
    pub(in crate::layout) inline: css::SelfAlignment,
    /// The effective `align-self` used while calculating the static position.
    /// `auto` has already inherited the appropriate parent default.
    pub(in crate::layout) block: css::SelfAlignment,
}

impl AbsposStaticAlignment {
    pub(in crate::layout) fn new(
        area: PageTopRect,
        writing_mode: WritingMode,
        direction: Direction,
        subject_writing_mode: WritingMode,
        subject_direction: Direction,
        inline: css::SelfAlignment,
        block: css::SelfAlignment,
    ) -> Self {
        Self {
            area,
            writing_mode,
            direction,
            subject_writing_mode,
            subject_direction,
            inline,
            block,
        }
    }

    fn alignment_for_physical_axis(self, axis: PhysicalAxis) -> css::SelfAlignment {
        let axes = WritingModeAxes::new(self.writing_mode, self.direction);
        if axes.logical_axis_for_physical(axis) == LogicalAxis::Inline {
            self.inline
        } else {
            self.block
        }
    }

    fn alignment_direction(self, axis: PhysicalAxis) -> AbsposAlignmentDirection {
        let container_axes = WritingModeAxes::new(self.writing_mode, self.direction);
        let container_logical_axis = container_axes.logical_axis_for_physical(axis);
        let subject_axes = WritingModeAxes::new(self.subject_writing_mode, self.subject_direction);
        let subject_logical_axis = subject_axes.logical_axis_for_physical(axis);
        let container_reverse_start_end = matches!(
            container_axes.physical_start_side(container_logical_axis),
            PhysicalSide::Right | PhysicalSide::Bottom
        );
        let subject_reverse_start_end = matches!(
            subject_axes.physical_start_side(subject_logical_axis),
            PhysicalSide::Right | PhysicalSide::Bottom
        );
        AbsposAlignmentDirection {
            container_reverse_start_end,
            subject_reverse_start_end,
            left_right_are_physical: axis == PhysicalAxis::Horizontal
                && container_logical_axis == LogicalAxis::Inline,
            left_right_resolve_to_inline_edges: container_logical_axis == LogicalAxis::Inline,
        }
    }

    pub(in crate::layout) fn available_outer_size(
        self,
        axis: PhysicalAxis,
        containing_block: ContainingBlock,
    ) -> Option<f32> {
        let alignment = self.alignment_for_physical_axis(axis);
        let direction = self.alignment_direction(axis);
        let (area_start, area_size, containing_start, containing_size) = match axis {
            PhysicalAxis::Horizontal => (
                self.area.x(),
                self.area.width(),
                containing_block.x(),
                containing_block.width(),
            ),
            PhysicalAxis::Vertical => (
                self.area.top_y(),
                self.area.height(),
                containing_block.top_y(),
                containing_block.height(),
            ),
        };
        match alignment.keyword {
            SelfAlignmentKeyword::Center => Some({
                let center = area_start + area_size / 2.0;
                (2.0 * (center - containing_start).min(containing_start + containing_size - center))
                    .max(0.0)
            }),
            keyword => {
                // CSS Align §6.5 sizes a statically positioned automatic
                // inline axis from the static rectangle edge selected by the
                // alignment value to the opposite edge of the actual
                // containing block. This is intentionally not the static
                // rectangle's own width: ordinary-flow rectangles are
                // degenerate in one axis.
                // <https://drafts.csswg.org/css-align-3/#abspos-static-size>
                let physical_start_is_left = match keyword {
                    SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd => {
                        let subject_start_is_left = !direction.subject_reverse_start_end;
                        if matches!(keyword, SelfAlignmentKeyword::SelfStart) {
                            subject_start_is_left
                        } else {
                            !subject_start_is_left
                        }
                    }
                    SelfAlignmentKeyword::End | SelfAlignmentKeyword::FlexEnd => {
                        direction.container_reverse_start_end
                    }
                    SelfAlignmentKeyword::Right if direction.left_right_are_physical => false,
                    SelfAlignmentKeyword::Left if direction.left_right_are_physical => true,
                    _ => !direction.container_reverse_start_end,
                };
                let static_edge = if physical_start_is_left {
                    area_start
                } else {
                    area_start + area_size
                };
                let containing_opposite_edge = if physical_start_is_left {
                    containing_start + containing_size
                } else {
                    containing_start
                };
                Some((containing_opposite_edge - static_edge).abs().max(0.0))
            }
        }
    }

    pub(in crate::layout) fn available_horizontal_outer_size(
        self,
        containing_block: ContainingBlock,
    ) -> Option<f32> {
        self.available_outer_size(PhysicalAxis::Horizontal, containing_block)
    }

    /// Resolve the static position to the physical margin-box start inset.
    ///
    /// [`PositionedAxis::start`] is the CSS inset before its start margin;
    /// positioned layout applies that margin when it projects the resolved
    /// axis to the border box. Returning a border-box edge here would make
    /// aligned absolutely positioned boxes consume their start margin twice.
    /// CSS Box Alignment aligns the margin box, including for Flexbox and
    /// Grid static-position rectangles:
    /// <https://drafts.csswg.org/css-align-3/#align-abspos>.
    pub(in crate::layout) fn horizontal_static_position(
        self,
        containing_block: ContainingBlock,
        border_box_width: f32,
        margin_left: f32,
        margin_right: f32,
    ) -> PhysicalStaticAxisFallback {
        let outer_width = margin_left + border_box_width + margin_right;
        let alignment = self.alignment_for_physical_axis(PhysicalAxis::Horizontal);
        let direction = self.alignment_direction(PhysicalAxis::Horizontal);
        let offset = abspos_static_alignment_offset(
            alignment,
            self.area.width(),
            border_box_width,
            margin_left,
            margin_right,
            direction,
        );
        // Convert the border-edge alignment offset to a margin-box offset
        // before adding the rectangle origin. Besides expressing the correct
        // coordinate contract directly, this preserves an exact zero for
        // start alignment instead of rounding `origin + margin - margin`.
        let margin_box_offset = offset - margin_left;
        let candidate_margin_box_start = self.area.x() + margin_box_offset;
        let candidate_overflows_containing_block = candidate_margin_box_start
            < containing_block.x()
            || candidate_margin_box_start + outer_width
                > containing_block.x() + containing_block.width();
        let margin_box_start = if safe_alignment_falls_back_to_static_position_start(
            alignment,
            offset,
            self.area.width(),
            border_box_width,
            margin_left,
            margin_right,
            direction,
            candidate_overflows_containing_block,
        ) {
            if direction.container_reverse_start_end {
                self.area.x() + self.area.width() - outer_width
            } else {
                self.area.x()
            }
        } else {
            candidate_margin_box_start
        };
        let start_in_containing_block = margin_box_start - containing_block.x();
        PhysicalStaticAxisFallback::new_unclamped(
            start_in_containing_block,
            containing_block.width() - start_in_containing_block - outer_width,
        )
    }

    /// Resolve the static position to the physical margin-box start inset.
    ///
    /// See [`Self::horizontal_static_position`] for why this deliberately
    /// excludes the start margin from the returned absolute-position inset.
    pub(in crate::layout) fn vertical_static_start(
        self,
        containing_block: ContainingBlock,
        border_box_height: f32,
        margin_top: f32,
        margin_bottom: f32,
    ) -> f32 {
        let outer_height = margin_top + border_box_height + margin_bottom;
        let alignment = self.alignment_for_physical_axis(PhysicalAxis::Vertical);
        let direction = self.alignment_direction(PhysicalAxis::Vertical);
        let offset = abspos_static_alignment_offset(
            alignment,
            self.area.height(),
            border_box_height,
            margin_top,
            margin_bottom,
            direction,
        );
        // As on the horizontal axis, form the margin-box offset first. This
        // avoids losing a fractional page coordinate when a start margin
        // cancels the start-alignment border offset.
        let margin_box_offset = offset - margin_top;
        let candidate_margin_box_top = self.area.top_y() - margin_box_offset;
        let candidate_overflows_containing_block = candidate_margin_box_top
            > containing_block.top_y()
            || candidate_margin_box_top - outer_height
                < containing_block.top_y() - containing_block.height();
        if safe_alignment_falls_back_to_static_position_start(
            alignment,
            offset,
            self.area.height(),
            border_box_height,
            margin_top,
            margin_bottom,
            direction,
            candidate_overflows_containing_block,
        ) {
            let margin_box_top = if direction.container_reverse_start_end {
                self.area.top_y() - outer_height
            } else {
                self.area.top_y()
            };
            containing_block.top_y() - margin_box_top
        } else {
            containing_block.top_y() - candidate_margin_box_top
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AbsposAlignmentDirection {
    container_reverse_start_end: bool,
    subject_reverse_start_end: bool,
    left_right_are_physical: bool,
    left_right_resolve_to_inline_edges: bool,
}

/// Resolve one physical-axis offset within a static-position rectangle.
///
/// `normal`, `stretch`, and baseline values fall back to logical start while
/// establishing a static position. The actual abspos sizing algorithm decides
/// whether `normal` or `stretch` changes an automatic size.
fn abspos_static_alignment_offset(
    alignment: css::SelfAlignment,
    area_size: f32,
    border_size: f32,
    margin_start: f32,
    margin_end: f32,
    direction: AbsposAlignmentDirection,
) -> f32 {
    let free_space = area_size - margin_start - border_size - margin_end;
    let keyword = alignment.keyword;
    let start = margin_start;
    let end = margin_start + free_space;
    match keyword {
        SelfAlignmentKeyword::Center => margin_start + free_space / 2.0,
        SelfAlignmentKeyword::End | SelfAlignmentKeyword::FlexEnd => {
            if direction.container_reverse_start_end {
                start
            } else {
                end
            }
        }
        SelfAlignmentKeyword::SelfEnd => {
            if direction.subject_reverse_start_end {
                start
            } else {
                end
            }
        }
        // First-baseline alignment falls back to start while last-baseline
        // alignment falls back to end when deriving a static position.
        // <https://drafts.csswg.org/css-align-3/#baseline-align-content>
        SelfAlignmentKeyword::LastBaseline => {
            if direction.container_reverse_start_end {
                start
            } else {
                end
            }
        }
        SelfAlignmentKeyword::Start | SelfAlignmentKeyword::FlexStart => {
            if direction.container_reverse_start_end {
                end
            } else {
                start
            }
        }
        SelfAlignmentKeyword::SelfStart => {
            if direction.subject_reverse_start_end {
                end
            } else {
                start
            }
        }
        SelfAlignmentKeyword::Left if direction.left_right_are_physical => start,
        SelfAlignmentKeyword::Right if direction.left_right_are_physical => end,
        // In a vertical inline axis `left` and `right` resolve to the inline
        // start and end edges respectively. When the alignment axis is the
        // block axis, they instead fall back to `start`.
        // <https://drafts.csswg.org/css-align-3/#self-align-terms>
        SelfAlignmentKeyword::Left if direction.left_right_resolve_to_inline_edges => {
            if direction.container_reverse_start_end {
                end
            } else {
                start
            }
        }
        SelfAlignmentKeyword::Right if direction.left_right_resolve_to_inline_edges => {
            if direction.container_reverse_start_end {
                start
            } else {
                end
            }
        }
        SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Right
        | SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline => {
            if direction.container_reverse_start_end {
                end
            } else {
                start
            }
        }
    }
}

/// `safe` alignment is first evaluated in the static-position rectangle. If
/// that result would overflow the inset-modified containing block, it falls
/// back to the static-position rectangle's logical start edge. The real
/// containing block detects unsafe overflow, but it does not replace the
/// Flex/Grid-derived static alignment container. A value that already resolves
/// to the static rectangle's logical start is unchanged: substituting `start`
/// must not make `safe start` behave as a different alignment value.
/// <https://drafts.csswg.org/css-align-3/#overflow-values>
#[allow(clippy::too_many_arguments)]
fn safe_alignment_falls_back_to_static_position_start(
    alignment: css::SelfAlignment,
    offset: f32,
    area_size: f32,
    border_size: f32,
    margin_start: f32,
    margin_end: f32,
    direction: AbsposAlignmentDirection,
    candidate_overflows_containing_block: bool,
) -> bool {
    if alignment.safety != AlignmentSafety::Safe || !candidate_overflows_containing_block {
        return false;
    }

    let logical_start_offset = if direction.container_reverse_start_end {
        area_size - border_size - margin_end
    } else {
        margin_start
    };
    (offset - logical_start_offset).abs() > 0.01
}

impl AbsoluteStaticPosition {
    /// Store an absolutely positioned box's static-position rectangle in page
    /// coordinates so it can be resolved against either an absolute or fixed
    /// containing block later.
    ///
    /// CSS 2.2 defines auto insets from the hypothetical normal-flow static
    /// position, while CSS Positioned Layout makes fixed boxes use the viewport
    /// as their containing block:
    /// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width> and
    /// <https://www.w3.org/TR/css-position-3/#fixed-positioning>.
    pub(in crate::layout) fn from_page_rect(
        page_left_x: f32,
        page_right_x: f32,
        page_top_y: f32,
    ) -> Self {
        Self::from_page_rect_with_horizontal_outside(page_left_x, page_right_x, page_top_y, false)
    }

    pub(in crate::layout) fn from_page_rect_with_horizontal_outside(
        page_left_x: f32,
        page_right_x: f32,
        page_top_y: f32,
        horizontal_can_fall_outside: bool,
    ) -> Self {
        Self {
            page_left_x,
            page_right_x,
            page_top_y,
            horizontal_can_fall_outside,
            has_vertical_position: true,
            static_alignment_source: None,
        }
    }

    /// Store only the inline-axis part of a static-position rectangle.
    ///
    /// A blockified box encountered within an inline run retains that run's
    /// inline position, while its block position is still selected by the
    /// block formatting context's hypothetical in-flow box.
    /// <https://www.w3.org/TR/css-position-3/#static-position>
    pub(in crate::layout) fn from_page_horizontal_position(
        page_left_x: f32,
        page_right_x: f32,
    ) -> Self {
        Self {
            page_left_x,
            page_right_x,
            page_top_y: 0.0,
            horizontal_can_fall_outside: false,
            has_vertical_position: false,
            static_alignment_source: None,
        }
    }

    pub(in crate::layout) fn with_static_alignment(
        mut self,
        static_alignment: AbsposStaticAlignment,
    ) -> Self {
        self.static_alignment_source = Some(match self.static_alignment_source {
            Some(StaticAlignmentSource::OrdinaryFlow { rectangle, .. }) => {
                StaticAlignmentSource::OrdinaryFlow {
                    rectangle,
                    alignment: Some(static_alignment),
                }
            }
            Some(StaticAlignmentSource::FormattingContext(_)) | None => {
                StaticAlignmentSource::FormattingContext(static_alignment)
            }
        });
        self
    }

    pub(in crate::layout) fn with_static_position_rectangle(
        mut self,
        rectangle: StaticPositionRectangle,
    ) -> Self {
        self.static_alignment_source = Some(StaticAlignmentSource::OrdinaryFlow {
            rectangle,
            alignment: self
                .static_alignment_source
                .and_then(|source| match source {
                    StaticAlignmentSource::OrdinaryFlow { alignment, .. } => alignment,
                    StaticAlignmentSource::FormattingContext(_) => None,
                }),
        });
        self
    }

    /// Attach an inline-source static-position rectangle and replace the
    /// physical page-top fallback with its captured inline edge.
    ///
    /// Vertical inline layout resolves automatic physical `top`/`bottom`
    /// through the scalar fallback before the rectangle's alignment payload.
    /// Retaining an ancestor's earlier page-top value here would therefore
    /// discard the hypothetical inline placeholder edge that this rectangle
    /// just captured.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    pub(in crate::layout) fn with_inline_static_position_rectangle(
        mut self,
        rectangle: StaticPositionRectangle,
    ) -> Self {
        self.page_top_y = rectangle.area.top_y();
        self.has_vertical_position = true;
        self.with_static_position_rectangle(rectangle)
    }

    pub(in crate::layout) fn static_position_rectangle(self) -> Option<StaticPositionRectangle> {
        match self.static_alignment_source {
            Some(StaticAlignmentSource::OrdinaryFlow { rectangle, .. }) => Some(rectangle),
            Some(StaticAlignmentSource::FormattingContext(_)) | None => None,
        }
    }

    pub(in crate::layout) fn static_alignment(self) -> Option<AbsposStaticAlignment> {
        match self.static_alignment_source {
            Some(StaticAlignmentSource::OrdinaryFlow { alignment, .. }) => alignment,
            Some(StaticAlignmentSource::FormattingContext(alignment)) => Some(alignment),
            None => None,
        }
    }

    /// Whether a formatting context (currently Flexbox or Grid) supplied the
    /// static-position alignment rectangle. Such a rectangle has not already
    /// selected an ordinary-flow hypothetical margin-box edge, even when its
    /// effective alignment is `normal`.
    pub(in crate::layout) fn has_formatting_context_static_alignment(self) -> bool {
        matches!(
            self.static_alignment_source,
            Some(StaticAlignmentSource::FormattingContext(_))
        )
    }

    /// The degenerate rectangle of a block-level source in a horizontal
    /// formatting context. Its inline span and hypothetical block-start are
    /// captured while normal flow still knows the source fragment.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
    pub(in crate::layout) fn horizontal_block_static_rectangle(self) -> Option<PageTopRect> {
        self.has_vertical_position.then(|| {
            PageTopRect::new(
                self.page_left_x,
                self.page_top_y,
                (self.page_right_x - self.page_left_x).max(0.0),
                0.0,
            )
        })
    }

    pub(in crate::layout) fn horizontal_position(
        self,
        containing_block: ContainingBlock,
    ) -> PhysicalStaticAxisFallback {
        let left = self.page_left_x - containing_block.x();
        let right = containing_block.x() + containing_block.width() - self.page_right_x;
        if self.horizontal_can_fall_outside {
            PhysicalStaticAxisFallback::new_unclamped(left, right)
        } else {
            PhysicalStaticAxisFallback::new(left, right)
        }
    }

    pub(in crate::layout) fn vertical_start(self, containing_block: ContainingBlock) -> f32 {
        containing_block.top_y() - self.page_top_y
    }

    pub(in crate::layout) const fn has_vertical_position(self) -> bool {
        self.has_vertical_position
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineLineMetrics {
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) baseline_offset: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct HangingPunctuationWidths {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
}

/// A positioned inline line ready for painting.
///
/// CSS Inline Layout constructs a line box before painting its inline
/// fragments. This prepared line stores the resolved line metrics and ordered
/// paint items so text shaping, atom placement, backgrounds, links, and
/// decorations consume one reusable line artifact:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedInlineLine {
    pub(in crate::layout) metrics: InlineLineMetrics,
    pub(in crate::layout) paint_items: Vec<PreparedInlinePaintItem>,
    /// Origin-owned decorating-box fragment geometry selected for this line.
    /// It is intentionally separate from text receiver provenance, which is
    /// segmented for paint but cannot define CSS percentage bases.
    pub(in crate::layout) decoration_origin_fragments:
        Rc<[crate::layout::text_paint::TextDecorationOriginFragmentGeometry]>,
}

/// A positioned inline paint item inside a prepared line box.
///
/// CSS painting observes source/line-box order: inline fragment backgrounds
/// are painted for each line fragment, shaped text groups are emitted on the
/// same baseline, and atomic inline boxes paint as indivisible margin boxes:
/// <https://www.w3.org/TR/CSS22/zindex.html> and
/// <https://www.w3.org/TR/css-inline-3/#model>.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub(in crate::layout) enum PreparedInlinePaintItem {
    FragmentBackground(PreparedInlineFragment),
    TextGroup(PreparedInlineTextGroup),
    Atom(PreparedInlineAtom),
}

/// A positioned inline text fragment with its line-fragment geometry.
///
/// CSS Backgrounds and Borders paints inline backgrounds and borders per
/// generated line fragment, while CSS Text may shape adjacent fragments as one
/// typographic context. Keeping both the original fragment and its used
/// geometry lets background/decor/link painting stay fragment-specific:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedInlineFragment {
    pub(in crate::layout) fragment: InlineFragment,
    pub(in crate::layout) rect: PhysicalInlineRect,
}

/// A positioned atomic inline box with resolved content geometry.
///
/// CSS 2.2 treats inline-blocks, replaced elements, and similar atomic inline
/// boxes as a single inline-level box participating in the parent line box:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedInlineAtom {
    pub(in crate::layout) atom: InlineAtom,
    /// The resolved physical border box to paint after the atom's separate
    /// margin-box participation has selected its line position.
    pub(in crate::layout) border_box: PhysicalInlineRect,
}

/// A shaped group of adjacent inline text fragments.
///
/// CSS Text requires boundary shaping to preserve cursive and complex-script
/// context across eligible inline boxes. This group stores the exact Parley
/// shaped runs selected before painting, including resolved font ids and
/// glyph advances used later by PDF text emission:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>,
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>, and
/// ISO 32000-2:2020, 9.10.3 "ToUnicode CMaps".
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedInlineTextGroup {
    pub(in crate::layout) bounds: PhysicalInlineTextBounds,
    pub(in crate::layout) style: ComputedStyle,
    /// Lexical decoration receivers preserved alongside a shared shaped run.
    ///
    /// CSS Text allows shaping across transparent inline boundaries, but line
    /// decoration propagation stops at the lexical receivers that carry a
    /// given origin.  These segments therefore affect only decoration paint;
    /// `shaped` remains the single glyph program used for PDF text output.
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    pub(in crate::layout) decoration_provenance: Vec<PreparedTextDecorationProvenanceSegment>,
    /// Block-axis trim belonging to the inline scope that owns this text.
    /// The text style itself may not carry the value because `text-box-trim`
    /// is not inherited, while the inline scope's background and link still
    /// cover this descendant text:
    /// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
    pub(in crate::layout) text_box_trim: TextBoxLineTrim,
    /// The product of this text run's used opacity and each owning inline
    /// effect scope. Opacity is non-inherited, so its lexical owners are
    /// retained separately from the text style through inline collection.
    /// <https://www.w3.org/TR/css-color-4/#transparency>
    pub(in crate::layout) paint_opacity: f32,
    /// Lexical opacity scopes, from outermost to innermost, that own this
    /// prepared text.  This keeps paint-effect ancestry explicit after text
    /// collection, typographic splitting, bidi reordering, and shaping.
    pub(in crate::layout) paint_scope_ancestry: Rc<[InlinePaintScopeId]>,
    /// The innermost lexical inline scope that owns this text group's
    /// positioned stacking context.  The scope's style is retained because
    /// descendant text itself inherits typography, not `position` or
    /// `z-index`.
    pub(in crate::layout) positioned_paint_style: Option<ComputedStyle>,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) link_paint_rect: Option<PaintRect>,
    pub(in crate::layout) decoration_paint_rect: Option<PaintRect>,
    pub(in crate::layout) shaped: ShapedInlineLine,
    /// Plain-text replacement for a shaped group containing one or more
    /// layout-only `word-space-transform` separators.
    ///
    /// The group can retain one continuous glyph stream while PDF output
    /// restores authored U+200B characters and omits generated `<wbr>` text.
    /// <https://drafts.csswg.org/css-text-4/#word-space-transform>
    pub(in crate::layout) actual_text: Option<Rc<str>>,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) source_run: Rc<()>,
}

/// Consecutive receiver ranges that carry the same ordered decoration-origin
/// chain.  Equality of declarations is not enough: `Rc` identity distinguishes
/// nested equal-looking decorating boxes.
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedTextDecorationProvenanceSegment {
    pub(in crate::layout) layers: Vec<css::TextDecorationLayer>,
    pub(in crate::layout) receivers: Vec<PreparedTextDecorationReceiver>,
}

/// One source fragment's physical receiver span within a shaped text group.
#[derive(Debug, Clone)]
pub(in crate::layout) struct PreparedTextDecorationReceiver {
    pub(in crate::layout) inline_span: TextInlineSpan,
    pub(in crate::layout) style: ComputedStyle,
}

impl PreparedInlineTextGroup {
    pub(in crate::layout) fn x(&self) -> f32 {
        self.bounds.x()
    }

    pub(in crate::layout) fn y(&self) -> f32 {
        self.bounds.y()
    }

    pub(in crate::layout) fn width(&self) -> f32 {
        self.bounds.width()
    }

    pub(in crate::layout) fn set_x(&mut self, x: f32) {
        self.bounds.set_x(x);
    }

    pub(in crate::layout) fn set_y(&mut self, y: f32) {
        self.bounds.set_y(y);
    }

    pub(in crate::layout) fn set_width(&mut self, width: f32) {
        self.bounds.set_width(width);
    }

    pub(in crate::layout) fn link_paint_rect(&self) -> PaintRect {
        self.link_paint_rect
            .unwrap_or_else(|| self.bounds.link_paint_rect(self.style.font_size))
    }
}

/// Logical inline-axis geometry for one prepared line.
///
/// CSS Writing Modes defines inline layout in logical inline/block axes, while
/// PDF painting consumes physical coordinates. This geometry keeps CSS Text
/// alignment, indentation, and hanging punctuation in logical inline space
/// until each fragment, text group, or atomic inline box is converted to a
/// physical paint artifact:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-text-3/#text-align-property>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineLineGeometry {
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    /// Offset from the block content edge to this line's logical inline start.
    /// Tabs use the content edge as their periodic-stop origin, rather than
    /// the potentially indented line start.
    pub(in crate::layout) inline_start_offset: f32,
    pub(in crate::layout) inline_start: f32,
    pub(in crate::layout) inline_size: f32,
    pub(in crate::layout) block_start: f32,
    /// Block-axis trim applied to this selected line. The trim is stored on
    /// the line record rather than the paint group's style when an inline box
    /// is the source of the trim, so links and decorations can use the same
    /// content rectangle as the inline background.
    pub(in crate::layout) text_box_line_trim: TextBoxLineTrim,
}

/// Physical rectangle for an inline line-fragment paint item.
///
/// CSS Inline Layout first positions fragments in logical inline/block axes,
/// then CSS Writing Modes maps those fragments to physical coordinates. This
/// rectangle stores that resolved physical box in the current layout container
/// before it is projected into paint primitives:
/// <https://www.w3.org/TR/css-inline-3/#line-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PhysicalInlineRect {
    pub(in crate::layout) rect: InlineRect,
}

impl PhysicalInlineRect {
    pub(in crate::layout) fn new(rect: InlineRect) -> Self {
        Self {
            rect: InlineRect::new(
                rect.origin,
                InlineSize::new(rect.size.width.max(0.0), rect.size.height.max(0.0)),
            ),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(in crate::layout) fn translated(self, offset: InlineVisualOffset) -> Self {
        if offset.is_zero() {
            return self;
        }
        Self::new(InlineRect::new(
            InlinePoint::new(self.x() + offset.x(), self.y() + offset.y()),
            self.rect.size,
        ))
    }

    pub(in crate::layout) fn paint_rect(self) -> PaintRect {
        PaintRect::new(
            PaintPoint::new(self.x(), self.y()),
            PaintSize::new(self.width(), self.height()),
        )
    }

    pub(in crate::layout) fn paint_clip(self) -> PaintClip {
        PaintClip::from_paint_rect(self.paint_rect())
    }
}

/// Baseline-origin geometry for one prepared shaped inline text group.
///
/// CSS Inline Layout positions text by a baseline point and an inline advance,
/// not by a full border box. The baseline origin is stored in resolved physical
/// inline formatting coordinates so painting, decoration, and link annotation
/// code all project through one typed boundary:
/// <https://www.w3.org/TR/css-inline-3/#baseline-tables> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PhysicalInlineTextBounds {
    pub(in crate::layout) baseline_origin: InlinePoint,
    pub(in crate::layout) inline_size: f32,
}

#[cfg(test)]
mod child_available_space_tests {
    use super::*;

    #[test]
    fn orthogonal_fallback_is_not_a_percentage_basis() {
        let space = ChildAvailableSpace::new(
            WritingMode::HorizontalTb,
            PhysicalContentWidth::new(content_box_pt(300.0)),
            true,
            None,
            PhysicalContentHeight::new(content_box_pt(200.0)),
        );

        assert_eq!(space.available_physical_height().points(), 200.0);
        assert!(!space.physical_height_percentage_basis().is_definite());
        assert!(
            !space
                .logical_inline_percentage_basis_for(WritingMode::VerticalRl)
                .is_definite()
        );
        assert_eq!(
            space.logical_inline_percentage_basis_for(WritingMode::HorizontalTb),
            PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(300.0)))
        );
    }

    #[test]
    fn definite_perpendicular_size_projects_to_vertical_percentage_basis() {
        let space = ChildAvailableSpace::new(
            WritingMode::HorizontalTb,
            PhysicalContentWidth::new(content_box_pt(300.0)),
            true,
            Some(PhysicalContentHeight::new(content_box_pt(200.0))),
            PhysicalContentHeight::new(content_box_pt(500.0)),
        );

        assert_eq!(
            space.logical_inline_percentage_basis_for(WritingMode::VerticalLr),
            PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(200.0)))
        );
    }

    #[test]
    fn auto_vertical_block_width_is_not_a_horizontal_percentage_basis() {
        let space = ChildAvailableSpace::new(
            WritingMode::VerticalLr,
            PhysicalContentWidth::new(content_box_pt(0.0)),
            false,
            None,
            PhysicalContentHeight::new(content_box_pt(500.0)),
        );

        assert!(
            !space
                .logical_inline_percentage_basis_for(WritingMode::HorizontalTb)
                .is_definite()
        );
    }

    #[test]
    fn scrollport_availability_stays_indefinite_for_percentages() {
        let space = ChildAvailableSpace::new(
            WritingMode::HorizontalTb,
            PhysicalContentWidth::new(content_box_pt(300.0)),
            true,
            None,
            PhysicalContentHeight::new(content_box_pt(500.0)),
        )
        .with_orthogonal_available_height(
            OrthogonalAvailableHeight::nearest_scroll_container(PhysicalContentHeight::new(
                content_box_pt(120.0),
            )),
        );

        assert_eq!(space.available_physical_height().points(), 120.0);
        assert!(matches!(
            space.orthogonal_available_height,
            OrthogonalAvailableHeight::NearestScrollContainer(_)
        ));
        assert!(
            !space
                .logical_inline_percentage_basis_for(WritingMode::VerticalRl)
                .is_definite()
        );
    }

    #[test]
    fn abspos_static_alignment_centers_both_physical_axes() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(10.0, 80.0, 100.0, 60.0),
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::new(SelfAlignmentKeyword::Center),
            css::SelfAlignment::new(SelfAlignmentKeyword::Center),
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 80.0, 100.0, 60.0));

        let horizontal = alignment.horizontal_static_position(containing_block, 20.0, 0.0, 0.0);
        let vertical = alignment.vertical_static_start(containing_block, 20.0, 0.0, 0.0);

        assert_eq!(horizontal.left, 40.0);
        assert_eq!(horizontal.right, 40.0);
        assert_eq!(vertical, 20.0);
    }

    #[test]
    fn abspos_static_alignment_returns_horizontal_margin_box_insets() {
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 80.0, 100.0, 60.0));
        let start = AbsposStaticAlignment::new(
            containing_block.rect,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::new(SelfAlignmentKeyword::Start),
            css::SelfAlignment::NORMAL,
        )
        .horizontal_static_position(containing_block, 20.0, 7.0, 11.0);
        assert_eq!((start.left, start.right), (0.0, 62.0));

        let end = AbsposStaticAlignment::new(
            containing_block.rect,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::new(SelfAlignmentKeyword::End),
            css::SelfAlignment::NORMAL,
        )
        .horizontal_static_position(containing_block, 20.0, 7.0, 11.0);
        assert_eq!((end.left, end.right), (62.0, 0.0));
    }

    #[test]
    fn abspos_static_alignment_returns_vertical_margin_box_insets() {
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(10.0, 80.0, 100.0, 60.0));
        let start = AbsposStaticAlignment::new(
            containing_block.rect,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::new(SelfAlignmentKeyword::Start),
        )
        .vertical_static_start(containing_block, 20.0, 7.0, 11.0);
        assert_eq!(start, 0.0);

        let end = AbsposStaticAlignment::new(
            containing_block.rect,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::new(SelfAlignmentKeyword::End),
        )
        .vertical_static_start(containing_block, 20.0, 7.0, 11.0);
        assert_eq!(end, 22.0);
    }

    #[test]
    fn abspos_static_alignment_maps_vertical_writing_inline_axis() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(0.0, 100.0, 80.0, 120.0),
            WritingMode::VerticalLr,
            Direction::Ltr,
            WritingMode::VerticalLr,
            Direction::Ltr,
            css::SelfAlignment::new(SelfAlignmentKeyword::End),
            css::SelfAlignment::new(SelfAlignmentKeyword::Start),
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, 80.0, 120.0));

        // In vertical-lr, physical Y is the logical inline axis; the inline
        // `end` therefore positions a 20pt subject at 100pt in a 120pt area.
        assert_eq!(
            alignment.vertical_static_start(containing_block, 20.0, 0.0, 0.0),
            100.0
        );
    }

    #[test]
    fn vertical_rtl_inline_static_rectangle_aligns_the_subject_bottom_to_inline_start() {
        let alignment = AbsposStaticAlignment::new(
            // In page-top coordinates the selected line's logical
            // inline-start is its physical bottom edge.
            PageTopRect::new(0.0, 80.0, 20.0, 0.0),
            WritingMode::VerticalLr,
            Direction::Rtl,
            WritingMode::VerticalLr,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::NORMAL,
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 200.0, 80.0, 120.0));

        // `normal` resolves to the static containing block's inline-start.
        // The 20pt subject therefore extends upward from the 80pt inline
        // edge, leaving its physical top at 100pt: `start = 200 - 100`.
        assert_eq!(
            alignment.vertical_static_start(containing_block, 20.0, 0.0, 0.0),
            100.0
        );
    }

    #[test]
    fn abspos_self_start_uses_the_subject_direction() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(10.0, 80.0, 80.0, 0.0),
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Rtl,
            css::SelfAlignment::new(SelfAlignmentKeyword::SelfStart),
            css::SelfAlignment::NORMAL,
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, 100.0, 100.0));

        let position = alignment.horizontal_static_position(containing_block, 20.0, 0.0, 0.0);
        assert_eq!(position.left, 70.0);
        assert_eq!(position.right, 10.0);
    }

    #[test]
    fn degenerate_inline_static_rectangle_still_centers_on_the_line() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(40.0, 80.0, 0.0, 25.0),
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::new(SelfAlignmentKeyword::Center),
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, 100.0, 100.0));

        assert_eq!(
            alignment.vertical_static_start(containing_block, 50.0, 0.0, 0.0),
            7.5
        );
    }

    #[test]
    fn inline_static_rectangle_end_alignment_uses_its_line_under_edge() {
        let alignment = AbsposStaticAlignment::new(
            // An inline source has a degenerate inline axis but spans the
            // selected line in the physical block axis.
            PageTopRect::new(40.0, 80.0, 0.0, 37.5),
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::new(SelfAlignmentKeyword::End),
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 80.0, 100.0, 37.5));

        // The aligned margin box starts at the line-under edge minus its
        // own size.  This value is consumed directly by the static inset
        // equation; no text-baseline replay may move it afterward.
        assert_eq!(
            alignment.vertical_static_start(containing_block, 15.0, 0.0, 0.0),
            22.5
        );
    }

    #[test]
    fn static_available_size_projects_the_selected_vertical_edge() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(10.0, 80.0, 0.0, 0.0),
            WritingMode::HorizontalTb,
            Direction::Ltr,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            css::SelfAlignment::NORMAL,
            css::SelfAlignment::new(SelfAlignmentKeyword::End),
        );
        let containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, 100.0, 100.0));

        // A block-end-aligned zero-height static rectangle leaves the span
        // from that selected edge to the containing block's opposite edge.
        assert_eq!(
            alignment.available_outer_size(PhysicalAxis::Vertical, containing_block),
            Some(20.0)
        );
    }

    #[test]
    fn escaped_atom_positioning_keeps_outer_cb_and_static_axes_separate() {
        let actual_containing_block =
            ContainingBlock::from_page_top_rect(PageTopRect::new(48.0, 720.0, 500.0, 700.0))
                .on_page(3);
        let static_position = AbsoluteStaticPosition::from_page_rect(0.0, 120.0, 9_960.0);
        let context = EscapedAtomPositioningContext {
            actual_containing_block,
            static_position,
        };

        assert_eq!(context.actual_containing_block, actual_containing_block);
        assert_eq!(context.actual_containing_block.origin_page_index, Some(3));
        assert_eq!(context.static_position.page_left_x, 0.0);
        assert_eq!(context.static_position.page_right_x, 120.0);
        assert_eq!(context.static_position.page_top_y, 9_960.0);

        let auto_axes = EscapedAtomTranslation::from_positioned_static_axes(
            context.actual_containing_block,
            true,
            true,
            true,
        );
        assert_eq!(auto_axes.escape_offset(-9_940.0).x, -48.0);
        assert_eq!(auto_axes.escape_offset(-9_940.0).y, -9_940.0);
        assert_eq!(auto_axes.atom_offset(160.0, 640.0).x, 160.0);
        assert_eq!(auto_axes.atom_offset(160.0, 640.0).y, 640.0);

        let explicit_axes = EscapedAtomTranslation::from_positioned_static_axes(
            context.actual_containing_block,
            false,
            false,
            false,
        );
        assert_eq!(explicit_axes.atom_offset(160.0, 640.0).x, 0.0);
        assert_eq!(explicit_axes.atom_offset(160.0, 640.0).y, 0.0);
        assert_eq!(auto_axes.replay_page_index(7, 3), 7);
        assert_eq!(explicit_axes.replay_page_index(7, 3), 3);
    }
}

#[cfg(test)]
mod inline_paint_scope_tests {
    use super::*;

    fn scope_decoration(scope_id: InlinePaintScopeId) -> InlineAncestorDecoration {
        let mut style = ComputedStyle::initial();
        style.opacity = css::Opacity::new_clamped(0.5).expect("finite opacity");
        InlineAncestorDecoration {
            style,
            hanging_edges: InlineHangingEdges::default(),
            paints_background_or_border: false,
            positioning_containing_block_id: None,
            paint_effect_scope_id: Some(scope_id),
        }
    }

    #[test]
    fn equal_opacity_siblings_remain_distinct_paint_scopes() {
        let left = scope_decoration(InlinePaintScopeId::allocate());
        let right = scope_decoration(InlinePaintScopeId::allocate());
        assert!(!inline_ancestor_decorations_have_same_text_paint_effect(
            &[left],
            &[right]
        ));
    }

    #[test]
    fn copied_scope_identity_survives_typographic_splitting() {
        let scope = InlinePaintScopeId::allocate();
        let decoration = scope_decoration(scope);
        let equivalent_decoration = decoration.clone();
        assert!(inline_ancestor_decorations_have_same_text_paint_effect(
            std::slice::from_ref(&decoration),
            std::slice::from_ref(&equivalent_decoration)
        ));
    }
}

#[cfg(test)]
mod inline_tracking_scope_tests {
    use super::*;

    fn scope_style() -> ComputedStyle {
        ComputedStyle::initial()
    }

    #[test]
    fn lowest_common_returns_an_identical_scope() {
        let scope = InlineTrackingScope::root(&scope_style());

        assert!(std::ptr::eq(
            InlineTrackingScope::lowest_common(&scope, &scope),
            scope.as_ref()
        ));
    }

    #[test]
    fn lowest_common_returns_the_ancestor_of_a_descendant() {
        let root = InlineTrackingScope::root(&scope_style());
        let child = InlineTrackingScope::child(Rc::clone(&root), &scope_style());
        let descendant = InlineTrackingScope::child(Rc::clone(&child), &scope_style());

        assert!(std::ptr::eq(
            InlineTrackingScope::lowest_common(&descendant, &child),
            child.as_ref()
        ));
    }

    #[test]
    fn lowest_common_returns_the_parent_of_sibling_scopes() {
        let root = InlineTrackingScope::root(&scope_style());
        let left = InlineTrackingScope::child(Rc::clone(&root), &scope_style());
        let right = InlineTrackingScope::child(Rc::clone(&root), &scope_style());

        assert!(std::ptr::eq(
            InlineTrackingScope::lowest_common(&left, &right),
            root.as_ref()
        ));
    }

    #[test]
    fn common_autospace_style_uses_the_innermost_shared_inline_scope() {
        let mut root_style = scope_style();
        root_style.font_size = 17.0;
        let mut child_style = root_style.clone();
        child_style.font_size = 43.0;
        let root = InlineTrackingScope::root(&root_style);
        let left = InlineTrackingScope::child(Rc::clone(&root), &child_style);
        let right = InlineTrackingScope::child(Rc::clone(&root), &child_style);

        assert_eq!(
            InlineTrackingScope::common_autospace_style(&left, &right).font_size,
            17.0
        );
        assert_eq!(
            InlineTrackingScope::common_autospace_style(&left, &left).font_size,
            43.0
        );
    }

    #[test]
    fn lowest_common_aligns_differently_nested_cousins() {
        let root = InlineTrackingScope::root(&scope_style());
        let left_branch = InlineTrackingScope::child(Rc::clone(&root), &scope_style());
        let left = InlineTrackingScope::child(left_branch, &scope_style());
        let right_branch = InlineTrackingScope::child(Rc::clone(&root), &scope_style());
        let right = InlineTrackingScope::child(
            InlineTrackingScope::child(right_branch, &scope_style()),
            &scope_style(),
        );

        assert!(std::ptr::eq(
            InlineTrackingScope::lowest_common(&left, &right),
            root.as_ref()
        ));
    }

    #[test]
    fn descendant_scope_retains_its_local_used_letter_spacing() {
        let mut tracked = scope_style();
        tracked.letter_spacing = crate::css::ComputedLengthPercentage::from_points(4.0);
        let root = InlineTrackingScope::root(&tracked);
        let descendant = InlineTrackingScope::child(root, &scope_style());

        assert_eq!(descendant.letter_spacing().points(), 0.0);
    }
}
