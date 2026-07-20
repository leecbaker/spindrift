use super::*;

impl PagePaintTree {
    pub(crate) fn new() -> Self {
        Self {
            root: PaintStackingContext::root(),
        }
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

    pub(crate) fn prepend_display_list(&mut self, display_list: PaintDisplayList) {
        self.root.bands.prepend_bands(display_list.bands);
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
    TableCellBorder,
    NegativeZ,
    InFlowBlock,
    TableCollapsedBorder,
    Float,
    Inline,
    AutoZeroZ,
    PositiveZ,
    Outline,
    ViewportChrome,
}

impl PaintBand {
    pub(crate) const ORDER: [Self; 12] = [
        Self::PageBackground,
        Self::BackgroundBorder,
        Self::TableCellBorder,
        Self::NegativeZ,
        Self::InFlowBlock,
        Self::TableCollapsedBorder,
        Self::Float,
        Self::Inline,
        Self::AutoZeroZ,
        Self::PositiveZ,
        Self::Outline,
        Self::ViewportChrome,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PageBackground => 0,
            Self::BackgroundBorder => 1,
            Self::TableCellBorder => 2,
            Self::NegativeZ => 3,
            Self::InFlowBlock => 4,
            Self::TableCollapsedBorder => 5,
            Self::Float => 6,
            Self::Inline => 7,
            Self::AutoZeroZ => 8,
            Self::PositiveZ => 9,
            Self::Outline => 10,
            Self::ViewportChrome => 11,
        }
    }
}

/// Ordered paint-band buckets for a fragment-local display list.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintBandList {
    pub(crate) bands: [Vec<PaintDisplayItem>; 12],
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
            // Floats do not create stacking contexts solely by floating, but
            // captured/overhanging float fragments are represented as atomic
            // contexts in this band. Their capture can complete after a
            // later sibling has been recorded, so Appendix E's tree order
            // must be restored before flattening the band.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            PaintBand::Float,
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
        let mut bands = bands.bands;
        for band in PaintBand::ORDER {
            self.bands[band.index()].extend(std::mem::take(&mut bands[band.index()]));
        }
    }

    /// Insert a display-list fragment before existing paint in every band.
    ///
    /// This is needed for generated page-margin stacking contexts with a
    /// negative z-index: CSS Paged Media places them beneath the document
    /// canvas even though margin boxes are generated after document layout.
    /// <https://www.w3.org/TR/css-page-3/#painting>
    pub(in crate::document) fn prepend_bands(&mut self, bands: PaintBandList) {
        let mut bands = bands.bands;
        for band in PaintBand::ORDER {
            let existing = std::mem::take(&mut self.bands[band.index()]);
            self.bands[band.index()] = std::mem::take(&mut bands[band.index()]);
            self.bands[band.index()].extend(existing);
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
        let mut bands = self.bands;
        PaintBand::ORDER
            .into_iter()
            .flat_map(|band| std::mem::take(&mut bands[band.index()]))
            .collect()
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

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                item.for_each_flattened_primitive(f);
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

    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .map(|item| item.translated(offset))
                    .collect()
            }),
        }
    }

    pub(in crate::document) fn into_recorded_nodes(self, page: &mut Page) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| item.into_recorded_node(page))
                    .collect()
            }),
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
        let mut bands = PaintBandList::default();
        for band in PaintBand::ORDER {
            bands.bands[band.index()].extend(
                self.bands[band.index()]
                    .iter()
                    .filter_map(|item| item.primitive_node_copy(page)),
            );
        }
        bands
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

    /// Clip primitive nodes without flattening their CSS paint-band tree.
    ///
    /// The PDF clip effect remains authoritative for paths, images, and glyphs
    /// that cross an edge. Geometry clipping here keeps Quire's public
    /// inspection primitives consistent for axis-aligned rectangles while
    /// retaining Appendix E ordering and nested stacking contexts.
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    pub(crate) fn clipped_primitives_to_rect(mut self, clip: PaintClip) -> Self {
        self.clip_primitives_to_rect(clip, PaintClipPurpose::Visual);
        self
    }

    /// Slice primitive nodes at a fragmentainer boundary while preserving
    /// paint groups whose principal boxes are monolithic.
    ///
    /// CSS Fragmentation moves a monolithic box as a unit (or lets it overflow
    /// an empty fragmentainer); it does not clip and replay successive pieces
    /// of that box in later fragmentainers:
    /// <https://www.w3.org/TR/css-break-3/#monolithic>.
    pub(crate) fn sliced_primitives_to_fragmentainer_rect(mut self, clip: PaintClip) -> Self {
        self.clip_primitives_to_rect(clip, PaintClipPurpose::FragmentainerSlice);
        self
    }

    fn clip_primitives_to_rect(&mut self, clip: PaintClip, purpose: PaintClipPurpose) {
        for items in &mut self.bands {
            *items = std::mem::take(items)
                .into_iter()
                .filter_map(|item| item.clipped_primitives_to_rect(clip, purpose))
                .collect();
        }
    }

    pub(crate) fn contains_monolithic_fragmentation(&self) -> bool {
        self.bands.iter().flatten().any(|item| match item {
            PaintDisplayItem::StackingContext(context) => {
                context.bands.contains_monolithic_fragmentation()
            }
            PaintDisplayItem::EffectScope(scope) => {
                scope.fragmentation == PaintFragmentation::Monolithic
                    || scope.items.iter().any(|item| match item {
                        PaintDisplayItem::StackingContext(context) => {
                            context.bands.contains_monolithic_fragmentation()
                        }
                        PaintDisplayItem::EffectScope(scope) => {
                            scope.contains_monolithic_fragmentation()
                        }
                        PaintDisplayItem::Operation(_)
                        | PaintDisplayItem::Primitive(_)
                        | PaintDisplayItem::Link(_) => false,
                    })
            }
            PaintDisplayItem::Operation(_)
            | PaintDisplayItem::Primitive(_)
            | PaintDisplayItem::Link(_) => false,
        })
    }

    pub(crate) fn contains_overflow_clip(&self) -> bool {
        self.bands
            .iter()
            .flatten()
            .any(PaintDisplayItem::contains_overflow_clip)
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
    pub(in crate::document) fn translated(self, offset: PaintTranslation) -> Self {
        match self {
            Self::Operation(operation) => Self::Operation(operation),
            Self::Primitive(primitive) => Self::Primitive(primitive.translated(offset)),
            Self::StackingContext(context) => Self::StackingContext(context.translated(offset)),
            Self::EffectScope(scope) => Self::EffectScope(scope.translated(offset)),
            Self::Link(link) => Self::Link(link.translated(offset)),
        }
    }

    fn contains_overflow_clip(&self) -> bool {
        match self {
            Self::StackingContext(context) => {
                context.effects.overflow_clip.is_some() || context.bands.contains_overflow_clip()
            }
            Self::EffectScope(scope) => {
                scope.effects.overflow_clip.is_some()
                    || scope.items.iter().any(Self::contains_overflow_clip)
            }
            Self::Operation(_) | Self::Primitive(_) | Self::Link(_) => false,
        }
    }

    /// Whether this already-materialized item lies entirely inside a
    /// rectangular paint effect.
    ///
    /// A PDF clip can antialias an otherwise covered edge, so adding one
    /// around a primitive that cannot reach the edge changes raster output.
    /// Keep this deliberately narrow: deferred operations and recursive
    /// contexts retain their clip because their final ink bounds are not
    /// available at this effect boundary.
    pub(in crate::document) fn is_wholly_contained_by_rect(&self, clip: PaintClip) -> bool {
        match self {
            Self::Primitive(primitive) => primitive
                .bounds()
                .is_some_and(|bounds| clip.contains(bounds)),
            Self::Link(link) => clip.contains(PaintClip::from_paint_rect(link.paint_rect())),
            Self::Operation(_) | Self::StackingContext(_) | Self::EffectScope(_) => false,
        }
    }

    /// Bounds of this recorded subtree after any of its own rectangular clips.
    ///
    /// This intentionally rejects transforms and non-rectangular effects:
    /// their visible ink cannot be established from the page operations alone.
    pub(in crate::document) fn recorded_paint_bounds(
        &self,
        page: &Page,
    ) -> std::result::Result<Option<PaintClip>, ()> {
        match self {
            Self::Operation(operation) => page
                .paint_primitive(operation)
                .and_then(|primitive| primitive.bounds())
                .map(Some)
                .ok_or(()),
            Self::Primitive(primitive) => primitive.bounds().map(Some).ok_or(()),
            Self::Link(link) => Ok(Some(PaintClip::from_paint_rect(link.paint_rect()))),
            Self::StackingContext(context) => recorded_context_paint_bounds(
                page,
                &context.effects,
                crate::document::PaintBand::ORDER
                    .into_iter()
                    .flat_map(|band| context.bands.bands[band.index()].iter()),
            ),
            Self::EffectScope(scope) => {
                recorded_context_paint_bounds(page, &scope.effects, scope.items.iter())
            }
        }
    }

    fn clipped_primitives_to_rect(
        self,
        clip: PaintClip,
        purpose: PaintClipPurpose,
    ) -> Option<Self> {
        match self {
            Self::Primitive(primitive) => primitive.clipped_to_rect(clip).map(Self::Primitive),
            Self::StackingContext(mut context) => {
                context.bands.clip_primitives_to_rect(clip, purpose);
                context.bounds = context.bounds.and_then(|bounds| bounds.intersect(clip));
                (!context.bands.is_empty()).then_some(Self::StackingContext(context))
            }
            Self::EffectScope(mut scope) => {
                if purpose == PaintClipPurpose::FragmentainerSlice
                    && scope.fragmentation == PaintFragmentation::Monolithic
                {
                    return scope
                        .bounds
                        .is_some_and(|bounds| monolithic_block_start_is_in_slice(bounds, clip))
                        .then_some(Self::EffectScope(scope));
                }
                scope.items = scope
                    .items
                    .into_iter()
                    .filter_map(|item| item.clipped_primitives_to_rect(clip, purpose))
                    .collect();
                scope.bounds = scope.bounds.and_then(|bounds| bounds.intersect(clip));
                (!scope.items.is_empty()).then_some(Self::EffectScope(scope))
            }
            Self::Link(mut link) => {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(Self::Link(link))
            }
            // Captured layout fragments normally contain primitive nodes. Keep
            // an already-recorded operation intact because resolving it needs
            // its owning `Page`; the enclosing PDF effect still clips it.
            Self::Operation(operation) => Some(Self::Operation(operation)),
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

    pub(in crate::document) fn into_recorded_node(self, page: &mut Page) -> Option<Self> {
        match self {
            Self::Primitive(primitive) => {
                let operation = page.record_paint_primitive(primitive);
                Some(Self::Operation(operation))
            }
            Self::StackingContext(context) => {
                Some(Self::StackingContext(context.into_recorded_nodes(page)))
            }
            Self::EffectScope(scope) => Some(Self::EffectScope(scope.into_recorded_nodes(page))),
            Self::Operation(operation) => Some(Self::Operation(operation)),
            Self::Link(link) => Some(Self::Link(link)),
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Option<Self> {
        match self {
            Self::Operation(operation) => page.paint_primitive(operation).map(Self::Primitive),
            Self::StackingContext(context) => {
                Some(Self::StackingContext(context.primitive_node_copy(page)))
            }
            Self::EffectScope(scope) => Some(Self::EffectScope(scope.primitive_node_copy(page))),
            Self::Primitive(primitive) => Some(Self::Primitive(primitive.clone())),
            Self::Link(link) => Some(Self::Link(link.clone())),
        }
    }

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        match self {
            Self::Primitive(primitive) => f(primitive),
            Self::StackingContext(context) => context.for_each_flattened_primitive(f),
            Self::EffectScope(scope) => scope.for_each_flattened_primitive(f),
            Self::Operation(_) | Self::Link(_) => {}
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

fn recorded_context_paint_bounds<'a>(
    page: &Page,
    effects: &PaintEffects,
    items: impl Iterator<Item = &'a PaintDisplayItem>,
) -> std::result::Result<Option<PaintClip>, ()> {
    if !effects_have_only_rectangular_clips(effects) {
        return Err(());
    }
    let mut bounds = None;
    for item in items {
        let Some(item_bounds) = item.recorded_paint_bounds(page)? else {
            continue;
        };
        bounds = Some(union_paint_clips(bounds, item_bounds));
    }
    for clip in [effects.absolute_clip, effects.overflow_clip]
        .into_iter()
        .flatten()
    {
        bounds = bounds.and_then(|bounds| bounds.intersect(clip));
    }
    Ok(bounds)
}

fn effects_have_only_rectangular_clips(effects: &PaintEffects) -> bool {
    effects.opacity >= 1.0
        && effects.transform.is_none()
        && !effects.suppress_paint
        && effects.overflow_clip_union.is_none()
        && effects.rounded_overflow_clip.is_none()
        && !effects.clip_path.is_active()
        && !effects.mask.is_active()
        && !effects.filter.is_active()
        && effects.blend_mode == PaintBlendMode::Normal
        && !effects.isolation
}

fn union_paint_clips(left: Option<PaintClip>, right: PaintClip) -> PaintClip {
    let Some(left) = left else {
        return right;
    };
    let x = left.x().min(right.x());
    let y = left.y().min(right.y());
    let right_edge = (left.x() + left.width()).max(right.x() + right.width());
    let top_edge = (left.y() + left.height()).max(right.y() + right.height());
    PaintClip::new(x, y, right_edge - x, top_edge - y)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaintClipPurpose {
    Visual,
    FragmentainerSlice,
}

fn monolithic_block_start_is_in_slice(bounds: PaintClip, slice: PaintClip) -> bool {
    let block_start = bounds.y() + bounds.height();
    // Fragmentainer slices share an edge. Treat the block-end edge as open
    // (with layout epsilon) so floating-point noise cannot assign a
    // monolithic subtree to both neighboring slices.
    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    block_start > slice.y() + 0.01 && block_start <= slice.y() + slice.height() + 0.01
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
    pub(crate) fragmentation: PaintFragmentation,
    pub(crate) items: Vec<PaintDisplayItem>,
}

/// Fragmentation behavior retained with an element's paint subtree.
///
/// This is separate from visual clipping: a monolithic subtree remains whole
/// only when anonymous fragmentainers partition captured paint, while authored
/// overflow and page-area clips still apply normally.
/// <https://www.w3.org/TR/css-break-3/#monolithic>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PaintFragmentation {
    #[default]
    Fragmentable,
    Monolithic,
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
            fragmentation: PaintFragmentation::Fragmentable,
            items,
        }
    }

    pub(crate) fn monolithic(bounds: PaintClip, items: Vec<PaintDisplayItem>) -> Self {
        Self {
            effects: PaintEffects::default(),
            bounds: Some(bounds),
            fragmentation: PaintFragmentation::Monolithic,
            items,
        }
    }

    fn contains_monolithic_fragmentation(&self) -> bool {
        self.fragmentation == PaintFragmentation::Monolithic
            || self.items.iter().any(|item| match item {
                PaintDisplayItem::StackingContext(context) => {
                    context.bands.contains_monolithic_fragmentation()
                }
                PaintDisplayItem::EffectScope(scope) => scope.contains_monolithic_fragmentation(),
                PaintDisplayItem::Operation(_)
                | PaintDisplayItem::Primitive(_)
                | PaintDisplayItem::Link(_) => false,
            })
    }

    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        Self {
            effects: self.effects.translated(offset),
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
            fragmentation: self.fragmentation,
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

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        for item in &self.items {
            item.for_each_flattened_primitive(f);
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

    pub(in crate::document) fn into_recorded_nodes(self, page: &mut Page) -> Self {
        Self {
            effects: self.effects.clone(),
            bounds: self.bounds,
            fragmentation: self.fragmentation,
            items: self
                .items
                .into_iter()
                .filter_map(|item| item.into_recorded_node(page))
                .collect(),
        }
    }

    pub(in crate::document) fn into_primitive_nodes(self, page: &Page) -> Self {
        Self {
            effects: self.effects,
            bounds: self.bounds,
            fragmentation: self.fragmentation,
            items: self
                .items
                .into_iter()
                .filter_map(|item| item.into_primitive_node(page))
                .collect(),
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
        Self {
            effects: self.effects.clone(),
            bounds: self.bounds,
            fragmentation: self.fragmentation,
            items: self
                .items
                .iter()
                .filter_map(|item| item.primitive_node_copy(page))
                .collect(),
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        if self.effects.suppresses_paint() {
            return;
        }
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
/// CSS Transforms, CSS CssColor opacity, and CSS Overflow act on stacking-context
/// contents as a group. The current flattening path keeps these defaults until
/// PDF group and matrix emission are wired through page streams:
/// <https://www.w3.org/TR/css-transforms-1/>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintEffects {
    pub(crate) opacity: f32,
    pub(crate) transform: Option<PaintTransform>,
    /// A non-invertible CSS transform suppresses the entire transformed
    /// element, including its descendants and links.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>
    pub(crate) suppress_paint: bool,
    pub(crate) overflow_clip: Option<PaintClip>,
    /// A disjoint union of rectangular overflow clips. This keeps table-cell
    /// rowspan holes intact through retained paint and PDF serialization.
    pub(crate) overflow_clip_union: Option<PaintClipUnion>,
    pub(crate) rounded_overflow_clip: Option<RenderedRoundedRect>,
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
            suppress_paint: false,
            overflow_clip: None,
            overflow_clip_union: None,
            rounded_overflow_clip: None,
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
    pub(crate) const fn suppresses_paint(&self) -> bool {
        self.suppress_paint
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.overflow_clip = self.overflow_clip.map(|clip| clip.translated(offset));
        self.overflow_clip_union = self
            .overflow_clip_union
            .map(|clips| clips.translated(offset));
        self.rounded_overflow_clip = self
            .rounded_overflow_clip
            .map(|clip| clip.translated(offset));
        self.absolute_clip = self.absolute_clip.map(|clip| clip.translated(offset));
        self.clip_path = match self.clip_path {
            PaintClipPathEffect::Polygon(polygon) => {
                PaintClipPathEffect::Polygon(Box::new((*polygon).translated(offset)))
            }
            PaintClipPathEffect::Path(path) => PaintClipPathEffect::Path(path.translated(offset)),
            effect => effect,
        };
        self
    }

    pub(crate) fn needs_group(&self) -> bool {
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

    pub(crate) fn ordered_steps(&self) -> Vec<PaintEffectStep> {
        let mut steps = Vec::new();
        // CSS transforms establish the local coordinate system for all of an
        // element's painting effects.  PDF freezes a clipping path in the
        // current CTM when it is installed, so writing clips first would
        // leave them page-aligned while the subtree moves independently.
        // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
        if let Some(transform) = self.transform {
            steps.push(PaintEffectStep::Transform(transform));
        }
        if let Some(clip) = self.absolute_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if let Some(clip) = self.overflow_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if let Some(clips) = &self.overflow_clip_union {
            steps.push(PaintEffectStep::ClipUnion(*clips));
        }
        if let Some(clip) = self.rounded_overflow_clip {
            steps.push(PaintEffectStep::RoundedClip(clip));
        }
        if self.clip_path.is_active() {
            steps.push(PaintEffectStep::ClipPath(self.clip_path.clone()));
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
/// A resolved polygon carries typed page-local geometry ready for PDF clipping;
/// unsupported CSS forms preserve their category for stacking behavior:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintClipPathEffect {
    None,
    Polygon(Box<RenderedClipPathPolygon>),
    Path(RenderedPathClip),
    Shape,
    Url,
    WillChange,
}

impl PaintClipPathEffect {
    pub(crate) const fn is_active(&self) -> bool {
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintEffectStep {
    Clip(PaintClip),
    ClipUnion(PaintClipUnion),
    RoundedClip(RenderedRoundedRect),
    ClipPath(PaintClipPathEffect),
    Transform(PaintTransform),
    Filter(PaintFilterEffect),
    Mask(PaintMaskEffect),
    Opacity(f32),
    Blend(PaintBlendMode),
    Isolation,
}

/// Resolved CSS `polygon()` geometry in page-local paint space.
///
/// The fixed inline capacity preserves the copyable paint-effect contract used
/// by fragmented layout. It covers ordinary basic-shape polygons while larger
/// polygons retain their CSS stacking effect until arbitrary basic-shape
/// storage is available.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RenderedClipPathPolygon {
    points: [PaintPoint; Self::MAX_POINTS],
    len: u8,
}

impl RenderedClipPathPolygon {
    /// Retained contours are also used for circular `border-shape` overflow
    /// clips.  Sixty-four points keep the maximum chord error below a raster
    /// pixel at ordinary CSS page scales while retaining fixed-size, copyable
    /// storage for paint-effect translation.
    pub(crate) const MAX_POINTS: usize = 64;

    pub(crate) fn new(points: &[PaintPoint]) -> Option<Self> {
        if !(3..=Self::MAX_POINTS).contains(&points.len()) {
            return None;
        }
        let mut stored = [PaintPoint::new(0.0, 0.0); Self::MAX_POINTS];
        stored[..points.len()].copy_from_slice(points);
        Some(Self {
            points: stored,
            len: points.len() as u8,
        })
    }

    pub(crate) fn points(&self) -> &[PaintPoint] {
        &self.points[..usize::from(self.len)]
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        for point in &mut self.points[..usize::from(self.len)] {
            *point = offset.transform_point(*point);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintTransform(euclid::Transform2D<f32, PaintSpace, PaintSpace>);

impl PaintTransform {
    pub(crate) fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self(euclid::Transform2D::new(a, b, c, d, e, f))
    }

    /// Adopt an affine transform which has already been resolved into
    /// bottom-left page paint coordinates.
    pub(crate) fn from_transform(
        transform: euclid::Transform2D<f32, PaintSpace, PaintSpace>,
    ) -> Self {
        Self(transform)
    }

    pub(crate) fn identity() -> Self {
        Self(euclid::Transform2D::identity())
    }

    /// Build a paint-space translation transform.
    ///
    /// CSS Transforms applies translation functions in the element's current
    /// painting coordinate system; by this point Quire has already projected
    /// layout geometry into [`PaintSpace`]:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-functions>.
    pub(crate) fn translate(offset: PaintTranslation) -> Self {
        Self(offset.to_transform())
    }

    pub(crate) fn multiply(self, right: Self) -> Self {
        // `Transform2D::then` applies its argument after its receiver. CSS
        // matrix multiplication applies `right` first, then `self`.
        Self(right.0.then(&self.0))
    }

    pub(crate) fn scale(x: f32, y: f32) -> Self {
        Self(euclid::Transform2D::scale(x, y))
    }

    pub(crate) fn a(self) -> f32 {
        self.0.m11
    }
    pub(crate) fn b(self) -> f32 {
        self.0.m12
    }
    pub(crate) fn c(self) -> f32 {
        self.0.m21
    }
    pub(crate) fn d(self) -> f32 {
        self.0.m22
    }
    pub(crate) fn e(self) -> f32 {
        self.0.m31
    }
    pub(crate) fn f(self) -> f32 {
        self.0.m32
    }

    pub(crate) fn pdf_components(self) -> [f32; 6] {
        [self.a(), self.b(), self.c(), self.d(), self.e(), self.f()]
    }

    /// Returns whether this 2D matrix can establish a CSS current
    /// transformation matrix. Non-invertible matrices suppress painting.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn is_invertible(self) -> bool {
        let determinant = self.a() * self.d() - self.b() * self.c();
        determinant.is_finite() && determinant != 0.0
    }

    /// Apply this transform to a page-local paint point.
    ///
    /// CSS Transforms maps already-painted geometry into the parent painting
    /// coordinate system. Keeping the input and output as [`PaintPoint`]
    /// prevents transform effects from crossing into layout top-edge or PDF
    /// user-space coordinates by accident:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_point(self, point: PaintPoint) -> PaintPoint {
        self.0.transform_point(point)
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

#[cfg(test)]
mod paint_transform_tests {
    use super::*;

    #[test]
    fn typed_affine_composition_keeps_css_matrix_order() {
        let transform = PaintTransform::translate(PaintTranslation::new(10.0, 20.0))
            .multiply(PaintTransform::scale(2.0, 3.0));

        // CSS matrix multiplication applies the scale first and then the
        // translation: T(10, 20) · S(2, 3) · (1, 2) = (12, 26).
        assert_eq!(
            transform.apply_point(PaintPoint::new(1.0, 2.0)),
            PaintPoint::new(12.0, 26.0)
        );
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

/// Retained union of disjoint rectangular clip regions. Table cells rarely
/// span many visible row fragments; preserving up to 8 regions avoids
/// approximating an interior collapsed row as a single bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintClipUnion {
    clips: [PaintClip; Self::MAX_REGIONS],
    len: u8,
}

impl PaintClipUnion {
    pub(crate) const MAX_REGIONS: usize = 8;

    pub(crate) fn from_clips(clips: &[PaintClip]) -> Option<Self> {
        let first = *clips.first()?;
        let mut union = Self {
            clips: [first; Self::MAX_REGIONS],
            len: 0,
        };
        for clip in clips.iter().copied().take(Self::MAX_REGIONS) {
            union.clips[usize::from(union.len)] = clip;
            union.len += 1;
        }
        Some(union)
    }

    pub(crate) fn clips(&self) -> &[PaintClip] {
        &self.clips[..usize::from(self.len)]
    }

    fn translated(mut self, offset: PaintTranslation) -> Self {
        for clip in &mut self.clips[..usize::from(self.len)] {
            *clip = clip.translated(offset);
        }
        self
    }
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

    pub(in crate::document) fn translated(self, offset: PaintTranslation) -> Self {
        Self::from_paint_rect(offset.transform_rect(&self.rect))
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        self.rect
            .intersection(&other.rect)
            .map(Self::from_paint_rect)
    }

    /// Exact closed-edge containment for an axis-aligned paint rectangle.
    ///
    /// This intentionally does not use a layout epsilon: the caller uses it
    /// to decide whether a PDF clip can be omitted without changing edge
    /// coverage.
    pub(crate) fn contains(self, other: Self) -> bool {
        other.x() >= self.x()
            && other.y() >= self.y()
            && other.x() + other.width() <= self.x() + self.width()
            && other.y() + other.height() <= self.y() + self.height()
    }
}

#[cfg(test)]
mod paint_effect_tests {
    use super::*;

    #[test]
    fn transform_establishes_the_coordinate_system_before_overflow_clip() {
        let effects = PaintEffects {
            transform: Some(PaintTransform::scale(2.0, 2.0)),
            overflow_clip: Some(PaintClip::new(10.0, 20.0, 30.0, 40.0)),
            ..PaintEffects::default()
        };

        assert!(matches!(
            effects.ordered_steps().as_slice(),
            [PaintEffectStep::Transform(_), PaintEffectStep::Clip(_)]
        ));
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
