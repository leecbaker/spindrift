use crate::document::Page;

use super::annotations::RenderedLink;
use super::effects::{PaintBlendMode, PaintEffectScope, PaintEffects, PaintFragmentation};
use super::fragments::PaintFragment;
use super::geometry::{PaintClip, PaintTransform, PaintTranslation};
use super::page::{PaintOperation, PaintPrimitive};
use super::stacking::PaintStackingContext;

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
                PaintBand::ORDER
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
/// Fragment-local CSS display list used before primitives are flattened into a page stream.
///
/// CSS painting order is a tree of stacking contexts, not just a page-wide
/// sequence. CSS 2.2 Appendix E defines the recursive stacking order, and CSS
/// Positioned Layout defines positioned boxes with stack levels. Quire
/// stores that recursive structure in captured fragments, then flattens it to
/// PDF drawing operators because PDF content streams paint sequentially
/// (ISO 32000-1:2008, §8.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintDisplayList {
    pub(in crate::document) bands: PaintBandList,
}

impl PaintDisplayList {
    pub(in crate::document) fn from_primitives(primitives: Vec<PaintPrimitive>) -> Self {
        let mut bands = PaintBandList::default();
        for primitive in primitives {
            let band = if matches!(
                primitive,
                PaintPrimitive::Line(_) | PaintPrimitive::OpaqueTextCoverage { .. }
            ) {
                PaintBand::Inline
            } else {
                PaintBand::InFlowBlock
            };
            bands.extend_band(band, [PaintDisplayItem::Primitive(primitive)]);
        }
        Self { bands }
    }

    pub(in crate::document) fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    pub(in crate::document) fn flattened_primitives(&self) -> Vec<PaintPrimitive> {
        let mut primitives = Vec::new();
        self.push_flattened_primitives(&mut primitives);
        primitives
    }

    pub(in crate::document) fn push_flattened_primitives(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
    ) {
        self.bands.push_flattened_primitives(primitives);
    }

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        self.bands.for_each_flattened_primitive(f);
    }

    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        Self {
            bands: self.bands.translated(offset),
        }
    }

    pub(in crate::document) fn into_recorded_nodes(self, page: &mut Page) -> Self {
        Self {
            bands: self.bands.into_recorded_nodes(page),
        }
    }

    pub(in crate::document) fn with_links(
        mut self,
        band: PaintBand,
        links: Vec<RenderedLink>,
    ) -> Self {
        self.bands
            .extend_band(band, links.into_iter().map(PaintDisplayItem::Link));
        self
    }
}

/// Page-level durable paint tree used as the private CSS paint-order source.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PagePaintTree {
    pub(crate) root: PaintStackingContext,
}
