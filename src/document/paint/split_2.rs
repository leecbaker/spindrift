use super::*;

impl PagePaintTree {
    pub(crate) fn new() -> Self {
        Self {
            root: PaintStackingContext::root(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.root.bands.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn flattened_operations(&self) -> Vec<PaintOperation> {
        self.root.bands.flattened_operations()
    }

    pub(crate) fn push_operation(&mut self, band: PaintBand, operation: PaintOperation) {
        self.root.bands.push_operation(band, operation);
    }

    pub(crate) fn push_link(&mut self, band: PaintBand, link: RenderedLink) {
        self.root.bands.push_link(band, link);
    }

    pub(crate) fn sort_stacking_contexts(&mut self) {
        self.root.bands.sort_stacking_contexts();
    }

    pub(crate) fn append_display_list(&mut self, display_list: PaintDisplayList) {
        self.root.bands.append_bands(display_list.bands);
    }

    pub(in crate::document) fn fragment_since(
        &self,
        checkpoint: &Self,
        page: &Page,
    ) -> PaintFragment {
        PaintFragment {
            display_list: PaintDisplayList {
                bands: self.root.bands.fragment_since(&checkpoint.root.bands, page),
            },
            links: Vec::new(),
        }
    }

    pub(in crate::document) fn operation_node_fragment_since(
        &self,
        checkpoint: &Self,
    ) -> PaintBandList {
        self.root
            .bands
            .operation_node_fragment_since(&checkpoint.root.bands)
    }

    pub(crate) fn transformed_links(&self) -> Vec<RenderedLink> {
        let mut links = Vec::new();
        self.root
            .push_transformed_links(PaintTransform::identity(), &mut links);
        links
    }
}

/// CSS painting-order band inside one stacking context.
///
/// CSS 2.2 Appendix E defines stacking-context painting as a sequence of
/// ordered bands. Keeping the band identity until flattening lets positioned
/// and fragmented descendants be replayed in their spec slot instead of being
/// spliced into an already-flat PDF primitive stream:
/// <https://www.w3.org/TR/CSS22/zindex.html>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintBand {
    PageBackground,
    BackgroundBorder,
    NegativeZ,
    InFlowBlock,
    Float,
    Inline,
    AutoZeroZ,
    PositiveZ,
    Outline,
}

impl PaintBand {
    pub(crate) const ORDER: [Self; 9] = [
        Self::PageBackground,
        Self::BackgroundBorder,
        Self::NegativeZ,
        Self::InFlowBlock,
        Self::Float,
        Self::Inline,
        Self::AutoZeroZ,
        Self::PositiveZ,
        Self::Outline,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PageBackground => 0,
            Self::BackgroundBorder => 1,
            Self::NegativeZ => 2,
            Self::InFlowBlock => 3,
            Self::Float => 4,
            Self::Inline => 5,
            Self::AutoZeroZ => 6,
            Self::PositiveZ => 7,
            Self::Outline => 8,
        }
    }
}

/// Ordered paint-band buckets for a fragment-local display list.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintBandList {
    pub(crate) bands: [Vec<PaintDisplayItem>; 9],
}

impl PaintBandList {
    pub(in crate::document) fn is_empty(&self) -> bool {
        self.bands.iter().all(Vec::is_empty)
    }

    pub(in crate::document) fn extend_band(
        &mut self,
        band: PaintBand,
        items: impl IntoIterator<Item = PaintDisplayItem>,
    ) {
        self.bands[band.index()].extend(items);
    }

    pub(in crate::document) fn push_operation(
        &mut self,
        band: PaintBand,
        operation: PaintOperation,
    ) {
        self.bands[band.index()].push(PaintDisplayItem::Operation(operation));
    }

    pub(in crate::document) fn push_link(&mut self, band: PaintBand, link: RenderedLink) {
        self.bands[band.index()].push(PaintDisplayItem::Link(link));
    }

    pub(crate) fn push_context(&mut self, context: PaintStackingContext) {
        let band = context.stack_level.paint_band();
        self.push_context_in_band(band, context);
    }

    pub(crate) fn push_context_in_band(&mut self, band: PaintBand, context: PaintStackingContext) {
        self.bands[band.index()].push(PaintDisplayItem::StackingContext(context));
    }

    pub(crate) fn push_effect_scope_in_band(&mut self, band: PaintBand, scope: PaintEffectScope) {
        self.bands[band.index()].push(PaintDisplayItem::EffectScope(scope));
    }

    pub(in crate::document) fn sort_stacking_contexts(&mut self) {
        for band in [
            PaintBand::NegativeZ,
            PaintBand::AutoZeroZ,
            PaintBand::PositiveZ,
        ] {
            self.bands[band.index()].sort_by_key(|item| match item {
                PaintDisplayItem::StackingContext(context) => {
                    (context.stack_level.sort_key(), context.source_order)
                }
                PaintDisplayItem::Operation(_)
                | PaintDisplayItem::EffectScope(_)
                | PaintDisplayItem::Primitive(_)
                | PaintDisplayItem::Link(_) => ((0, 0), 0),
            });
        }
    }

    pub(in crate::document) fn append_bands(&mut self, bands: PaintBandList) {
        for band in PaintBand::ORDER {
            self.bands[band.index()].extend(bands.bands[band.index()].clone());
        }
    }

    pub(in crate::document) fn fragment_since(&self, checkpoint: &Self, page: &Page) -> Self {
        let mut bands = PaintBandList::default();
        for band in PaintBand::ORDER {
            let current = &self.bands[band.index()];
            let checkpoint = &checkpoint.bands[band.index()];
            let start = shared_prefix_len(current, checkpoint);
            bands.bands[band.index()].extend(
                current[start..]
                    .iter()
                    .cloned()
                    .filter_map(|item| item.into_primitive_node(page)),
            );
        }
        bands
    }

    pub(in crate::document) fn operation_node_fragment_since(&self, checkpoint: &Self) -> Self {
        let mut bands = PaintBandList::default();
        for band in PaintBand::ORDER {
            let current = &self.bands[band.index()];
            let checkpoint = &checkpoint.bands[band.index()];
            let start = shared_prefix_len(current, checkpoint);
            bands.bands[band.index()].extend(current[start..].iter().cloned());
        }
        bands
    }

    pub(in crate::document) fn into_items_in_order(self) -> Vec<PaintDisplayItem> {
        let mut ordered = Vec::new();
        for band in PaintBand::ORDER {
            ordered.extend(self.bands[band.index()].clone());
        }
        ordered
    }

    pub(in crate::document) fn push_flattened_primitives(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
    ) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                match item {
                    PaintDisplayItem::Operation(_) | PaintDisplayItem::Link(_) => {}
                    PaintDisplayItem::Primitive(primitive) => primitives.push(primitive.clone()),
                    PaintDisplayItem::StackingContext(context) => {
                        context.push_flattened_primitives(primitives);
                    }
                    PaintDisplayItem::EffectScope(scope) => {
                        scope.push_flattened_primitives(primitives);
                    }
                }
            }
        }
    }

    pub(in crate::document) fn flattened_operations(&self) -> Vec<PaintOperation> {
        let mut operations = Vec::new();
        self.push_flattened_operations(&mut operations);
        operations
    }

    pub(in crate::document) fn push_flattened_operations(
        &self,
        operations: &mut Vec<PaintOperation>,
    ) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                match item {
                    PaintDisplayItem::Operation(operation) => operations.push(*operation),
                    PaintDisplayItem::StackingContext(context) => {
                        context.bands.push_flattened_operations(operations);
                    }
                    PaintDisplayItem::EffectScope(scope) => {
                        scope.push_flattened_operations(operations);
                    }
                    PaintDisplayItem::Primitive(_) | PaintDisplayItem::Link(_) => {}
                }
            }
        }
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .map(|item| item.translated(offset))
                    .collect()
            }),
        }
    }

    pub(in crate::document) fn into_operation_nodes(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| item.into_operation_node(operations))
                    .collect()
            }),
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
        Self {
            bands: self.bands.clone().map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| item.into_primitive_node(page))
                    .collect()
            }),
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                item.push_transformed_links(transform, links);
            }
        }
    }
}

pub(in crate::document) fn shared_prefix_len(
    left: &[PaintDisplayItem],
    right: &[PaintDisplayItem],
) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

/// One item in a fragment-local display list.
///
/// The `StackingContext` variant represents the recursive units described by
/// CSS 2.2 Appendix E and CSS Positioned Layout stack levels:
/// <https://www.w3.org/TR/CSS22/zindex.html> and
/// <https://www.w3.org/TR/css-position-3/#painting-order>.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintDisplayItem {
    Operation(PaintOperation),
    Primitive(PaintPrimitive),
    StackingContext(PaintStackingContext),
    EffectScope(PaintEffectScope),
    Link(RenderedLink),
}

impl PaintDisplayItem {
    pub(in crate::document) fn translated(self, offset: PaintVector) -> Self {
        match self {
            Self::Operation(operation) => Self::Operation(operation),
            Self::Primitive(primitive) => Self::Primitive(primitive.translated(offset)),
            Self::StackingContext(context) => Self::StackingContext(context.translated(offset)),
            Self::EffectScope(scope) => Self::EffectScope(scope.translated(offset)),
            Self::Link(link) => Self::Link(link.translated(offset)),
        }
    }

    pub(in crate::document) fn into_operation_node(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Option<Self> {
        match self {
            Self::Primitive(_) => operations.next().map(Self::Operation),
            Self::StackingContext(context) => Some(Self::StackingContext(
                context.into_operation_nodes(operations),
            )),
            Self::EffectScope(scope) => {
                Some(Self::EffectScope(scope.into_operation_nodes(operations)))
            }
            Self::Operation(operation) => Some(Self::Operation(operation)),
            Self::Link(link) => Some(Self::Link(link)),
        }
    }

    pub(in crate::document) fn into_primitive_node(self, page: &Page) -> Option<Self> {
        match self {
            Self::Operation(operation) => page.paint_primitive(&operation).map(Self::Primitive),
            Self::StackingContext(context) => {
                Some(Self::StackingContext(context.into_primitive_nodes(page)))
            }
            Self::EffectScope(scope) => Some(Self::EffectScope(scope.into_primitive_nodes(page))),
            Self::Primitive(primitive) => Some(Self::Primitive(primitive)),
            Self::Link(link) => Some(Self::Link(link)),
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        match self {
            Self::Link(link) => links.push(link.transformed(transform)),
            Self::StackingContext(context) => context.push_transformed_links(transform, links),
            Self::EffectScope(scope) => scope.push_transformed_links(transform, links),
            Self::Operation(_) | Self::Primitive(_) => {}
        }
    }
}

/// Non-stacking paint effects applied to display items in their existing band.
///
/// CSS Overflow clips descendants without creating a stacking context. Keeping
/// the effect as an in-band scope preserves CSS 2.2 Appendix E paint ordering
/// while still emitting a PDF graphics-state clip around the affected content:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
/// <https://www.w3.org/TR/CSS22/zindex.html>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintEffectScope {
    pub(crate) effects: PaintEffects,
    pub(crate) bounds: Option<PaintClip>,
    pub(crate) items: Vec<PaintDisplayItem>,
}

impl PaintEffectScope {
    pub(crate) fn new(
        effects: PaintEffects,
        bounds: Option<PaintClip>,
        items: Vec<PaintDisplayItem>,
    ) -> Self {
        Self {
            effects,
            bounds,
            items,
        }
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            effects: self.effects.translated(offset),
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
            items: self
                .items
                .into_iter()
                .map(|item| item.translated(offset))
                .collect(),
        }
    }

    pub(in crate::document) fn push_flattened_primitives(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
    ) {
        for item in &self.items {
            match item {
                PaintDisplayItem::Operation(_) | PaintDisplayItem::Link(_) => {}
                PaintDisplayItem::Primitive(primitive) => primitives.push(primitive.clone()),
                PaintDisplayItem::StackingContext(context) => {
                    context.push_flattened_primitives(primitives);
                }
                PaintDisplayItem::EffectScope(scope) => {
                    scope.push_flattened_primitives(primitives);
                }
            }
        }
    }

    pub(in crate::document) fn push_flattened_operations(
        &self,
        operations: &mut Vec<PaintOperation>,
    ) {
        for item in &self.items {
            match item {
                PaintDisplayItem::Operation(operation) => operations.push(*operation),
                PaintDisplayItem::StackingContext(context) => {
                    context.bands.push_flattened_operations(operations);
                }
                PaintDisplayItem::EffectScope(scope) => {
                    scope.push_flattened_operations(operations);
                }
                PaintDisplayItem::Primitive(_) | PaintDisplayItem::Link(_) => {}
            }
        }
    }

    pub(in crate::document) fn into_operation_nodes(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Self {
        Self {
            effects: self.effects,
            bounds: self.bounds,
            items: self
                .items
                .into_iter()
                .filter_map(|item| item.into_operation_node(operations))
                .collect(),
        }
    }

    pub(in crate::document) fn into_primitive_nodes(self, page: &Page) -> Self {
        Self {
            effects: self.effects,
            bounds: self.bounds,
            items: self
                .items
                .into_iter()
                .filter_map(|item| item.into_primitive_node(page))
                .collect(),
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        let transform = if let Some(transform_effect) = self.effects.transform {
            transform.multiply(transform_effect)
        } else {
            transform
        };
        for item in &self.items {
            item.push_transformed_links(transform, links);
        }
    }
}

/// CSS positioned stack level for one stacking-context node.
///
/// `auto` is distinct from integer `0` in CSS Positioned Layout even though
/// both paint in the auto/zero band of the parent stacking context:
/// <https://www.w3.org/TR/css-position-3/#painting-order>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackLevel {
    Auto,
    Integer(i32),
}

impl StackLevel {
    pub(crate) fn from_z_index(z_index: i32) -> Self {
        Self::Integer(z_index)
    }

    pub(crate) fn from_optional_z_index(z_index: Option<i32>) -> Self {
        z_index.map_or(Self::Auto, Self::Integer)
    }

    pub(crate) fn paint_band(self) -> PaintBand {
        match self {
            Self::Integer(value) if value < 0 => PaintBand::NegativeZ,
            Self::Integer(value) if value > 0 => PaintBand::PositiveZ,
            Self::Auto | Self::Integer(0) => PaintBand::AutoZeroZ,
            Self::Integer(_) => PaintBand::AutoZeroZ,
        }
    }

    pub(crate) fn sort_key(self) -> (i32, i32) {
        match self {
            Self::Integer(value) => (value, 0),
            Self::Auto => (0, 0),
        }
    }
}

/// Effects applied to a whole stacking context before PDF emission.
///
/// CSS Transforms, CSS Color opacity, and CSS Overflow act on stacking-context
/// contents as a group. The current flattening path keeps these defaults until
/// PDF group and matrix emission are wired through page streams:
/// <https://www.w3.org/TR/css-transforms-1/>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintEffects {
    pub(crate) opacity: f32,
    pub(crate) transform: Option<PaintTransform>,
    pub(crate) overflow_clip: Option<PaintClip>,
    pub(crate) absolute_clip: Option<PaintClip>,
    pub(crate) clip_path: PaintClipPathEffect,
    pub(crate) mask: PaintMaskEffect,
    pub(crate) filter: PaintFilterEffect,
    pub(crate) blend_mode: PaintBlendMode,
    pub(crate) isolation: bool,
}

impl Default for PaintEffects {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            transform: None,
            overflow_clip: None,
            absolute_clip: None,
            clip_path: PaintClipPathEffect::None,
            mask: PaintMaskEffect::None,
            filter: PaintFilterEffect::None,
            blend_mode: PaintBlendMode::Normal,
            isolation: false,
        }
    }
}

impl PaintEffects {
    pub(crate) fn translated(mut self, offset: PaintVector) -> Self {
        self.overflow_clip = self.overflow_clip.map(|clip| clip.translated(offset));
        self.absolute_clip = self.absolute_clip.map(|clip| clip.translated(offset));
        self
    }

    pub(crate) fn needs_group(self) -> bool {
        self.opacity < 1.0
            || self.filter.is_active()
            || self.mask.is_active()
            || self.blend_mode != PaintBlendMode::Normal
            || self.isolation
    }

    pub(crate) fn without_group_effects(mut self) -> Self {
        self.opacity = 1.0;
        self.filter = PaintFilterEffect::None;
        self.mask = PaintMaskEffect::None;
        self.blend_mode = PaintBlendMode::Normal;
        self.isolation = false;
        self
    }

    pub(crate) fn ordered_steps(self) -> Vec<PaintEffectStep> {
        let mut steps = Vec::new();
        if let Some(clip) = self.absolute_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if let Some(clip) = self.overflow_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if self.clip_path.is_active() {
            steps.push(PaintEffectStep::ClipPath(self.clip_path));
        }
        if let Some(transform) = self.transform {
            steps.push(PaintEffectStep::Transform(transform));
        }
        if self.filter.is_active() {
            steps.push(PaintEffectStep::Filter(self.filter));
        }
        if self.mask.is_active() {
            steps.push(PaintEffectStep::Mask(self.mask));
        }
        if self.opacity < 1.0 {
            steps.push(PaintEffectStep::Opacity(self.opacity));
        }
        if self.blend_mode != PaintBlendMode::Normal {
            steps.push(PaintEffectStep::Blend(self.blend_mode));
        }
        if self.isolation {
            steps.push(PaintEffectStep::Isolation);
        }
        steps
    }
}

/// Shape source for a context-level CSS `clip-path`.
///
/// The current renderer records the source category so stacking and PDF group
/// construction are deterministic. Geometry emission is implemented later for
/// each non-`None` variant:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintClipPathEffect {
    None,
    Inset,
    Shape,
    Url,
    WillChange,
}

impl PaintClipPathEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Masking source recorded for context-level PDF grouping.
///
/// CSS Masking allows image and generated-image masks. Quire currently records
/// the presence of a mask for isolation/grouping and leaves shape/raster
/// emission as a remaining conformance step:
/// <https://www.w3.org/TR/css-masking-1/#the-mask-image>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintMaskEffect {
    None,
    MaskImage,
    WillChange,
}

impl PaintMaskEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Filter source recorded for context-level PDF grouping.
///
/// Filter function rendering is not complete yet; this type distinguishes real
/// authored filters from `will-change` pre-isolation.
/// <https://www.w3.org/TR/filter-effects-1/#FilterProperty>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintFilterEffect {
    None,
    FilterList,
    WillChange,
}

impl PaintFilterEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// PDF-facing blend mode selected by CSS `mix-blend-mode`.
///
/// The current content writer uses this to force isolated group construction;
/// future PDF ExtGState output can map these variants to `/BM` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl PaintBlendMode {
    pub(crate) const fn pdf_name(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Multiply => Some("Multiply"),
            Self::Screen => Some("Screen"),
            Self::Overlay => Some("Overlay"),
            Self::Darken => Some("Darken"),
            Self::Lighten => Some("Lighten"),
            Self::ColorDodge => Some("ColorDodge"),
            Self::ColorBurn => Some("ColorBurn"),
            Self::HardLight => Some("HardLight"),
            Self::SoftLight => Some("SoftLight"),
            Self::Difference => Some("Difference"),
            Self::Exclusion => Some("Exclusion"),
            Self::Hue => Some("Hue"),
            Self::Saturation => Some("Saturation"),
            Self::Color => Some("Color"),
            Self::Luminosity => Some("Luminosity"),
        }
    }

    pub(crate) fn resource_name(self) -> Option<String> {
        self.pdf_name().map(|name| format!("GSblend{name}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaintEffectStep {
    Clip(PaintClip),
    ClipPath(PaintClipPathEffect),
    Transform(PaintTransform),
    Filter(PaintFilterEffect),
    Mask(PaintMaskEffect),
    Opacity(f32),
    Blend(PaintBlendMode),
    Isolation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintTransform {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) c: f32,
    pub(crate) d: f32,
    pub(crate) e: f32,
    pub(crate) f: f32,
}

impl PaintTransform {
    pub(crate) const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a paint-space translation transform.
    ///
    /// CSS Transforms applies translation functions in the element's current
    /// painting coordinate system; by this point Quire has already projected
    /// layout geometry into [`PaintSpace`]:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-functions>.
    pub(crate) fn translate(offset: PaintVector) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: offset.x,
            f: offset.y,
        }
    }

    pub(crate) fn multiply(self, right: Self) -> Self {
        Self {
            a: self.a * right.a + self.c * right.b,
            b: self.b * right.a + self.d * right.b,
            c: self.a * right.c + self.c * right.d,
            d: self.b * right.c + self.d * right.d,
            e: self.a * right.e + self.c * right.f + self.e,
            f: self.b * right.e + self.d * right.f + self.f,
        }
    }

    /// Apply this transform to a page-local paint point.
    ///
    /// CSS Transforms maps already-painted geometry into the parent painting
    /// coordinate system. Keeping the input and output as [`PaintPoint`]
    /// prevents transform effects from crossing into layout top-edge or PDF
    /// user-space coordinates by accident:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_point(self, point: PaintPoint) -> PaintPoint {
        PaintPoint::new(
            self.a * point.x + self.c * point.y + self.e,
            self.b * point.x + self.d * point.y + self.f,
        )
    }

    /// Transform a paint-space clip rectangle and return its axis-aligned bounds.
    ///
    /// CSS rectangular clips are transformed with the element, while PDF
    /// annotation bounds and effect isolation need a conservative axis-aligned
    /// paint rectangle after transform application:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_clip_to_aabb(self, clip: PaintClip) -> PaintClip {
        let points = [
            self.apply_point(clip.bottom_left()),
            self.apply_point(clip.bottom_right()),
            self.apply_point(clip.top_left()),
            self.apply_point(clip.top_right()),
        ];
        let min_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::NEG_INFINITY, f32::max);
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(min_x, min_y),
            PaintSize::new((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)),
        ))
    }
}

/// Axis-aligned paint clipping rectangle.
///
/// CSS Overflow clips box contents to a rectangular overflow clip edge in the
/// untransformed local coordinate space, and CSS Transforms then maps that
/// clipped output into parent coordinates:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintClip {
    pub(in crate::document) rect: PaintRect,
}

impl PaintClip {
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, y),
            PaintSize::new(width.max(0.0), height.max(0.0)),
        ))
    }

    pub(crate) fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    pub(crate) fn from_paint_point(point: PaintPoint) -> Self {
        Self::from_paint_rect(PaintRect::new(point, PaintSize::new(0.0, 0.0)))
    }

    pub(crate) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(crate) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(crate) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(crate) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    pub(crate) fn bottom_left(self) -> PaintPoint {
        self.rect.origin
    }

    pub(crate) fn bottom_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y())
    }

    pub(crate) fn top_left(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y() + self.height())
    }

    pub(crate) fn top_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y() + self.height())
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let left = self.x().max(other.x());
        let right = (self.x() + self.width()).min(other.x() + other.width());
        let bottom = self.y().max(other.y());
        let top = (self.y() + self.height()).min(other.y() + other.height());
        (right > left && top > bottom).then_some(Self::new(
            left,
            bottom,
            right - left,
            top - bottom,
        ))
    }

    pub(in crate::document) fn union(self, other: Self) -> Self {
        let left = self.x().min(other.x());
        let right = (self.x() + self.width()).max(other.x() + other.width());
        let bottom = self.y().min(other.y());
        let top = (self.y() + self.height()).max(other.y() + other.height());
        Self::new(
            left,
            bottom,
            (right - left).max(0.0),
            (top - bottom).max(0.0),
        )
    }
}

/// Nested stacking context captured during layout before PDF emission.
///
/// CSS 2.2 Appendix E paints each stacking context atomically at its parent
/// stack level. The current node keeps normal-flow content and child stacking
/// contexts together so descendants with large `z-index` values cannot escape
/// an ancestor stacking context when the fragment is replayed onto a page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintStackingContext {
    pub(crate) source_order: usize,
    pub(crate) stack_level: StackLevel,
    pub(crate) bands: PaintBandList,
    pub(crate) effects: PaintEffects,
    pub(crate) bounds: Option<PaintClip>,
}
