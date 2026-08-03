use crate::document::Page;

use super::annotations::RenderedLink;
use super::display_list::PaintDisplayItem;
use super::geometry::{
    AxisSelectivePaintClip, PaintClip, PaintClipUnion, PaintPoint, PaintTransform, PaintTranslation,
};
use super::page::{PaintOperation, PaintPrimitive};
use super::paths::RenderedPathClip;
use super::shapes::RenderedRoundedRect;

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

    pub(crate) fn contains_monolithic_fragmentation(&self) -> bool {
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintEffects {
    pub(crate) opacity: f32,
    pub(crate) transform: Option<PaintTransform>,
    /// A non-invertible CSS transform suppresses the entire transformed
    /// element, including its descendants and links.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>
    pub(crate) suppress_paint: bool,
    pub(crate) overflow_clip: Option<PaintClip>,
    /// Overflow clip retained without collapsing a visible physical axis into
    /// a finite rectangle. Generic clips continue to use `overflow_clip`.
    pub(crate) axis_selective_overflow_clip: Option<AxisSelectivePaintClip>,
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
            axis_selective_overflow_clip: None,
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
        self.axis_selective_overflow_clip = self
            .axis_selective_overflow_clip
            .map(|clip| clip.translated(offset));
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
        if let Some(clip) = self.axis_selective_overflow_clip {
            steps.push(PaintEffectStep::AxisSelectiveClip(clip));
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
    AxisSelectiveClip(AxisSelectivePaintClip),
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

#[cfg(test)]
mod tests {
    use super::{PaintEffectStep, PaintEffects};
    use crate::document::paint::geometry::{PaintClip, PaintTransform};

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
