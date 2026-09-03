use super::annotations::RenderedLink;
use super::contours::{OverflowClipEffect, ResolvedBoxContentClip};
use super::display_list::PaintDisplayItem;
use super::geometry::{
    Affine3dPaintTransform, AxisSelectivePaintClip, PaintClip, PaintClipUnion, PaintPoint,
    PaintTransform, PaintTranslation, Projective3dPaintTransform,
};
use super::page::{PaintOperation, PaintPrimitive};
use super::paths::RenderedPathClip;
use crate::document::Page;

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
            // This scope is introduced by anonymous fragmentation machinery,
            // not by a CSS box, so it must never introduce a `flat` boundary
            // inside a 3D rendering context.
            effects: PaintEffects {
                three_d_participation: ThreeDParticipation::TransparentLayoutBridge,
                ..PaintEffects::default()
            },
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
        clip: Option<PaintClip>,
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
        let clip = intersect_link_clip(
            clip,
            self.effects
                .scene_plane_clip
                .map(|clip| transform.apply_clip_to_aabb(clip.bounds())),
        );
        for item in &self.items {
            item.push_transformed_links(transform, clip, links);
        }
    }
}

fn intersect_link_clip(left: Option<PaintClip>, right: Option<PaintClip>) -> Option<PaintClip> {
    match (left, right) {
        (Some(left), Some(right)) => left.intersect(right),
        (Some(clip), None) | (None, Some(clip)) => Some(clip),
        (None, None) => None,
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
    /// The local affine 3D matrix retained until a `preserve-3d` rendering
    /// context chooses its final page-plane transform.
    pub(crate) affine_3d_transform: Option<Affine3dPaintTransform>,
    /// A non-affine 3D projection retained until projective primitive lowering.
    /// This is intentionally separate from the affine fast path because PDF
    /// content streams cannot express it as a graphics-state CTM.
    pub(crate) projective_3d_transform: Option<Projective3dPaintTransform>,
    /// A `perspective` property applies to children, rather than to the
    /// perspective element's own plane.
    pub(crate) descendant_projective_3d_transform: Option<Projective3dPaintTransform>,
    pub(crate) three_d_participation: ThreeDParticipation,
    /// The computed `backface-visibility` choice. Its final visibility is
    /// resolved against the accumulated 3D matrix, not this local matrix.
    pub(crate) hide_backface: bool,
    /// A non-invertible CSS transform suppresses the entire transformed
    /// element, including its descendants and links.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>
    pub(crate) suppress_paint: bool,
    /// CSS Overflow clipping, retained independently from CSS `clip-path`.
    ///
    /// This is deliberately a single logical effect: a contoured edge carries
    /// its conservative bounds with the exact path, while rectangular,
    /// axis-selective, and fragmented-table-union cases preserve their own
    /// geometry without formatter-specific side channels.
    pub(crate) overflow_clip_effect: Option<OverflowClipEffect>,
    pub(crate) absolute_clip: Option<PaintClip>,
    /// Internal convex fragment clip emitted when Newell ordering splits a
    /// 3D scene plane. It composes with authored clips rather than replacing
    /// them.
    pub(crate) scene_plane_clip: Option<RenderedClipPathPolygon>,
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
            affine_3d_transform: None,
            projective_3d_transform: None,
            descendant_projective_3d_transform: None,
            three_d_participation: ThreeDParticipation::Flat,
            hide_backface: false,
            suppress_paint: false,
            overflow_clip_effect: None,
            absolute_clip: None,
            scene_plane_clip: None,
            clip_path: PaintClipPathEffect::None,
            mask: PaintMaskEffect::None,
            filter: PaintFilterEffect::None,
            blend_mode: PaintBlendMode::Normal,
            isolation: false,
        }
    }
}

/// An element's role while paint records are assembled into a CSS 3D scene.
///
/// Anonymous layout boxes use `TransparentLayoutBridge` so table fixup and
/// block-in-inline wrappers cannot accidentally create an element-level
/// `flat` boundary.
/// <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ThreeDParticipation {
    #[default]
    Flat,
    Preserve3d,
    TransparentLayoutBridge,
}

impl PaintEffects {
    /// An internally introduced overflow scope carries an authored clip but
    /// is not itself a CSS box. It must not introduce an extra used-`flat`
    /// boundary into an enclosing 3D rendering context.
    /// <https://drafts.csswg.org/css-transforms-2/#grouping-property-values>
    pub(crate) fn transparent_overflow_scope(effect: OverflowClipEffect) -> Self {
        Self {
            overflow_clip_effect: Some(effect),
            three_d_participation: ThreeDParticipation::TransparentLayoutBridge,
            ..Self::default()
        }
    }

    pub(crate) fn is_effect_free_transparent_layout_bridge(&self) -> bool {
        self.three_d_participation == ThreeDParticipation::TransparentLayoutBridge
            && self.opacity >= 1.0
            && self.transform.is_none()
            && self.affine_3d_transform.is_none()
            && self.projective_3d_transform.is_none()
            && self.descendant_projective_3d_transform.is_none()
            && !self.hide_backface
            && !self.suppress_paint
            && self.overflow_clip_effect.is_none()
            && self.absolute_clip.is_none()
            && self.scene_plane_clip.is_none()
            && !self.clip_path.is_active()
            && !self.mask.is_active()
            && !self.filter.is_active()
            && self.blend_mode == PaintBlendMode::Normal
            && !self.isolation
    }
    /// Conservative bounds for one non-union overflow effect.  This is for
    /// culling and fragmentation only; painting must retain the exact enum.
    pub(crate) fn overflow_clip_bounds(&self) -> Option<PaintClip> {
        match self.overflow_clip_effect.as_ref()? {
            OverflowClipEffect::Rect(clip) => Some(*clip),
            OverflowClipEffect::AxisSelective(clip) => Some(clip.bounds()),
            OverflowClipEffect::Union(_) => None,
            OverflowClipEffect::Contoured(clip) => Some(clip.bounds),
        }
    }

    pub(crate) fn set_rectangular_overflow_clip(&mut self, clip: Option<PaintClip>) {
        self.overflow_clip_effect = clip.map(OverflowClipEffect::Rect);
    }

    /// Restrict an already-retained non-union overflow edge to a replay or
    /// fragmentainer rectangle while preserving its semantic edge kind.
    pub(crate) fn intersect_overflow_clip_bounds(&mut self, clip: PaintClip) {
        self.overflow_clip_effect = match self.overflow_clip_effect.take() {
            Some(OverflowClipEffect::Rect(existing)) => {
                existing.intersect(clip).map(OverflowClipEffect::Rect)
            }
            Some(OverflowClipEffect::AxisSelective(existing)) => {
                existing.bounds().intersect(clip).map(|bounds| {
                    OverflowClipEffect::AxisSelective(AxisSelectivePaintClip::new(
                        bounds,
                        existing.clips_x(),
                        existing.clips_y(),
                    ))
                })
            }
            Some(OverflowClipEffect::Contoured(mut existing)) => {
                existing.bounds = existing.bounds.intersect(clip).unwrap_or(PaintClip::new(
                    clip.x(),
                    clip.y(),
                    0.0,
                    0.0,
                ));
                Some(OverflowClipEffect::Contoured(existing))
            }
            // A fragmented table union cannot be compressed to one
            // rectangle. Its owning replay keeps the union scope; callers
            // that need an additional outer fragmentainer clip must wrap it.
            Some(OverflowClipEffect::Union(existing)) => Some(OverflowClipEffect::Union(existing)),
            None => Some(OverflowClipEffect::Rect(clip)),
        };
    }

    /// Remove only CSS Overflow effects while preserving independent absolute
    /// clipping, CSS `clip-path`, masks, filters, and transforms. Atomic
    /// replaced-element scopes use this when their content primitive owns the
    /// exact content edge and the enclosing scope also contains decoration.
    pub(crate) fn clear_overflow_clip_effects(&mut self) {
        self.overflow_clip_effect = None;
    }

    pub(crate) const fn suppresses_paint(&self) -> bool {
        self.suppress_paint
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.overflow_clip_effect = self.overflow_clip_effect.map(|effect| match effect {
            OverflowClipEffect::Rect(clip) => OverflowClipEffect::Rect(clip.translated(offset)),
            OverflowClipEffect::AxisSelective(clip) => {
                OverflowClipEffect::AxisSelective(clip.translated(offset))
            }
            OverflowClipEffect::Union(clips) => OverflowClipEffect::Union(clips.translated(offset)),
            OverflowClipEffect::Contoured(clip) => {
                OverflowClipEffect::Contoured(clip.translated(offset))
            }
        });
        self.absolute_clip = self.absolute_clip.map(|clip| clip.translated(offset));
        self.scene_plane_clip = self.scene_plane_clip.map(|clip| clip.translated(offset));
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
            || self.filter.requires_compositing_group()
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
        if let Some(effect) = &self.overflow_clip_effect {
            match effect {
                OverflowClipEffect::Rect(clip) => steps.push(PaintEffectStep::Clip(*clip)),
                OverflowClipEffect::AxisSelective(clip) => {
                    steps.push(PaintEffectStep::AxisSelectiveClip(*clip))
                }
                OverflowClipEffect::Union(clips) => steps.push(PaintEffectStep::ClipUnion(*clips)),
                OverflowClipEffect::Contoured(clip) => {
                    steps.push(PaintEffectStep::ContouredOverflowClip(clip.clone()))
                }
            }
        }
        if let Some(clip) = self.scene_plane_clip {
            steps.push(PaintEffectStep::ScenePlaneClip(Box::new(clip)));
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
    #[expect(
        dead_code,
        reason = "CSS path() parsing is retained separately from box contour clipping"
    )]
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
/// CSS Masking allows image and generated-image masks. Spindrift currently records
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
/// [`Exact`](Self::Exact) is the narrow set of sRGB color transforms that can
/// be distributed across ordinary source-over paint without a raster surface.
/// All other valid filter values intentionally retain their grouping behavior
/// as [`RequiresRasterBackend`](Self::RequiresRasterBackend).
/// <https://www.w3.org/TR/filter-effects-1/#FilterProperty>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaintFilterEffect {
    None,
    Exact(crate::css::ExactFilterLowering),
    RequiresRasterBackend,
    WillChange,
}

impl PaintFilterEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }

    /// CSS filter activity establishes paint-tree semantics independently of
    /// whether its exact output needs a PDF transparency group.
    pub(crate) fn requires_compositing_group(self) -> bool {
        match self {
            Self::None => false,
            Self::Exact(lowering) => !lowering.is_visual_identity(),
            Self::RequiresRasterBackend | Self::WillChange => true,
        }
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
    ContouredOverflowClip(ResolvedBoxContentClip),
    ScenePlaneClip(Box<RenderedClipPathPolygon>),
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

    /// Conservative axis-aligned bounds of a retained local scene fragment.
    /// PDF annotations are rectangles, so clipping a split link annotation to
    /// this box is the most precise representation available at that boundary.
    pub(in crate::document) fn bounds(&self) -> PaintClip {
        let points = self.points();
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
        PaintClip::new(min_x, min_y, max_x - min_x, max_y - min_y)
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
    use super::{PaintEffectStep, PaintEffects, PaintFilterEffect, RenderedClipPathPolygon};
    use crate::css::{BoundedSrgbColorTransform, ExactFilterLowering, UnitFilterAmount};
    use crate::document::paint::geometry::{PaintClip, PaintPoint, PaintTransform};

    #[test]
    fn transform_establishes_the_coordinate_system_before_overflow_clip() {
        let effects = PaintEffects {
            transform: Some(PaintTransform::scale(2.0, 2.0)),
            overflow_clip_effect: Some(super::OverflowClipEffect::Rect(PaintClip::new(
                10.0, 20.0, 30.0, 40.0,
            ))),
            ..PaintEffects::default()
        };

        assert!(matches!(
            effects.ordered_steps().as_slice(),
            [PaintEffectStep::Transform(_), PaintEffectStep::Clip(_)]
        ));
    }

    #[test]
    fn scene_plane_polygon_bounds_cover_every_vertex() {
        let polygon = RenderedClipPathPolygon::new(&[
            PaintPoint::new(-5.0, 7.0),
            PaintPoint::new(3.0, -2.0),
            PaintPoint::new(11.0, 4.0),
        ])
        .expect("three points form a polygon");

        assert_eq!(polygon.bounds(), PaintClip::new(-5.0, -2.0, 16.0, 9.0));
    }

    #[test]
    fn identity_filter_remains_active_without_a_transparency_group() {
        let filter = PaintFilterEffect::Exact(ExactFilterLowering {
            color: BoundedSrgbColorTransform::IDENTITY,
            alpha: UnitFilterAmount::ONE,
        });
        let effects = PaintEffects {
            filter,
            ..PaintEffects::default()
        };

        assert!(filter.is_active());
        assert!(!filter.requires_compositing_group());
        assert!(!effects.needs_group());
    }

    #[test]
    fn nonidentity_exact_filter_still_requires_a_transparency_group() {
        let filter = PaintFilterEffect::Exact(ExactFilterLowering {
            color: BoundedSrgbColorTransform::grayscale(UnitFilterAmount::ONE),
            alpha: UnitFilterAmount::ONE,
        });

        assert!(filter.requires_compositing_group());
    }
}
