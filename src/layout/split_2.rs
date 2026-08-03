use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::image_store::ImageId;

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

    pub(in crate::layout) fn available_physical_height(self) -> PhysicalContentHeight {
        self.physical_content_height
            .value()
            .or(self
                .direct_orthogonal_available_height
                .map(DirectOrthogonalAvailableHeight::value))
            .unwrap_or_else(|| self.orthogonal_available_height.value())
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
            LogicalInlineContentSize::new(self.available_physical_height().content_box_length())
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
}

impl OverflowClip {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self {
            rect,
            clips_x: true,
            clips_y: true,
        }
    }

    /// Construct an overflow clip that constrains only the requested physical
    /// axes. `overflow: clip visible` must not acquire rounded or rectangular
    /// clipping on its visible axis.
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>
    pub(in crate::layout) fn from_paint_rect_with_axes(
        rect: PaintRect,
        clips_x: bool,
        clips_y: bool,
    ) -> Self {
        Self {
            rect,
            clips_x,
            clips_y,
        }
    }

    pub(in crate::layout) fn with_axes(mut self, clips_x: bool, clips_y: bool) -> Self {
        self.clips_x = clips_x;
        self.clips_y = clips_y;
        self
    }

    pub(in crate::layout) fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self::from_paint_rect(rect.paint_rect())
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.size.height
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
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DecodedPngImage {
    pub(in crate::layout) image_id: Option<ImageId>,
    pub(in crate::layout) pixel_width: u32,
    pub(in crate::layout) pixel_height: u32,
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
            pixel_width,
            pixel_height,
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

    pub(in crate::layout) fn pixel_size(&self) -> RasterPixelSize {
        RasterPixelSize::new(self.pixel_width, self.pixel_height)
    }

    pub(in crate::layout) fn natural_layout_size(&self) -> crate::units::LayoutSize {
        raster_natural_layout_size(self.pixel_size())
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
    /// Source element for layers built by the ordinary positioned-box path.
    /// Re-entering that path replaces stale provisional layout for the same
    /// element and final page.
    pub(in crate::layout) source_element: Option<crate::dom::ElementId>,
    /// The originating box's computed style is retained with the final
    /// page-space stacking context so enclosing scroll containers can form
    /// snap areas only after positioned remapping is complete.
    pub(in crate::layout) source_style: ComputedStyle,
    pub(in crate::layout) source_is_target: bool,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context: PaintStackingContext,
    pub(in crate::layout) links: Vec<RenderedLink>,
    pub(in crate::layout) escaped_atom_translation: EscapedAtomTranslation,
}

impl PositionedPaintLayer {
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
            ) || style.has_transform(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_non_positioned_effect(
        element: &Element,
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        let mut effects = assets::paint_effects_for_element_box(element, style, bounds);
        // Block/flex/grid layout owns the exact used padding-box clip. The
        // dispatcher only sees descendant ink bounds, which are not a valid
        // overflow clip rectangle.
        effects.overflow_clip = None;
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
        Self::for_non_positioned_effect_with_effects(style, effects, true)
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
            establishes_containing_block: style.has_transform(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_atomic(
        style: &ComputedStyle,
        parent_band: PaintBand,
        bounds: PaintClip,
    ) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
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
            parent_band: if in_flow_positioned {
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
            establishes_containing_block: in_flow_positioned || style.has_transform(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_flex_item(style: &ComputedStyle, bounds: PaintClip) -> Self {
        let stack_level = StackLevel::from_optional_z_index(style.z_index.stack_level());
        // The grid/flex item's independent formatting context owns the used
        // padding-box clip, and its replayed paint is captured as one atomic
        // item context. Keep that clip on the item policy so descendants
        // whose geometry was intentionally retained for positioning and
        // compositing cannot escape when the context is serialized.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        let effects = assets::paint_effects_for_box(style, bounds);
        let is_real_stacking_context = style.z_index.establishes_stacking_context()
            || style_creates_effect_stacking_context(style, &effects);
        Self {
            // A static flex/grid item with `z-index: auto` is an atomic scope
            // for its in-flow contents, but it is not a stacking context.
            // Its own synthetic scope therefore remains in the in-flow phase;
            // only positioned descendants escape to the ancestor's auto/zero
            // stacking level.  Otherwise an earlier auto-level descendant can
            // be emitted before this item's background and be covered by it.
            // <https://www.w3.org/TR/CSS22/zindex.html#painting-order>
            parent_band: if is_real_stacking_context {
                stack_level.paint_band()
            } else {
                PaintBand::InFlowBlock
            },
            stack_level,
            context_kind: if is_real_stacking_context {
                StackingContextKind::Real
            } else {
                StackingContextKind::None
            },
            child_layer_policy: if is_real_stacking_context {
                ChildLayerPolicy::CaptureAll
            } else {
                ChildLayerPolicy::EscapeAll
            },
            is_real_stacking_context,
            is_fake_context: false,
            creates_compositing_group: effects.needs_group(),
            establishes_containing_block: style.has_transform(),
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
    style_creates_effect_stacking_context_with_containment(style, effects, true)
}

fn style_creates_effect_stacking_context_with_containment(
    style: &ComputedStyle,
    effects: &PaintEffects,
    containment_applies: bool,
) -> bool {
    effects.opacity < 1.0
        || effects.transform.is_some()
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
        // Layout containment establishes an independent formatting context and
        // containing blocks, but it does not establish a paint stacking
        // context. Paint containment adds that isolation.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        || (containment_applies && style.contain.paint)
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
    pub(in crate::layout) content_inline_span: PageInlineSpan,
    pub(in crate::layout) formatted_line_block_start: PageTopBlockPosition,
    pub(in crate::layout) alphabetic_baseline: PageTopBlockPosition,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct PendingOutsideMarkerAnchor {
    pub(in crate::layout) marker: ListMarker,
    pub(in crate::layout) list_item_style: ComputedStyle,
    pub(in crate::layout) content_inline_span: PageInlineSpan,
    /// Principal block start retained for an item that never produces an
    /// eligible in-flow line. Fragmented descendants may advance the layout
    /// cursor before the pending marker is finalized, but they must not move
    /// this CSS Lists fallback anchor.
    pub(in crate::layout) fallback_line_block_start: PageTopBlockPosition,
    pub(in crate::layout) painted: bool,
}

impl ListMarker {
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
/// CSS Text assigns both tracking and cross-element wrapping to the innermost
/// inline box containing both typographic units. This immutable parent chain
/// survives collection, source slicing, and bidi reordering without making
/// layout consumers reconstruct scope from decoration atoms:
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property> and
/// <https://drafts.csswg.org/css-text-3/#line-break-details>.
#[derive(Debug)]
pub(in crate::layout) struct InlineTrackingScope {
    parent: Option<Rc<Self>>,
    depth: usize,
    letter_spacing: LayoutLength,
    boundary_policy: InlineBoundaryPolicy,
}

impl InlineTrackingScope {
    pub(in crate::layout) fn root(style: &ComputedStyle) -> Rc<Self> {
        Self::root_with_boundary_policy(style, InlineBoundaryPolicy::from_style(style))
    }

    /// Construct the implicit root lexical scope with tracking inherited from
    /// `style` while retaining the paragraph's already-established boundary
    /// policy. Anonymous inline formatting contexts can first expose their
    /// tracking style on a descendant text item, but that must not transfer
    /// `white-space` ownership away from the paragraph that contains the
    /// cross-element boundary.
    pub(in crate::layout) fn root_with_boundary_policy(
        style: &ComputedStyle,
        boundary_policy: InlineBoundaryPolicy,
    ) -> Rc<Self> {
        Rc::new(Self {
            parent: None,
            depth: 0,
            letter_spacing: style.used_letter_spacing(),
            boundary_policy,
        })
    }

    pub(in crate::layout) fn child(parent: Rc<Self>, style: &ComputedStyle) -> Rc<Self> {
        Rc::new(Self {
            depth: parent.depth + 1,
            parent: Some(parent),
            letter_spacing: style.used_letter_spacing(),
            boundary_policy: InlineBoundaryPolicy::from_style(style),
        })
    }

    pub(in crate::layout) fn letter_spacing(&self) -> LayoutLength {
        self.letter_spacing
    }

    pub(in crate::layout) fn boundary_policy(&self) -> InlineBoundaryPolicy {
        self.boundary_policy
    }

    /// Return the nearest shared lexical ancestor without allocating a
    /// temporary ancestor list.
    ///
    /// All scopes for one inline formatting context share a root.  The depth
    /// recorded at construction lets this walk align both immutable parent
    /// chains, then advance them together until their allocation identities
    /// match.
    pub(in crate::layout) fn lowest_common<'left>(left: &'left Self, right: &Self) -> &'left Self {
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
    LeadingPreservedWhitespace,
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
    /// True when this fragment is a selected source-range slice whose glyph
    /// forms were retained from its unbroken shaping run.
    pub(in crate::layout) preserves_source_shaping: bool,
    /// UAX #9 direction resolved for a selected visual source slice.
    ///
    /// Logical source fragments leave this unset. It is only carried after
    /// mixed-inline visual reordering so the final paint shaping pass can
    /// preserve the already-selected order and glyph mirroring level.
    pub(in crate::layout) resolved_bidi_direction: Option<ResolvedBidiDirection>,
    pub(in crate::layout) ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
    /// The source inline ancestry used to resolve visual tracking boundaries.
    pub(in crate::layout) tracking_scope: Option<Rc<InlineTrackingScope>>,
    /// A paintless advance before this visual item.  It is set only after UBA
    /// reordering by the CSS Text tracking resolver.
    pub(in crate::layout) leading_tracking: LayoutLength,
    /// Whether this fragment's shaper-terminal tracking has already been
    /// transferred into a visual-boundary advance.
    pub(in crate::layout) terminal_tracking_normalized: bool,
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
    /// A visual inline advance retained after an initial-letter exclusion has
    /// removed this pseudo fragment from normal line advancement.
    pub(in crate::layout) out_of_flow_paint_inline_advance: Option<LayoutLength>,
    /// A visual block extent retained with an out-of-flow first-letter prefix
    /// without allowing that prefix to enlarge its ordinary line box.
    pub(in crate::layout) out_of_flow_paint_block_size: Option<LayoutLength>,
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
                preserves_source_shaping: false,
                resolved_bidi_direction: None,
                ancestor_inline_decorations,
                tracking_scope: None,
                leading_tracking: layout_pt(0.0),
                terminal_tracking_normalized: false,
                starts_visual_fragment: false,
                selected_discretionary_marker: false,
                first_letter_pseudo_role: FirstLetterPseudoFragmentRole::Ordinary,
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

    pub(in crate::layout) fn leading_tracking(&self) -> LayoutLength {
        self.data.leading_tracking
    }

    pub(in crate::layout) fn set_leading_tracking(&mut self, advance: LayoutLength) {
        Rc::make_mut(&mut self.data).leading_tracking = advance;
    }

    pub(in crate::layout) fn terminal_tracking_normalized(&self) -> bool {
        self.data.terminal_tracking_normalized
    }

    pub(in crate::layout) fn mark_terminal_tracking_normalized(&mut self) {
        Rc::make_mut(&mut self.data).terminal_tracking_normalized = true;
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
        data.text = text.into();
        data.terminal_tracking_normalized = false;
    }

    pub(in crate::layout) fn set_mergeable(&mut self, mergeable: bool) {
        Rc::make_mut(&mut self.data).mergeable = mergeable;
    }

    pub(in crate::layout) fn set_preserves_source_shaping(&mut self, value: bool) {
        Rc::make_mut(&mut self.data).preserves_source_shaping = value;
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
    fn generated_leader(&self) -> bool;
    /// A shaped selected-line artifact that can be reused without losing
    /// source-run contextual shaping at a soft wrap.
    fn selected_shaped(&self) -> Option<&ShapedInlineLine> {
        None
    }
    fn preserves_source_shaping(&self) -> bool;
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

    fn generated_leader(&self) -> bool {
        self.generated_leader()
    }
    fn preserves_source_shaping(&self) -> bool {
        self.data.preserves_source_shaping
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
    shaped: Option<&'a ShapedInlineLine>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
}

impl<'a> PendingInlineFragment<'a> {
    pub(in crate::layout) fn new(
        fragment: &'a InlineFragment,
        shaped: Option<&'a ShapedInlineLine>,
    ) -> Self {
        Self {
            fragment,
            shaped,
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

    fn generated_leader(&self) -> bool {
        self.fragment.generated_leader()
    }

    fn selected_shaped(&self) -> Option<&ShapedInlineLine> {
        self.shaped
    }

    fn preserves_source_shaping(&self) -> bool {
        self.fragment.data.preserves_source_shaping
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

impl InlineTextSource {
    pub(in crate::layout) fn is_generated(self) -> bool {
        matches!(self, Self::Generated | Self::FootnoteCall(_))
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
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct DefinitionListColumnItem<'a> {
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) signature: ElementSignature,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) children: Option<&'a [box_tree::FormattingBox<'a>]>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineStaticPosition {
    pub(in crate::layout) start_x: f32,
    pub(in crate::layout) end_x: f32,
    pub(in crate::layout) top_y: f32,
    pub(in crate::layout) baseline_y: f32,
    pub(in crate::layout) use_margin_box_top: bool,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct StaticHorizontalPosition {
    pub(in crate::layout) left: f32,
    pub(in crate::layout) right: f32,
    pub(in crate::layout) can_fall_outside: bool,
}

impl StaticHorizontalPosition {
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
    static_alignment: Option<AbsposStaticAlignment>,
}

/// The physical content rectangle a block formatting context contributes to
/// descendants' block-layout static-position rectangles.
///
/// Anonymous inline wrappers inherit flow axes but have no principal box
/// geometry of their own. Keeping this context separately lets an out-of-flow
/// child recover the actual parent's vertical inline span.
/// <https://www.w3.org/TR/css-position-3/#static-position>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BlockStaticPositionContext {
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    pub(in crate::layout) content_top_y: f32,
    pub(in crate::layout) content_height: f32,
    /// Whether a blockified static child can grow the parent's physical block
    /// axis while constructing the hypothetical layout.
    pub(in crate::layout) physical_block_size_is_auto: bool,
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
    pub(in crate::layout) area: PageTopRect,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
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
        inline: css::SelfAlignment,
        block: css::SelfAlignment,
    ) -> Self {
        Self {
            area,
            writing_mode,
            direction,
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
        let axes = WritingModeAxes::new(self.writing_mode, self.direction);
        let logical_axis = axes.logical_axis_for_physical(axis);
        let reverse_start_end = matches!(
            axes.physical_start_side(logical_axis),
            PhysicalSide::Right | PhysicalSide::Bottom
        );
        AbsposAlignmentDirection {
            reverse_start_end,
            left_right_are_physical: axis == PhysicalAxis::Horizontal
                && logical_axis == LogicalAxis::Inline,
        }
    }

    pub(in crate::layout) fn available_horizontal_outer_size(
        self,
        containing_block: ContainingBlock,
    ) -> Option<f32> {
        (self
            .alignment_for_physical_axis(PhysicalAxis::Horizontal)
            .keyword
            == SelfAlignmentKeyword::Center)
            .then(|| {
                let center = self.area.x() + self.area.width() / 2.0;
                (2.0 * (center - containing_block.x())
                    .min(containing_block.x() + containing_block.width() - center))
                .max(0.0)
            })
    }

    pub(in crate::layout) fn horizontal_static_position(
        self,
        containing_block: ContainingBlock,
        border_box_width: f32,
        margin_left: f32,
        margin_right: f32,
    ) -> StaticHorizontalPosition {
        let outer_width = margin_left + border_box_width + margin_right;
        let start = self.area.x()
            + abspos_static_alignment_offset(
                self.alignment_for_physical_axis(PhysicalAxis::Horizontal),
                self.area.width(),
                border_box_width,
                margin_left,
                margin_right,
                self.alignment_direction(PhysicalAxis::Horizontal),
            );
        let start_in_containing_block = start - containing_block.x();
        StaticHorizontalPosition::new_unclamped(
            start_in_containing_block,
            containing_block.width() - start_in_containing_block - outer_width,
        )
    }

    pub(in crate::layout) fn vertical_static_start(
        self,
        containing_block: ContainingBlock,
        border_box_height: f32,
        margin_top: f32,
        margin_bottom: f32,
    ) -> f32 {
        containing_block.top_y() - self.area.top_y()
            + abspos_static_alignment_offset(
                self.alignment_for_physical_axis(PhysicalAxis::Vertical),
                self.area.height(),
                border_box_height,
                margin_top,
                margin_bottom,
                self.alignment_direction(PhysicalAxis::Vertical),
            )
    }
}

#[derive(Debug, Clone, Copy)]
struct AbsposAlignmentDirection {
    reverse_start_end: bool,
    left_right_are_physical: bool,
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
    let keyword = if alignment.safety == AlignmentSafety::Safe && free_space < 0.0 {
        SelfAlignmentKeyword::Start
    } else {
        alignment.keyword
    };
    let start = margin_start;
    let end = margin_start + free_space;
    match keyword {
        SelfAlignmentKeyword::Center => margin_start + free_space / 2.0,
        SelfAlignmentKeyword::End
        | SelfAlignmentKeyword::SelfEnd
        | SelfAlignmentKeyword::FlexEnd => {
            if direction.reverse_start_end {
                start
            } else {
                end
            }
        }
        SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::SelfStart
        | SelfAlignmentKeyword::FlexStart => {
            if direction.reverse_start_end {
                end
            } else {
                start
            }
        }
        SelfAlignmentKeyword::Left if direction.left_right_are_physical => start,
        SelfAlignmentKeyword::Right if direction.left_right_are_physical => end,
        SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Right
        | SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline
        | SelfAlignmentKeyword::LastBaseline => {
            if direction.reverse_start_end {
                end
            } else {
                start
            }
        }
    }
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
            static_alignment: None,
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
            static_alignment: None,
        }
    }

    pub(in crate::layout) fn with_static_alignment(
        mut self,
        static_alignment: AbsposStaticAlignment,
    ) -> Self {
        self.static_alignment = Some(static_alignment);
        self
    }

    /// Attach formatting-context alignment only when it alters the static
    /// rectangle. A `normal` Grid alignment leaves the CSS Positioned Layout
    /// automatic-inset equations to consume that rectangle directly.
    pub(in crate::layout) fn with_static_alignment_if(
        self,
        static_alignment: AbsposStaticAlignment,
        applies: bool,
    ) -> Self {
        applies
            .then_some(static_alignment)
            .map_or(self, |alignment| self.with_static_alignment(alignment))
    }

    pub(in crate::layout) fn static_alignment(self) -> Option<AbsposStaticAlignment> {
        self.static_alignment
    }

    pub(in crate::layout) fn horizontal_position(
        self,
        containing_block: ContainingBlock,
    ) -> StaticHorizontalPosition {
        let left = self.page_left_x - containing_block.x();
        let right = containing_block.x() + containing_block.width() - self.page_right_x;
        if self.horizontal_can_fall_outside {
            StaticHorizontalPosition::new_unclamped(left, right)
        } else {
            StaticHorizontalPosition::new(left, right)
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
    pub(in crate::layout) content_rect: PhysicalInlineRect,
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
    /// The product of this text run's used opacity and each owning inline
    /// effect scope. Opacity is non-inherited, so its lexical owners are
    /// retained separately from the text style through inline collection.
    /// <https://www.w3.org/TR/css-color-4/#transparency>
    pub(in crate::layout) paint_opacity: f32,
    /// Lexical opacity scopes, from outermost to innermost, that own this
    /// prepared text.  This keeps paint-effect ancestry explicit after text
    /// collection, typographic splitting, bidi reordering, and shaping.
    pub(in crate::layout) paint_scope_ancestry: Rc<[InlinePaintScopeId]>,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) link_paint_rect: Option<PaintRect>,
    pub(in crate::layout) decoration_paint_rect: Option<PaintRect>,
    pub(in crate::layout) shaped: ShapedInlineLine,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) source_run: Rc<()>,
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
    fn abspos_static_alignment_maps_vertical_writing_inline_axis() {
        let alignment = AbsposStaticAlignment::new(
            PageTopRect::new(0.0, 100.0, 80.0, 120.0),
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
        style.opacity = 0.5;
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
}
