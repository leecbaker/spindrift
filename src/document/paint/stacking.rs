use crate::document::Page;

use super::annotations::RenderedLink;
use super::display_list::{PaintBand, PaintBandList, recorded_context_paint_bounds};
use super::effects::PaintEffects;
use super::fragments::PaintFragment;
use super::geometry::{PaintClip, PaintTransform, PaintTranslation};
use super::page::PaintPrimitive;

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
pub(crate) struct PaintStackingContext {
    pub(crate) source_order: usize,
    pub(crate) stack_level: StackLevel,
    /// Keep this context's materialized PDF paint separate from adjacent
    /// contexts even when their CSS paint values happen to match. Fragment
    /// replay uses the boundary so independently clipped continuations retain
    /// their own device-pixel edge coverage.
    pub(crate) pdf_paint_boundary: bool,
    pub(crate) bands: PaintBandList,
    pub(crate) effects: PaintEffects,
    pub(crate) bounds: Option<PaintClip>,
}

impl PaintStackingContext {
    pub(in crate::document) fn root() -> Self {
        Self {
            source_order: 0,
            stack_level: StackLevel::Auto,
            pdf_paint_boundary: false,
            bands: PaintBandList::default(),
            effects: PaintEffects::default(),
            bounds: None,
        }
    }

    /// Build a stacking-context node for an independently painted fragment.
    ///
    /// CSS 2.2 Appendix E requires descendant stacking contexts to be painted
    /// atomically inside the parent stack level:
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn new(
        z_index: i32,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        Self::new_with_stack_level(StackLevel::from_z_index(z_index), content, child_contexts)
    }

    pub(crate) fn new_with_stack_level(
        stack_level: StackLevel,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = PaintBandList::default();
        bands.extend_band(
            PaintBand::BackgroundBorder,
            content.display_list.bands.into_items_in_order(),
        );
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(stack_level, bands)
    }

    /// Build a stacking-context node for an independently painted fragment
    /// while preserving the fragment's internal CSS paint bands.
    ///
    /// CSS Positioned Layout assigns the outer stack level in the parent
    /// context, but CSS 2.2 Appendix E still applies recursively inside that
    /// positioned context:
    /// <https://www.w3.org/TR/css-position-3/#painting-order> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn from_banded_fragment_with_stack_level(
        stack_level: StackLevel,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = content.display_list.bands;
        if matches!(stack_level, StackLevel::Integer(_)) {
            let in_flow = std::mem::take(&mut bands.bands[PaintBand::InFlowBlock.index()]);
            bands
                .bands
                .get_mut(PaintBand::BackgroundBorder.index())
                .expect("background band index should exist")
                .extend(in_flow);
        }
        for link in content.links {
            bands.push_link(PaintBand::Inline, link);
        }
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(stack_level, bands)
    }

    /// Build an atomic stacking-context node while preserving the fragment's
    /// existing paint-band structure.
    ///
    /// CSS Transforms and CSS CssColor opacity create stacking contexts whose
    /// descendants still follow CSS 2.2 Appendix E inside the isolated group:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering> and
    /// <https://www.w3.org/TR/css-color-4/#transparency>.
    pub(crate) fn from_banded_fragment(
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = content.display_list.bands;
        for link in content.links {
            bands.push_link(PaintBand::Inline, link);
        }
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(StackLevel::Auto, bands)
    }

    pub(in crate::document) fn with_bands(stack_level: StackLevel, bands: PaintBandList) -> Self {
        Self {
            source_order: 0,
            stack_level,
            pdf_paint_boundary: false,
            bands,
            effects: PaintEffects::default(),
            bounds: None,
        }
    }

    pub(crate) fn with_source_order(mut self, source_order: usize) -> Self {
        self.source_order = source_order;
        self
    }

    pub(crate) fn with_effects(mut self, effects: PaintEffects) -> Self {
        self.effects = effects;
        self
    }

    pub(crate) fn with_bounds(mut self, bounds: PaintClip) -> Self {
        self.bounds = Some(bounds);
        self
    }

    pub(crate) fn with_links(mut self, links: Vec<RenderedLink>) -> Self {
        for link in links {
            self.bands.push_link(PaintBand::Inline, link);
        }
        self
    }

    /// Return bounds after context-level clipping and transforms.
    ///
    /// CSS applies overflow/absolute clipping in the context's local coordinate
    /// space and then maps the painted result through transforms. PDF opacity
    /// groups need a Form XObject `/BBox` covering that composed output:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>,
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>, and
    /// ISO 32000-1:2008 §11.6.6.
    pub(crate) fn effect_bounds(&self, fallback: PaintClip) -> PaintClip {
        let mut bounds = self.bounds.unwrap_or(fallback);
        for clip in [
            self.effects.absolute_clip,
            self.effects.overflow_clip_bounds(),
        ]
        .into_iter()
        .flatten()
        {
            bounds =
                bounds
                    .intersect(clip)
                    .unwrap_or(PaintClip::new(bounds.x(), bounds.y(), 0.0, 0.0));
        }
        if let Some(transform) = self.effects.transform {
            bounds = transform.apply_clip_to_aabb(bounds);
        }
        bounds
    }

    /// Return the recorded ink bounds when every nested paint effect is a
    /// rectangular, axis-aligned operation.
    ///
    /// Fragmentainer replay uses this only to reject source slices that cannot
    /// contain any ink. Effects whose bounds cannot be established exactly are
    /// intentionally reported as indeterminate so callers conservatively keep
    /// every candidate.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(crate) fn recorded_paint_bounds(
        &self,
        page: &Page,
    ) -> std::result::Result<Option<PaintClip>, ()> {
        recorded_context_paint_bounds(
            page,
            &self.effects,
            PaintBand::ORDER
                .into_iter()
                .flat_map(|band| self.bands.bands[band.index()].iter()),
        )
    }

    pub(in crate::document) fn push_flattened_primitives(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
    ) {
        self.bands.push_flattened_primitives(primitives);
    }

    pub(crate) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        self.bands.for_each_flattened_primitive(f);
    }

    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            pdf_paint_boundary: self.pdf_paint_boundary,
            bands: self.bands.translated(offset),
            // Effects are expressed in the same page-top coordinate space as
            // the band's primitives.  Keeping them in place would detach an
            // overflow or containment clip from an escaped inline fragment.
            effects: self.effects.translated(offset),
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
        }
    }

    /// Trim materialized primitive geometry to a fragmentation projection
    /// without flattening the stacking-context tree.
    ///
    /// The accompanying rectangular effect remains necessary for paths,
    /// images, glyphs, and deferred page operations whose exact ink cannot be
    /// reduced to a rectangle here.  Rectangular primitives are nevertheless
    /// trimmed before PDF serialization so a fragmentainer edge has the same
    /// coverage as an independently painted fragment rather than a PDF clip
    /// seam.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(crate) fn sliced_primitives_to_fragmentainer_rect(mut self, clip: PaintClip) -> Self {
        self.bands = self.bands.sliced_primitives_to_fragmentainer_rect(clip);
        self.bounds = self.bounds.and_then(|bounds| bounds.intersect(clip));
        self
    }

    /// Whether all retained items have concrete rectangular bounds within a
    /// clip. Callers may elide an additional PDF clip only in this narrow
    /// case: deferred operations and nested effect scopes intentionally keep
    /// the clip because their final ink is not known here.
    pub(crate) fn has_only_items_wholly_contained_by_rect(&self, clip: PaintClip) -> bool {
        let mut has_item = false;
        for band in PaintBand::ORDER {
            for item in &self.bands.bands[band.index()] {
                has_item = true;
                if !item.is_wholly_contained_by_rect(clip) {
                    return false;
                }
            }
        }
        has_item
    }

    /// Whether a rectangular overflow clip can be omitted after all retained
    /// primitive geometry has been cut to its effective bounds.
    ///
    /// This deliberately rejects every other effect. A transform, rounded
    /// clip, mask, filter, or nested effect scope can change final ink after
    /// local rectangle inspection, so its overflow clip remains authoritative.
    pub(crate) fn can_elide_overflow_clip_after_materialization(
        &self,
        effective_clip: PaintClip,
    ) -> bool {
        let mut effects_without_overflow_clip = self.effects.clone();
        effects_without_overflow_clip.overflow_clip_effect = None;
        effects_without_overflow_clip
            == PaintEffects {
                opacity: self.effects.opacity,
                ..PaintEffects::default()
            }
            && self.has_only_items_wholly_contained_by_rect(effective_clip)
    }

    pub(in crate::document) fn into_recorded_nodes(self, page: &mut Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            pdf_paint_boundary: self.pdf_paint_boundary,
            bands: self.bands.into_recorded_nodes(page),
            effects: self.effects.clone(),
            bounds: self.bounds,
        }
    }

    pub(crate) fn into_primitive_nodes(self, page: &Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            pdf_paint_boundary: self.pdf_paint_boundary,
            bands: self.bands.primitive_node_copy(page),
            effects: self.effects,
            bounds: self.bounds,
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            pdf_paint_boundary: self.pdf_paint_boundary,
            bands: self.bands.primitive_node_copy(page),
            effects: self.effects.clone(),
            bounds: self.bounds,
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        parent_transform: PaintTransform,
        clip: Option<PaintClip>,
        links: &mut Vec<RenderedLink>,
    ) {
        if self.effects.suppresses_paint() {
            return;
        }
        let transform = if let Some(transform) = self.effects.transform {
            parent_transform.multiply(transform)
        } else {
            parent_transform
        };
        let clip = match (
            clip,
            self.effects
                .scene_plane_clip
                .map(|clip| transform.apply_clip_to_aabb(clip.bounds())),
        ) {
            (Some(left), Some(right)) => left.intersect(right),
            (Some(clip), None) | (None, Some(clip)) => Some(clip),
            (None, None) => None,
        };
        self.bands.push_transformed_links(transform, clip, links);
    }
}
