use crate::document::Page;

use super::annotations::RenderedLink;
use super::display_list::{PaintBand, PaintBandList};
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
    pub(crate) bands: PaintBandList,
    pub(crate) effects: PaintEffects,
    pub(crate) bounds: Option<PaintClip>,
}

impl PaintStackingContext {
    pub(in crate::document) fn root() -> Self {
        Self {
            source_order: 0,
            stack_level: StackLevel::Auto,
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
        for clip in [self.effects.absolute_clip, self.effects.overflow_clip]
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
            bands: self.bands.translated(offset),
            // Effects are expressed in the same page-top coordinate space as
            // the band's primitives.  Keeping them in place would detach an
            // overflow or containment clip from an escaped inline fragment.
            effects: self.effects.translated(offset),
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
        }
    }

    pub(in crate::document) fn into_recorded_nodes(self, page: &mut Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.into_recorded_nodes(page),
            effects: self.effects.clone(),
            bounds: self.bounds,
        }
    }

    pub(in crate::document) fn into_primitive_nodes(self, page: &Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.primitive_node_copy(page),
            effects: self.effects,
            bounds: self.bounds,
        }
    }

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.primitive_node_copy(page),
            effects: self.effects.clone(),
            bounds: self.bounds,
        }
    }

    pub(in crate::document) fn push_transformed_links(
        &self,
        parent_transform: PaintTransform,
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
        self.bands.push_transformed_links(transform, links);
    }
}
