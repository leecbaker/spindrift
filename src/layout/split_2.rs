use std::rc::Rc;

use super::*;

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
    pub(in crate::layout) border_box_width: f32,
    pub(in crate::layout) border_box_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatAvoidingBfcPlacement {
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
}

impl ContainingBlock {
    pub(in crate::layout) fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self { rect }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.x
    }

    pub(in crate::layout) fn top_y(self) -> f32 {
        self.rect.top_y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.height
    }
}

/// Physical content-box size a formatting context exports to its children.
///
/// CSS Writing Modes defines the available inline size of an orthogonal flow
/// from the containing block's perpendicular physical axis, with fallback
/// constraints when that axis is indefinite:
/// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ChildAvailableSpace {
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) physical_content_width: f32,
    pub(in crate::layout) physical_content_height: Option<f32>,
    pub(in crate::layout) fallback_physical_content_height: f32,
}

impl ChildAvailableSpace {
    pub(in crate::layout) fn new(
        writing_mode: WritingMode,
        physical_content_width: f32,
        physical_content_height: Option<f32>,
        fallback_physical_content_height: f32,
    ) -> Self {
        Self {
            writing_mode,
            physical_content_width: physical_content_width.max(0.0),
            physical_content_height: physical_content_height.map(|height| height.max(0.0)),
            fallback_physical_content_height: fallback_physical_content_height.max(0.0),
        }
    }

    pub(in crate::layout) fn available_physical_height(self) -> f32 {
        self.physical_content_height
            .unwrap_or(self.fallback_physical_content_height)
    }

    pub(in crate::layout) fn logical_inline_size_for(self, writing_mode: WritingMode) -> f32 {
        match writing_mode {
            WritingMode::HorizontalTb => self.physical_content_width,
            WritingMode::VerticalRl | WritingMode::VerticalLr => self.available_physical_height(),
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
    pub(in crate::layout) line_mode: OverflowClipLineMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum OverflowClipLineMode {
    Intersect,
    Contain,
}

impl OverflowClip {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self {
            rect,
            line_mode: OverflowClipLineMode::Intersect,
        }
    }

    pub(in crate::layout) fn with_line_mode(mut self, line_mode: OverflowClipLineMode) -> Self {
        self.line_mode = line_mode;
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
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DecodedPngImage {
    pub(in crate::layout) pixel_width: u32,
    pub(in crate::layout) pixel_height: u32,
    pub(in crate::layout) rgb: Vec<u8>,
    pub(in crate::layout) alpha: Option<Vec<u8>>,
}

impl DecodedPngImage {
    pub(in crate::layout) fn pixel_size(&self) -> RasterPixelSize {
        RasterPixelSize::new(self.pixel_width, self.pixel_height)
    }

    pub(in crate::layout) fn natural_layout_size(&self) -> crate::units::LayoutSize {
        raster_natural_layout_size(self.pixel_size())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct ListState {
    pub(in crate::layout) step: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CounterSet {
    pub(in crate::layout) values: HashMap<String, Vec<i32>>,
    pub(in crate::layout) frames: Vec<CounterFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct CounterFrame {
    pub(in crate::layout) base_lengths: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct CounterScopeState {
    pub(in crate::layout) temporary_counters: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GeneratedPseudoCounterMode {
    Commit,
    Rollback,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct PositionedPaintLayer {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context: PaintStackingContext,
    pub(in crate::layout) links: Vec<RenderedLink>,
    pub(in crate::layout) escaped_atom_translation: EscapedAtomTranslation,
}

impl PositionedPaintLayer {
    pub(in crate::layout) fn translated(mut self, offset: PaintVector) -> Self {
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

    pub(in crate::layout) fn escape_offset(self, atom_local_y_offset: f32) -> PaintVector {
        PaintVector::new(
            self.normalize_x,
            if self.translate_y_with_atom {
                atom_local_y_offset
            } else {
                0.0
            },
        )
    }

    pub(in crate::layout) fn atom_offset(self, atom_x: f32, atom_y: f32) -> PaintVector {
        PaintVector::new(
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
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FixedPaintLayer {
    pub(in crate::layout) stack_level: StackLevel,
    pub(in crate::layout) context: PaintStackingContext,
    pub(in crate::layout) links: Vec<RenderedLink>,
}

/// Internal CSS stacking-context decision for one laid-out box fragment.
///
/// CSS Positioned Layout and CSS 2.2 Appendix E decide paint placement from
/// stack level, while CSS Transforms, CSS Color opacity, and CSS Overflow add
/// group effects. Keeping this classification in one value prevents layout
/// paths from independently deciding which positioned descendants are captured:
/// <https://www.w3.org/TR/css-position-3/#painting-order>,
/// <https://www.w3.org/TR/CSS22/zindex.html>,
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
#[derive(Debug, Clone, Copy, PartialEq)]
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
        let is_real_stacking_context = matches!(style.position, Position::Fixed | Position::Sticky)
            || style.z_index.is_some()
            || style_creates_effect_stacking_context(style, effects);
        let is_fake_context = !is_real_stacking_context
            && matches!(style.position, Position::Relative | Position::Absolute);
        Self {
            parent_band: StackLevel::from_optional_z_index(style.z_index).paint_band(),
            stack_level: StackLevel::from_optional_z_index(style.z_index),
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
                style.position,
                Position::Relative | Position::Absolute | Position::Fixed | Position::Sticky
            ) || !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_non_positioned_effect(
        element: &Element,
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        let effects = assets::paint_effects_for_element_box(element, style, bounds);
        Self::for_non_positioned_effect_with_effects(style, effects)
    }

    pub(in crate::layout) fn for_non_positioned_style_effect(
        style: &ComputedStyle,
        bounds: PaintClip,
    ) -> Self {
        let effects = assets::paint_effects_for_box(style, bounds);
        Self::for_non_positioned_effect_with_effects(style, effects)
    }

    pub(in crate::layout) fn for_non_positioned_effect_with_effects(
        style: &ComputedStyle,
        effects: PaintEffects,
    ) -> Self {
        let in_flow_positioned = matches!(style.position, Position::Relative | Position::Sticky);
        let stack_level = if in_flow_positioned {
            StackLevel::from_optional_z_index(style.z_index)
        } else {
            StackLevel::Auto
        };
        let is_real_stacking_context = matches!(style.position, Position::Sticky)
            || (style.position == Position::Relative && style.z_index.is_some())
            || style_creates_effect_stacking_context(style, effects);
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
            establishes_containing_block: !style.transform.is_empty(),
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
        let is_real_stacking_context = style_creates_effect_stacking_context(style, effects);
        Self {
            parent_band,
            stack_level: StackLevel::Auto,
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
            establishes_containing_block: !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn for_flex_item(style: &ComputedStyle, bounds: PaintClip) -> Self {
        let stack_level = StackLevel::from_optional_z_index(style.z_index);
        let effects = assets::paint_effects_for_box(style, bounds);
        let is_real_stacking_context =
            style.z_index.is_some() || style_creates_effect_stacking_context(style, effects);
        Self {
            parent_band: stack_level.paint_band(),
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
            establishes_containing_block: !style.transform.is_empty(),
            captures_positioned_descendants: is_real_stacking_context,
            effects,
        }
    }

    pub(in crate::layout) fn style_needs_non_positioned_scope(
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        matches!(style.position, Position::Relative | Position::Sticky)
            || style_creates_effect_stacking_context(
                style,
                assets::paint_effects_for_element_box(
                    element,
                    style,
                    PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
                ),
            )
            || used_overflow_clips_element(element, style)
    }
}

pub(in crate::layout) fn style_creates_effect_stacking_context(
    style: &ComputedStyle,
    effects: PaintEffects,
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
        || style.contain.paint
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

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct MarkerImage {
    pub(in crate::layout) decoded: DecodedPngImage,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineWord {
    pub(in crate::layout) text: String,
    pub(in crate::layout) style: InlineStyle,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    pub(in crate::layout) ancestor_inline_decorations: Vec<InlineAncestorDecoration>,
}

pub(in crate::layout) type InlineStyle = Rc<ComputedStyle>;

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
            vector: InlineVector::new(offset.x, offset.y),
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

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragmentData {
    pub(in crate::layout) text: Rc<str>,
    pub(in crate::layout) style: Rc<ComputedStyle>,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) generated_leader: bool,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    pub(in crate::layout) ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
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
            text,
            Rc::new(style),
            baseline_shift,
            link_target,
            mergeable,
            source,
            generated_leader,
            hanging_edges,
            ancestor_inline_decorations,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn new_shared_style(
        text: impl Into<String>,
        style: InlineStyle,
        baseline_shift: f32,
        link_target: Option<String>,
        mergeable: bool,
        source: InlineTextSource,
        generated_leader: bool,
        hanging_edges: InlineHangingEdges,
        ancestor_inline_decorations: Vec<InlineAncestorDecoration>,
    ) -> Self {
        Self {
            data: Rc::new(InlineFragmentData {
                text: Rc::from(text.into()),
                style,
                link_target: link_target.map(Rc::from),
                mergeable,
                source,
                generated_leader,
                hanging_edges,
                ancestor_inline_decorations: Rc::from(
                    ancestor_inline_decorations.into_boxed_slice(),
                ),
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

    pub(in crate::layout) fn with_hanging_edges(
        mut self,
        hanging_edges: InlineHangingEdges,
    ) -> Self {
        Rc::make_mut(&mut self.data).hanging_edges = hanging_edges;
        self
    }

    pub(in crate::layout) fn set_text(&mut self, text: impl Into<String>) {
        Rc::make_mut(&mut self.data).text = Rc::from(text.into());
    }

    pub(in crate::layout) fn set_mergeable(&mut self, mergeable: bool) {
        Rc::make_mut(&mut self.data).mergeable = mergeable;
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
}

pub(in crate::layout) trait InlineFragmentAccess {
    fn text(&self) -> &str;
    fn style(&self) -> &ComputedStyle;
    fn baseline_shift(&self) -> f32;
    fn visual_offset(&self) -> InlineVisualOffset;
    fn link_target(&self) -> Option<&str>;
    fn mergeable(&self) -> bool;
    fn source(&self) -> InlineTextSource;
    fn generated_leader(&self) -> bool;
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

    fn generated_leader(&self) -> bool {
        self.generated_leader()
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

    pub(in crate::layout) fn add_baseline_shift(&mut self, shift: f32) {
        self.baseline_shift += shift;
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

    fn generated_leader(&self) -> bool {
        self.fragment.generated_leader()
    }

    fn ancestor_inline_decorations(&self) -> &[InlineAncestorDecoration] {
        self.fragment.ancestor_inline_decorations()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineTextSource {
    Normal,
    Generated,
    Marker,
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
}

impl StaticHorizontalPosition {
    pub(in crate::layout) fn new(left: f32, right: f32) -> Self {
        Self { left, right }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsoluteStaticPosition {
    page_left_x: f32,
    page_right_x: f32,
    page_top_y: f32,
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
        Self {
            page_left_x,
            page_right_x,
            page_top_y,
        }
    }

    pub(in crate::layout) fn horizontal_position(
        self,
        containing_block: ContainingBlock,
    ) -> StaticHorizontalPosition {
        StaticHorizontalPosition::new(
            self.page_left_x - containing_block.x(),
            containing_block.x() + containing_block.width() - self.page_right_x,
        )
    }

    pub(in crate::layout) fn vertical_start(self, containing_block: ContainingBlock) -> f32 {
        containing_block.top_y() - self.page_top_y
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
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) link_paint_rect: Option<PaintRect>,
    pub(in crate::layout) decoration_paint_rect: Option<PaintRect>,
    pub(in crate::layout) shaped: ShapedInlineLine,
    pub(in crate::layout) source: InlineTextSource,
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
    pub(in crate::layout) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: InlineRect::new(
                InlinePoint::new(x, y),
                InlineSize::new(width.max(0.0), height.max(0.0)),
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
        Self::new(
            self.x() + offset.x(),
            self.y() + offset.y(),
            self.width(),
            self.height(),
        )
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
