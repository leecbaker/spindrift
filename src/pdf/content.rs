use std::collections::{BTreeMap, HashMap};

use pdf_writer::types::{ColorSpaceOperand, LineCapStyle, LineJoinStyle, TextRenderingMode};
use pdf_writer::{Content, Name, Str, TextStr};

use super::colors::{
    PdfBlendColorSpace, PdfColorMode, PdfLoweringColorPolicy, output_color, set_fill_color,
    set_stroke_color,
};
use super::*;
use crate::document::paint::geometry::PdfSize;
use crate::document::paint::paths::{
    RenderedGradient, RenderedPathCommandPoints, RenderedPathLineCap, RenderedPathLineJoin,
    RenderedPathPaint, RenderedPathPaintOrder,
};

/// PDF stroke width used to emulate a matched synthetic bold face.
///
/// It is expressed in the CSS font's em space so shaping and layout remain
/// untouched; only the emitted glyph ink changes.
const SYNTHETIC_BOLD_STROKE_EM: f32 = 1.0 / 24.0;

/// Apply a Filter Effects lowering in encoded sRGB immediately before the
/// PDF output conversion.  This is the color-space boundary required by
/// Filter Effects Level 1, rather than a matrix in whichever ICC space the
/// source CSS color happened to use.
fn filtered_color(
    color: crate::CssColor,
    transform: crate::css::BoundedSrgbColorTransform,
) -> crate::CssColor {
    // A filter-free scope must retain the CSS color until the PDF output
    // boundary chooses its final calibrated space. Converting even an
    // identity transform through sRGB here clips wide-gamut samples before
    // `PdfColorPlan` can select Display-P3.
    // <https://www.w3.org/TR/css-color-4/#color-conversion>
    if transform == crate::css::BoundedSrgbColorTransform::IDENTITY {
        return color;
    }
    crate::color::apply_bounded_srgb_filter_transform(color, transform)
}

fn set_filtered_fill_color(
    content: &mut Content,
    color: crate::CssColor,
    color_mode: PdfColorMode,
    transform: crate::css::BoundedSrgbColorTransform,
) {
    set_fill_color(content, filtered_color(color, transform), color_mode);
}

fn set_filtered_stroke_color(
    content: &mut Content,
    color: crate::CssColor,
    color_mode: PdfColorMode,
    transform: crate::css::BoundedSrgbColorTransform,
) {
    set_stroke_color(content, filtered_color(color, transform), color_mode);
}

pub(super) struct PageContentRenderInputs<'a> {
    pub(super) resources: &'a mut PdfResourceRegistry,
    pub(super) color_policy: &'a PdfLoweringColorPolicy,
    pub(super) image_resources: &'a [PreparedImageResource],
    pub(super) page_image_sources: &'a [PlannedImageIndex],
    pub(super) page_svg_pattern_image_sources: &'a [PlannedImageIndex],
    pub(super) page_image_pattern_sources: &'a [PlannedImageIndex],
    pub(super) raster_resolution_dppx: f32,
}

pub(super) fn page_content_render<'a>(
    page: &crate::Page,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    inputs: PageContentRenderInputs<'a>,
) -> PageContentRender {
    let mut content = Content::new();
    let mut forms = Vec::new();
    let mut vector_paints = VectorPaintResources::with_page_images(
        page.images.as_slice(),
        inputs.page_image_sources,
        page.svg_pattern_images.as_slice(),
        inputs.page_svg_pattern_image_sources,
        inputs.raster_resolution_dppx,
    );
    let mut state = PaintTreeRenderState {
        resources: inputs.resources,
        forms: &mut forms,
        next_form_name: 1,
        form_dependency_scopes: Vec::new(),
        root_form_dependencies: BTreeMap::new(),
        vector_paints: &mut vector_paints,
        // Current page paint coordinates map identically to unrotated PDF
        // user space. Construct the typed PDF extent once at that boundary.
        page_size: PdfSize::new(page.width(), page.height()),
        color_mode: inputs.color_policy.mode(),
        color_policy: inputs.color_policy,
        image_resources: inputs.image_resources,
        page_image_sources: inputs.page_image_sources,
        page_image_pattern_sources: inputs.page_image_pattern_sources,
        active_paint_transform: crate::document::paint::geometry::PaintTransform::identity(),
        active_filter_color_transform: crate::css::BoundedSrgbColorTransform::IDENTITY,
    };
    // PDF only has affine 2D graphics-state transforms. Resolve retained
    // CSS 3D rendering contexts at this backend boundary, after layout has
    // completed and before the display list is consumed.
    let mut paint_tree = page.paint_tree().clone();
    paint_tree.resolve_affine_3d_contexts(page);
    write_paint_tree(&mut content, page, &paint_tree, embedded_fonts, &mut state);
    PageContentRender {
        stream: PdfStreamProgram {
            bytes: content.finish().into_vec(),
            resource_uses: PdfStreamResourceUses {
                xobjects: state
                    .root_form_dependencies
                    .into_iter()
                    .map(|(name, reference)| (name, PdfXObjectHandle::Form(reference.id)))
                    .collect(),
                ..PdfStreamResourceUses::default()
            },
            resolved_resources: None,
        },
        form_xobjects: forms,
        gradient_patterns: vector_paints.plans,
        gradient_tiling_patterns: vector_paints.tilings,
        svg_tiling_patterns: vector_paints.svg_tilings,
        svg_path_tiling_patterns: vector_paints.svg_path_tilings,
    }
}

fn write_paint_tree(
    content: &mut Content,
    page: &crate::Page,
    tree: &crate::document::paint::display_list::PagePaintTree,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    write_stacking_context(content, page, &tree.root, embedded_fonts, state, true);
}

struct PaintTreeRenderState<'a, 'b> {
    resources: &'a mut PdfResourceRegistry,
    forms: &'b mut Vec<FormXObjectRender>,
    next_form_name: usize,
    form_dependency_scopes: Vec<BTreeMap<String, FormXObjectReference>>,
    root_form_dependencies: BTreeMap<String, FormXObjectReference>,
    vector_paints: &'b mut VectorPaintResources,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_policy: &'a PdfLoweringColorPolicy,
    image_resources: &'a [PreparedImageResource],
    page_image_sources: &'a [PlannedImageIndex],
    page_image_pattern_sources: &'a [PlannedImageIndex],
    /// The CSS effect CTM active in the current PDF content stream. Tiling
    /// pattern matrices are page resources, so they must retain this mapping
    /// rather than assuming their tile origin is always page-aligned.
    active_paint_transform: crate::document::paint::geometry::PaintTransform,
    /// An exact Filter Effects color transform inherited from isolated
    /// ancestor groups.  CSS filters operate in sRGB before outer clips,
    /// masks, and opacity; this state is therefore changed only while writing
    /// the group's Form XObject contents.
    active_filter_color_transform: crate::css::BoundedSrgbColorTransform,
}

impl PaintTreeRenderState<'_, '_> {
    fn push_exact_filter(
        &mut self,
        filter: crate::css::ExactFilterLowering,
    ) -> crate::css::UnitFilterAmount {
        self.active_filter_color_transform = self.active_filter_color_transform.then(filter.color);
        filter.alpha
    }
}

impl PaintTreeRenderState<'_, '_> {
    /// Reserve a unique Form resource name before recursively emitting its
    /// contents. Nested effect groups must not derive names from the number of
    /// completed forms, because their parent has not been recorded yet.
    fn reserve_transparency_form(&mut self) -> FormXObjectReference {
        let reference = FormXObjectReference {
            id: self.resources.form(),
            name: format!("Fm{}", self.next_form_name),
        };
        self.next_form_name += 1;
        reference
    }

    fn begin_form_dependencies(&mut self) {
        self.form_dependency_scopes.push(BTreeMap::new());
    }

    fn finish_form_dependencies(&mut self) -> PdfStreamResourceUses {
        PdfStreamResourceUses {
            xobjects: (self
                .form_dependency_scopes
                .pop()
                .expect("form dependency scope is active"))
            .into_iter()
            .map(|(name, reference)| (name, PdfXObjectHandle::Form(reference.id)))
            .collect(),
            ..PdfStreamResourceUses::default()
        }
    }

    fn record_form_dependency(&mut self, reference: FormXObjectReference) {
        if let Some(dependencies) = self.form_dependency_scopes.last_mut() {
            dependencies.insert(reference.name.clone(), reference);
        } else {
            self.root_form_dependencies
                .insert(reference.name.clone(), reference);
        }
    }
}

#[derive(Default)]
struct VectorPaintResources {
    plans: Vec<GradientPatternPlan>,
    gradients: BTreeMap<String, GradientPaintResource>,
    tilings: Vec<GradientTilingPatternPlan>,
    svg_tilings: Vec<SvgTilingPatternPlan>,
    svg_path_tilings: Vec<SvgPathTilingPatternPlan>,
    image_indexes: HashMap<ImageResourceSource, PlannedImageIndex>,
    raster_resolution_dppx: f32,
}

impl VectorPaintResources {
    fn with_page_images(
        images: &[crate::document::paint::images::RenderedImage],
        indexes: &[PlannedImageIndex],
        svg_pattern_images: &[crate::document::paint::images::RenderedImage],
        svg_pattern_indexes: &[PlannedImageIndex],
        raster_resolution_dppx: f32,
    ) -> Self {
        Self {
            image_indexes: images
                .iter()
                .zip(indexes)
                .map(|(image, index)| {
                    (
                        crate::pdf::resources::image_source(image, raster_resolution_dppx),
                        *index,
                    )
                })
                .chain(
                    svg_pattern_images
                        .iter()
                        .zip(svg_pattern_indexes)
                        .map(|(image, index)| {
                            (
                                crate::pdf::resources::image_source(image, raster_resolution_dppx),
                                *index,
                            )
                        }),
                )
                .collect(),
            plans: Vec::new(),
            gradients: BTreeMap::new(),
            tilings: Vec::new(),
            svg_tilings: Vec::new(),
            svg_path_tilings: Vec::new(),
            raster_resolution_dppx,
        }
    }

    fn svg_path_tiling_resource(
        &mut self,
        pattern: &crate::document::paint::paths::RenderedSvgPathPattern,
        resources: &mut PdfResourceRegistry,
        color_mode: PdfColorMode,
    ) -> String {
        let name = format!("SVP{}", self.svg_path_tilings.len() + 1);
        let id = resources.pattern();
        self.svg_path_tilings.push(SvgPathTilingPatternPlan {
            id,
            name: name.clone(),
            pattern: pattern.clone(),
            stream: svg_path_pattern_tile_content(
                pattern,
                color_mode,
                &self.image_indexes,
                self.raster_resolution_dppx,
            ),
        });
        name
    }

    fn tiling_gradient_resource(
        &mut self,
        pattern: &crate::document::paint::patterns::RenderedGradientPattern,
        resources: &mut PdfResourceRegistry,
        page_size: PdfSize,
        color_transform: crate::css::BoundedSrgbColorTransform,
    ) -> String {
        let gradient = transformed_gradient(&pattern.gradient, color_transform);
        let shading = self.gradient_resource(&gradient, resources, page_size);
        let name = format!("GP{}", self.tilings.len() + 1);
        let id = resources.pattern();
        let mut content = Content::new();
        content.save_state();
        if let Some(alpha_gstate_name) = &shading.alpha_gstate_name {
            content.set_parameters(pdf_name(alpha_gstate_name));
        }
        content
            .set_fill_color_space(ColorSpaceOperand::Pattern)
            .set_fill_pattern([], pdf_name(&shading.pattern_name))
            .rect(
                0.0,
                0.0,
                pattern.tiling.tile_size.width,
                pattern.tiling.tile_size.height,
            )
            .fill_nonzero()
            .restore_state();
        self.tilings.push(GradientTilingPatternPlan {
            id,
            name: name.clone(),
            shading_pattern_name: shading.pattern_name.clone(),
            alpha_gstate_name: shading.alpha_gstate_name.clone(),
            pattern: pattern.clone(),
            stream: PdfStreamProgram {
                bytes: content.finish().into_vec(),
                resource_uses: PdfStreamResourceUses {
                    patterns: [(
                        shading.pattern_name,
                        PdfPatternResourceHandle::Dynamic(shading.pattern_id),
                    )]
                    .into(),
                    ext_gstates: shading
                        .alpha_gstate_name
                        .into_iter()
                        .zip(shading.alpha_gstate_id)
                        .map(|(name, handle)| (name, PdfExtGStateResourceHandle::Dynamic(handle)))
                        .collect(),
                    ..PdfStreamResourceUses::default()
                },
                resolved_resources: None,
            },
        });
        name
    }
}

#[derive(Clone)]
struct GradientPaintResource {
    pattern_name: String,
    pattern_id: PdfPatternHandle,
    alpha_gstate_name: Option<String>,
    alpha_gstate_id: Option<PdfExtGStateHandle>,
}

impl VectorPaintResources {
    fn gradient_resource(
        &mut self,
        gradient: &RenderedGradient,
        resources: &mut PdfResourceRegistry,
        page_size: PdfSize,
    ) -> GradientPaintResource {
        let key = gradient_key(gradient);
        if let Some(resource) = self.gradients.get(&key) {
            return resource.clone();
        }
        let name = format!("SG{}", self.plans.len() + 1);
        let id = resources.pattern();
        let function_count = if gradient.periodic.is_some() || gradient.stops.len() == 2 {
            1
        } else {
            gradient.stops.len()
        };
        let function_ids = (0..function_count).map(|_| resources.function()).collect();
        let alpha = gradient.has_transparent_stop().then(|| {
            let pattern_id = resources.pattern();
            let alpha_function_ids = (0..function_count).map(|_| resources.function()).collect();
            let form_id = resources.form();
            let ext_gstate_id = resources.ext_gstate();
            let pattern_name = format!("SGA{}", self.plans.len() + 1);
            let mut content = Content::new();
            content
                .set_fill_color_space(ColorSpaceOperand::Pattern)
                .set_fill_pattern([], pdf_name(&pattern_name))
                .rect(0.0, 0.0, page_size.width, page_size.height)
                .fill_nonzero();
            GradientAlphaPlan {
                pattern_id,
                pattern_name: pattern_name.clone(),
                function_ids: alpha_function_ids,
                form_id,
                ext_gstate_id,
                ext_gstate_name: format!("GSsvgAlpha{}", self.plans.len() + 1),
                page_size,
                stream: PdfStreamProgram {
                    bytes: content.finish().into_vec(),
                    resource_uses: PdfStreamResourceUses {
                        patterns: [(pattern_name, PdfPatternResourceHandle::Dynamic(pattern_id))]
                            .into(),
                        ..PdfStreamResourceUses::default()
                    },
                    resolved_resources: None,
                },
            }
        });
        let resource = GradientPaintResource {
            pattern_name: name.clone(),
            pattern_id: id,
            alpha_gstate_name: alpha.as_ref().map(|alpha| alpha.ext_gstate_name.clone()),
            alpha_gstate_id: alpha.as_ref().map(|alpha| alpha.ext_gstate_id),
        };
        self.plans.push(GradientPatternPlan {
            id,
            name,
            function_ids,
            gradient: gradient.clone(),
            alpha,
        });
        self.gradients.insert(key, resource.clone());
        resource
    }
}

fn gradient_key(gradient: &RenderedGradient) -> String {
    format!("{gradient:?}")
}

fn transformed_gradient(
    gradient: &RenderedGradient,
    transform: crate::css::BoundedSrgbColorTransform,
) -> RenderedGradient {
    let mut gradient = gradient.clone();
    let transform_stop = |stop: &mut crate::document::paint::paths::RenderedGradientStop| {
        stop.color = filtered_color(stop.color, transform);
    };
    gradient.stops.iter_mut().for_each(transform_stop);
    if let Some(periodic) = &mut gradient.periodic {
        periodic.stops.iter_mut().for_each(transform_stop);
    }
    gradient.color_space = crate::css::CssColorSpace::Srgb;
    gradient
}

fn write_stacking_context(
    content: &mut Content,
    page: &crate::Page,
    context: &crate::document::paint::stacking::PaintStackingContext,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
    is_page_root: bool,
) {
    if context.effects.suppresses_paint() {
        return;
    }
    if context.effects.needs_group() {
        write_effect_group(content, page, context, embedded_fonts, state);
        return;
    }
    let items = crate::document::paint::display_list::PaintBand::ORDER
        .into_iter()
        .flat_map(|band| context.bands.bands[band.index()].iter().cloned())
        .collect::<Vec<_>>();
    let elide_redundant_rect_clips = false;
    let effect_steps = context
        .effects
        .ordered_steps()
        .into_iter()
        .filter(|step| {
            !(elide_redundant_rect_clips
                && matches!(
                    step,
                    crate::document::paint::effects::PaintEffectStep::Clip(_)
                ))
        })
        .collect::<Vec<_>>();
    let scoped = !effect_steps.is_empty();
    if scoped {
        content.save_state();
    }
    let parent_paint_transform = state.active_paint_transform;
    for step in effect_steps {
        match step {
            crate::document::paint::effects::PaintEffectStep::Clip(clip)
                if !clip_is_page_media_box(clip, state) =>
            {
                write_rect_clip(content, clip)
            }
            crate::document::paint::effects::PaintEffectStep::AxisSelectiveClip(clip) => {
                write_axis_selective_clip(content, clip, state)
            }
            crate::document::paint::effects::PaintEffectStep::ClipUnion(clips) => {
                write_rect_union_clip(content, clips.clips())
            }
            crate::document::paint::effects::PaintEffectStep::ContouredOverflowClip(clip) => {
                write_rect_clip(content, clip.bounds);
                match &clip.contour {
                    crate::document::paint::contours::BoxContentContour::Rect => {}
                    crate::document::paint::contours::BoxContentContour::Rounded(rounded) => {
                        write_rounded_clip(content, rounded)
                    }
                    crate::document::paint::contours::BoxContentContour::Path(path) => {
                        write_rendered_path_clip(content, path)
                    }
                    crate::document::paint::contours::BoxContentContour::Empty => {
                        write_rect_clip(
                            content,
                            crate::document::paint::geometry::PaintClip::new(
                                clip.bounds.x(),
                                clip.bounds.y(),
                                0.0,
                                0.0,
                            ),
                        );
                    }
                }
            }
            crate::document::paint::effects::PaintEffectStep::ScenePlaneClip(clip) => {
                write_polygon_clip(content, &clip);
            }
            crate::document::paint::effects::PaintEffectStep::ClipPath(clip) => match clip {
                crate::document::paint::effects::PaintClipPathEffect::Polygon(clip) => {
                    write_polygon_clip(content, &clip);
                }
                crate::document::paint::effects::PaintClipPathEffect::Path(clip) => {
                    write_rendered_path_clip(content, &clip);
                }
                _ => {}
            },
            crate::document::paint::effects::PaintEffectStep::Transform(transform) => {
                content.transform(transform.pdf_components());
                state.active_paint_transform = state.active_paint_transform.multiply(transform);
            }
            crate::document::paint::effects::PaintEffectStep::Clip(_)
            | crate::document::paint::effects::PaintEffectStep::Filter(_)
            | crate::document::paint::effects::PaintEffectStep::Mask(_)
            | crate::document::paint::effects::PaintEffectStep::Opacity(_)
            | crate::document::paint::effects::PaintEffectStep::Blend(_)
            | crate::document::paint::effects::PaintEffectStep::Isolation => {}
        }
    }
    // The page-root Appendix-E tree may contain recursive effect-free
    // contexts. Their descendants still participate in the root's final
    // composited output, so the opaque-coverage proof must traverse those
    // contexts rather than requiring a flat primitive list.
    let cull_hidden_opaque_background = is_page_root
        || (!context.effects.ordered_steps().is_empty()
            && (effects_are_rectangular_clips_only(&context.effects)
                || effects_are_axis_selective_clips_only(&context.effects)));
    // The root's Appendix-E bands establish a complete paint order even when
    // their entries are recursive stacking contexts rather than direct PDF
    // primitives. `write_display_items_with_pending_rects` carries each
    // ancestor's later sibling list into that recursive serialization, and
    // its coverage collector accepts only effect-free (or fully accounted
    // rectangular-clip) descendants. That is sufficient proof for invisible
    // text coverage, but not for general background culling: backgrounds can
    // have retained paint significance even when their pixels are covered.
    //
    // CSS 2.2 Appendix E: <https://www.w3.org/TR/CSS22/zindex.html>.
    // ISO 32000-2:2020, 9.3.6 defines the invisible-text realization.
    let allow_opaque_text_coverage_elision =
        opaque_text_coverage_elision_allowed_in_context(is_page_root, &context.effects);
    write_display_items(
        content,
        page,
        &items,
        embedded_fonts,
        state,
        // The retained page paint tree keeps every authored primitive. At
        // this final PDF-realization boundary, an opaque rectangle wholly
        // obscured by later opaque rectangular paint has no observable
        // composited output. Eliding only that underpaint avoids PDF edge
        // antialiasing sampling a CSS-hidden backdrop.
        cull_hidden_opaque_background,
        allow_opaque_text_coverage_elision,
    );
    if scoped {
        content.restore_state();
    }
    state.active_paint_transform = parent_paint_transform;
}

fn write_effect_scope(
    content: &mut Content,
    page: &crate::Page,
    scope: &crate::document::paint::effects::PaintEffectScope,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    if scope.effects.suppresses_paint() {
        return;
    }
    if scope.effects.needs_group() {
        write_effect_scope_group(content, page, scope, embedded_fonts, state);
        return;
    }
    let elide_redundant_rect_clips = false;
    let effect_steps = scope
        .effects
        .ordered_steps()
        .into_iter()
        .filter(|step| {
            !(elide_redundant_rect_clips
                && matches!(
                    step,
                    crate::document::paint::effects::PaintEffectStep::Clip(_)
                ))
        })
        .collect::<Vec<_>>();
    let scoped = !effect_steps.is_empty();
    if scoped {
        content.save_state();
    }
    let parent_paint_transform = state.active_paint_transform;
    for step in effect_steps {
        match step {
            crate::document::paint::effects::PaintEffectStep::Clip(clip)
                if !clip_is_page_media_box(clip, state) =>
            {
                write_rect_clip(content, clip)
            }
            crate::document::paint::effects::PaintEffectStep::AxisSelectiveClip(clip) => {
                write_axis_selective_clip(content, clip, state)
            }
            crate::document::paint::effects::PaintEffectStep::ClipUnion(clips) => {
                write_rect_union_clip(content, clips.clips())
            }
            crate::document::paint::effects::PaintEffectStep::ContouredOverflowClip(clip) => {
                write_rect_clip(content, clip.bounds);
                match &clip.contour {
                    crate::document::paint::contours::BoxContentContour::Rect => {}
                    crate::document::paint::contours::BoxContentContour::Rounded(rounded) => {
                        write_rounded_clip(content, rounded)
                    }
                    crate::document::paint::contours::BoxContentContour::Path(path) => {
                        write_rendered_path_clip(content, path)
                    }
                    crate::document::paint::contours::BoxContentContour::Empty => {
                        write_rect_clip(
                            content,
                            crate::document::paint::geometry::PaintClip::new(
                                clip.bounds.x(),
                                clip.bounds.y(),
                                0.0,
                                0.0,
                            ),
                        );
                    }
                }
            }
            crate::document::paint::effects::PaintEffectStep::ScenePlaneClip(clip) => {
                write_polygon_clip(content, &clip);
            }
            crate::document::paint::effects::PaintEffectStep::ClipPath(clip) => match clip {
                crate::document::paint::effects::PaintClipPathEffect::Polygon(clip) => {
                    write_polygon_clip(content, &clip);
                }
                crate::document::paint::effects::PaintClipPathEffect::Path(clip) => {
                    write_rendered_path_clip(content, &clip);
                }
                _ => {}
            },
            crate::document::paint::effects::PaintEffectStep::Transform(transform) => {
                content.transform(transform.pdf_components());
                state.active_paint_transform = state.active_paint_transform.multiply(transform);
            }
            crate::document::paint::effects::PaintEffectStep::Clip(_)
            | crate::document::paint::effects::PaintEffectStep::Filter(_)
            | crate::document::paint::effects::PaintEffectStep::Mask(_)
            | crate::document::paint::effects::PaintEffectStep::Opacity(_)
            | crate::document::paint::effects::PaintEffectStep::Blend(_)
            | crate::document::paint::effects::PaintEffectStep::Isolation => {}
        }
    }
    write_display_items(
        content,
        page,
        &scope.items,
        embedded_fonts,
        state,
        // A wholly in-clip opaque rectangle has the same realized output as
        // its retained geometry. Other effect scopes remain atomic.
        effects_are_rectangular_clips_only(&scope.effects)
            || effects_are_axis_selective_clips_only(&scope.effects),
        opaque_text_coverage_elision_allowed_in_context(false, &scope.effects),
    );
    if scoped {
        content.restore_state();
    }
    state.active_paint_transform = parent_paint_transform;
}

/// Serialize an in-band SVG/CSS effect scope through an isolated Form XObject.
///
/// Group opacity and blend modes apply after all descendants have composed,
/// so painting children one by one would change overlapping SVG geometry.
fn write_effect_scope_group(
    content: &mut Content,
    page: &crate::Page,
    scope: &crate::document::paint::effects::PaintEffectScope,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let reference = state.reserve_transparency_form();
    let bbox = scope
        .bounds
        .unwrap_or(crate::document::paint::geometry::PaintClip::new(
            0.0,
            0.0,
            state.page_size.width,
            state.page_size.height,
        ));
    let mut form_content = Content::new();
    let mut form_scope = scope.clone();
    form_scope.effects = form_scope.effects.without_group_effects();
    let parent_filter_transform = state.active_filter_color_transform;
    let filter_alpha = exact_filter_lowering_for_scope(scope, page)
        .map(|lowering| state.push_exact_filter(lowering));
    state.begin_form_dependencies();
    write_effect_scope(&mut form_content, page, &form_scope, embedded_fonts, state);
    let resource_uses = state.finish_form_dependencies();
    state.active_filter_color_transform = parent_filter_transform;
    state.forms.push(FormXObjectRender {
        id: reference.id,
        name: reference.name.clone(),
        bbox,
        stream: PdfStreamProgram {
            bytes: form_content.finish().into_vec(),
            resource_uses,
            resolved_resources: None,
        },
        kind: PdfFormKind::TransparencyGroup {
            blending_space: PdfBlendColorSpace::Srgb,
        },
    });
    content.save_state();
    let group_alpha = filter_alpha.map_or(scope.effects.opacity, |filter_alpha| {
        scope.effects.opacity * filter_alpha.value()
    });
    if group_alpha < 1.0
        && let Some(resource_name) = paint_opacity_resource_name(group_alpha)
    {
        content.set_parameters(pdf_name(&resource_name));
    }
    if let Some(resource_name) = scope.effects.blend_mode.resource_name() {
        content.set_parameters(pdf_name(&resource_name));
    }
    content.x_object(pdf_name(&reference.name));
    state.record_form_dependency(reference);
    content.restore_state();
}

fn write_effect_group(
    content: &mut Content,
    page: &crate::Page,
    context: &crate::document::paint::stacking::PaintStackingContext,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let reference = state.reserve_transparency_form();
    let bbox = context.effect_bounds(crate::document::paint::geometry::PaintClip::new(
        0.0,
        0.0,
        state.page_size.width,
        state.page_size.height,
    ));
    let mut form_content = Content::new();
    let mut form_context = context.clone();
    form_context.effects = form_context.effects.without_group_effects();
    let parent_filter_transform = state.active_filter_color_transform;
    let filter_alpha = exact_filter_lowering_for_context(context, page)
        .map(|lowering| state.push_exact_filter(lowering));
    state.begin_form_dependencies();
    write_stacking_context(
        &mut form_content,
        page,
        &form_context,
        embedded_fonts,
        state,
        false,
    );
    let resource_uses = state.finish_form_dependencies();
    state.active_filter_color_transform = parent_filter_transform;
    state.forms.push(FormXObjectRender {
        id: reference.id,
        name: reference.name.clone(),
        bbox,
        stream: PdfStreamProgram {
            bytes: form_content.finish().into_vec(),
            resource_uses,
            resolved_resources: None,
        },
        kind: PdfFormKind::TransparencyGroup {
            blending_space: PdfBlendColorSpace::Srgb,
        },
    });
    content.save_state();
    let group_alpha = filter_alpha.map_or(context.effects.opacity, |filter_alpha| {
        context.effects.opacity * filter_alpha.value()
    });
    if group_alpha < 1.0
        && let Some(resource_name) = paint_opacity_resource_name(group_alpha)
    {
        content.set_parameters(pdf_name(&resource_name));
    }
    if let Some(resource_name) = context.effects.blend_mode.resource_name() {
        content.set_parameters(pdf_name(&resource_name));
    }
    content.x_object(pdf_name(&reference.name));
    state.record_form_dependency(reference);
    content.restore_state();
}

/// Decide whether an exact color lowering can be distributed through every
/// paint source in a Filter Effects group.  Anything we cannot prove is
/// ordinary source-over paint is retained as a non-rendered raster filter;
/// emitting it partially would violate filter-group semantics.
fn exact_filter_lowering_for_context(
    context: &crate::document::paint::stacking::PaintStackingContext,
    page: &crate::Page,
) -> Option<crate::css::ExactFilterLowering> {
    let crate::document::paint::effects::PaintFilterEffect::Exact(lowering) =
        context.effects.filter
    else {
        return None;
    };
    (context.effects.blend_mode == crate::document::paint::effects::PaintBlendMode::Normal
        && crate::document::paint::display_list::PaintBand::ORDER
            .into_iter()
            .all(|band| {
                context.bands.bands[band.index()]
                    .iter()
                    .all(|item| filter_lowering_capable_item(item, page))
            }))
    .then_some(lowering)
}

fn exact_filter_lowering_for_scope(
    scope: &crate::document::paint::effects::PaintEffectScope,
    page: &crate::Page,
) -> Option<crate::css::ExactFilterLowering> {
    let crate::document::paint::effects::PaintFilterEffect::Exact(lowering) = scope.effects.filter
    else {
        return None;
    };
    (scope.effects.blend_mode == crate::document::paint::effects::PaintBlendMode::Normal
        && scope
            .items
            .iter()
            .all(|item| filter_lowering_capable_item(item, page)))
    .then_some(lowering)
}

fn filter_lowering_capable_item(
    item: &crate::document::paint::display_list::PaintDisplayItem,
    page: &crate::Page,
) -> bool {
    use crate::document::paint::display_list::PaintDisplayItem;
    use crate::document::paint::effects::{PaintBlendMode, PaintFilterEffect};
    use crate::document::paint::page::{PaintOperation, PaintPrimitive};

    match item {
        PaintDisplayItem::Link(_) => true,
        PaintDisplayItem::Operation(operation) => match operation {
            PaintOperation::Rect(_)
            | PaintOperation::RoundedRect(_)
            | PaintOperation::Stroke(_) => true,
            PaintOperation::Line(index) => page.lines.get(*index).is_some(),
            PaintOperation::Path(index) => page
                .paths
                .get(*index)
                .is_some_and(path_filter_lowering_capable),
            PaintOperation::GradientPattern(_) => true,
            PaintOperation::Image(_)
            | PaintOperation::ImagePattern(_)
            | PaintOperation::SvgPattern(_)
            | PaintOperation::OpaqueTextCoverage(_)
            | PaintOperation::SvgTextOutline(_) => false,
        },
        PaintDisplayItem::Primitive(primitive) => match primitive {
            PaintPrimitive::Rect(_)
            | PaintPrimitive::RoundedRect(_)
            | PaintPrimitive::Stroke(_)
            | PaintPrimitive::Line(_) => true,
            PaintPrimitive::Path(path) => path_filter_lowering_capable(path),
            PaintPrimitive::GradientPattern(_) => true,
            PaintPrimitive::Image(_)
            | PaintPrimitive::ImagePattern(_)
            | PaintPrimitive::ProjectiveRaster(_)
            | PaintPrimitive::SvgPattern(_)
            | PaintPrimitive::OpaqueTextCoverage { .. }
            | PaintPrimitive::SvgTextOutline { .. } => false,
        },
        PaintDisplayItem::StackingContext(context) => {
            context.effects.blend_mode == PaintBlendMode::Normal
                && !matches!(
                    context.effects.filter,
                    PaintFilterEffect::RequiresRasterBackend | PaintFilterEffect::WillChange
                )
                && crate::document::paint::display_list::PaintBand::ORDER
                    .into_iter()
                    .all(|band| {
                        context.bands.bands[band.index()]
                            .iter()
                            .all(|item| filter_lowering_capable_item(item, page))
                    })
        }
        PaintDisplayItem::EffectScope(scope) => {
            scope.effects.blend_mode == PaintBlendMode::Normal
                && !matches!(
                    scope.effects.filter,
                    PaintFilterEffect::RequiresRasterBackend | PaintFilterEffect::WillChange
                )
                && scope
                    .items
                    .iter()
                    .all(|item| filter_lowering_capable_item(item, page))
        }
    }
}

fn path_filter_lowering_capable(path: &crate::document::paint::paths::RenderedPath) -> bool {
    use crate::document::paint::paths::RenderedPathPaint;
    path.fill_paint.as_ref().is_none_or(|paint| {
        matches!(
            paint,
            RenderedPathPaint::Solid(_) | RenderedPathPaint::Gradient(_)
        )
    }) && path.stroke_paint.as_ref().is_none_or(|paint| {
        matches!(
            paint,
            RenderedPathPaint::Solid(_) | RenderedPathPaint::Gradient(_)
        )
    })
}

fn write_display_items(
    content: &mut Content,
    page: &crate::Page,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
    cull_hidden_opaque_background: bool,
    allow_opaque_text_coverage_elision: bool,
) {
    let mut pending_rects = PendingFillRects::default();
    write_display_items_with_pending_rects(
        content,
        page,
        items,
        embedded_fonts,
        state,
        cull_hidden_opaque_background,
        allow_opaque_text_coverage_elision,
        &mut pending_rects,
        &[],
        &[],
    );
    flush_pending_rects(
        content,
        &mut pending_rects,
        state.color_mode,
        state.active_filter_color_transform,
    );
}

/// Serialize display items while retaining adjacent opaque fill rectangles.
///
/// CSS effect-free captured contexts do not introduce a compositing boundary.
/// Keeping compatible fills pending across those structural boundaries emits
/// one PDF path for abutting rectangles, avoiding rasterizer stitching seams
/// along fractional CSS-pixel edges. A context with any effect still flushes
/// first, because clipping, transforms, or transparency change the painting
/// coordinate space or compositing result.
///
/// CSS 2.2 Appendix E defines paint order, rather than a PDF serialization
/// boundary, for effect-free in-flow descendants:
/// <https://www.w3.org/TR/CSS22/zindex.html>
#[allow(clippy::too_many_arguments)]
fn write_display_items_with_pending_rects(
    content: &mut Content,
    page: &crate::Page,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
    cull_hidden_opaque_background: bool,
    allow_opaque_text_coverage_elision: bool,
    pending_rects: &mut PendingFillRects,
    prior_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
) {
    for (item_index, item) in items.iter().enumerate() {
        let can_cull_item = cull_hidden_opaque_background;
        if can_cull_item
            && transformed_opaque_rect_container_is_fully_covered(
                page,
                item,
                &items[item_index + 1..],
                state.color_mode,
            )
        {
            // A transform ordinarily establishes a PDF graphics-state
            // boundary, so it is intentionally not flattened into the
            // surrounding fill batch.  A scope containing exactly one opaque
            // axis-aligned rectangle is different: if later sibling scopes
            // wholly cover its transformed rectangle, CSS compositing makes
            // the scope unobservable.  Omitting that underpaint keeps PDF
            // edge antialiasing from sampling a color which CSS has hidden.
            continue;
        }
        if let Some(coverage) = display_item_opaque_text_coverage(page, item) {
            write_opaque_text_coverage(
                content,
                page,
                coverage,
                items,
                item_index,
                prior_item_lists,
                later_item_lists,
                embedded_fonts,
                state,
                allow_opaque_text_coverage_elision,
                pending_rects,
            );
        } else if let Some(line) = display_item_line(page, item) {
            // CSS paints the later opaque cover over this text, but a PDF
            // rasterizer can still sample the hidden ink at a fractional
            // vector edge. When the retained glyph-ink bounds prove that the
            // line is fully covered in this effect-free scope, retain its
            // text codes and ToUnicode mapping with rendering mode 3 instead
            // of emitting visible ink. CSS 2.2 Appendix E defines the paint
            // order; ISO 32000-2:2020, 9.3.6 defines invisible text.
            let invisible = allow_opaque_text_coverage_elision
                && rendered_line_ink_is_fully_hidden(
                    page,
                    line,
                    items,
                    item_index,
                    later_item_lists,
                    Some(state),
                );
            flush_pending_rects(
                content,
                pending_rects,
                state.color_mode,
                state.active_filter_color_transform,
            );
            write_line_with_visibility(
                content,
                line,
                embedded_fonts,
                state.color_mode,
                state.active_filter_color_transform,
                invisible,
            );
        } else if let Some(rect) = display_item_rect(page, item, state) {
            // Preserve ordinary display-list paint in the PDF. Fully
            // covered fills can be elided only while serializing a clipped
            // scope, where the optimization prevents a duplicated clip edge
            // from showing through antialiasing. Applying it to every
            // opaque rectangle changes the authored stacking-context output
            // (and can discard tagged or otherwise significant content).
            let position = DisplayListPaintPosition {
                items,
                item_index,
                prior_item_lists,
                later_item_lists,
            };
            let rects =
                if rect_is_unpainted_white_canvas(page, position, &rect, state, can_cull_item)
                    || (can_cull_item
                        && rect_is_fully_covered_by_uninterrupted_prior_same_color(
                            page, position, &rect, state,
                        ))
                {
                    Vec::new()
                } else if can_cull_item {
                    visible_rect_after_later_display_rects(
                        page,
                        items,
                        item_index,
                        &rect,
                        later_item_lists,
                        state,
                    )
                } else {
                    vec![rect]
                };
            for rect in rects {
                // Merge based on the actual PDF paint values, not their CSS
                // source coordinates. Equivalent Lab/OKLab and sRGB fills can
                // otherwise serialize as adjacent subpaths and acquire a
                // rasterizer stitching seam at their shared edge.
                let rect = rect_with_output_fill(rect, state.color_mode);
                if is_mergeable_fill_rect(&rect) {
                    if !pending_rects.try_push(&rect) {
                        flush_pending_rects(
                            content,
                            pending_rects,
                            state.color_mode,
                            state.active_filter_color_transform,
                        );
                        let pushed = pending_rects.try_push(&rect);
                        debug_assert!(pushed);
                    }
                } else {
                    flush_pending_rects(
                        content,
                        pending_rects,
                        state.color_mode,
                        state.active_filter_color_transform,
                    );
                    write_rect(
                        content,
                        &rect,
                        state.color_mode,
                        state.active_filter_color_transform,
                    );
                }
            }
        } else if let crate::document::paint::display_list::PaintDisplayItem::StackingContext(
            context,
        ) = item
            && (context.effects == crate::document::paint::effects::PaintEffects::default()
                || effects_are_rectangular_clips_only(&context.effects))
        {
            if context.pdf_paint_boundary {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
            let context_items = crate::document::paint::display_list::PaintBand::ORDER
                .into_iter()
                .flat_map(|band| context.bands.bands[band.index()].iter().cloned())
                .collect::<Vec<_>>();
            let clipped = effects_are_rectangular_clips_only(&context.effects)
                && !rectangular_clips_are_redundant_for_solid_images(
                    page,
                    &context.effects,
                    &context_items,
                    state,
                );
            if clipped {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
                content.save_state();
                write_rectangular_clip_effects(content, &context.effects, state);
            }
            let mut context_later_item_lists =
                Vec::with_capacity(later_item_lists.len().saturating_add(1));
            context_later_item_lists.push(&items[item_index + 1..]);
            context_later_item_lists.extend_from_slice(later_item_lists);
            let mut context_prior_item_lists =
                Vec::with_capacity(prior_item_lists.len().saturating_add(1));
            context_prior_item_lists.push(&items[..item_index]);
            context_prior_item_lists.extend_from_slice(prior_item_lists);
            // The parent lists establish source containment, not a sibling
            // paint order inside this captured context. Coverage culling may
            // use them only once this context has installed a clip that makes
            // its local geometry authoritative.
            let nested_cull_hidden_opaque_background = cull_hidden_opaque_background || clipped;
            write_display_items_with_pending_rects(
                content,
                page,
                &context_items,
                embedded_fonts,
                state,
                nested_cull_hidden_opaque_background,
                allow_opaque_text_coverage_elision,
                pending_rects,
                &context_prior_item_lists,
                &context_later_item_lists,
            );
            if clipped {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
                content.restore_state();
            }
            if context.pdf_paint_boundary {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        } else if let crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) =
            item
            && (scope.effects == crate::document::paint::effects::PaintEffects::default()
                || effects_are_rectangular_clips_only(&scope.effects))
        {
            let clipped = effects_are_rectangular_clips_only(&scope.effects)
                && !rectangular_clips_are_redundant_for_solid_images(
                    page,
                    &scope.effects,
                    &scope.items,
                    state,
                );
            if clipped {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
                content.save_state();
                write_rectangular_clip_effects(content, &scope.effects, state);
            }
            let mut scope_later_item_lists =
                Vec::with_capacity(later_item_lists.len().saturating_add(1));
            scope_later_item_lists.push(&items[item_index + 1..]);
            scope_later_item_lists.extend_from_slice(later_item_lists);
            let mut scope_prior_item_lists =
                Vec::with_capacity(prior_item_lists.len().saturating_add(1));
            scope_prior_item_lists.push(&items[..item_index]);
            scope_prior_item_lists.extend_from_slice(prior_item_lists);
            let nested_cull_hidden_opaque_background = cull_hidden_opaque_background || clipped;
            write_display_items_with_pending_rects(
                content,
                page,
                &scope.items,
                embedded_fonts,
                state,
                nested_cull_hidden_opaque_background,
                allow_opaque_text_coverage_elision,
                pending_rects,
                &scope_prior_item_lists,
                &scope_later_item_lists,
            );
            if clipped {
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
                content.restore_state();
            }
        } else {
            flush_pending_rects(
                content,
                pending_rects,
                state.color_mode,
                state.active_filter_color_transform,
            );
            write_display_item(content, page, item, embedded_fonts, state);
        }
    }
}

/// Deduplicate only a direct earlier same-color fill with no intervening
/// opaque different-color rectangle. A same-color page canvas is not proof of
/// redundancy after a dark background has painted over it.
fn rect_is_fully_covered_by_uninterrupted_prior_same_color(
    page: &crate::Page,
    position: DisplayListPaintPosition<'_>,
    rect: &crate::document::paint::shapes::RenderedRect,
    state: &PaintTreeRenderState<'_, '_>,
) -> bool {
    let rect = rect_with_output_fill(rect.clone(), state.color_mode);
    let mut prior_rects = Vec::new();
    for prior_items in position.prior_item_lists.iter().rev() {
        for item in prior_items.iter() {
            collect_later_opaque_rects(page, item, state, &mut prior_rects);
        }
    }
    for item in &position.items[..position.item_index] {
        collect_later_opaque_rects(page, item, state, &mut prior_rects);
    }
    let mut intervening = Vec::new();
    for previous in prior_rects.iter().rev() {
        if !rects_intersect(previous, &rect) {
            continue;
        }
        if previous.fill == rect.fill {
            // A later same-color rectangle may cover the intervening paint,
            // but that cannot make the earlier same-color fill reusable.
            // Doing so is circular: the intervening fills can be elided only
            // because this rectangle will paint, while this rectangle would
            // then be elided because of the earlier fill. Preserve the later
            // source whenever another opaque color interrupted the sequence.
            return intervening.is_empty()
                && rect_area_is_covered_by_rects(&rect, std::slice::from_ref(previous));
        }
        intervening.push(previous.clone());
    }
    false
}

/// Serialize text whose full-em glyph paths are explicit opaque coverage.
///
/// The line keeps the normal PDF text object (and therefore its `/ToUnicode`
/// extraction data), while `VectorPath` glyphs are emitted in PDF rendering
/// mode 3. When every companion rectangle is covered by later, effect-free
/// opaque coverage, omitting those paths exactly preserves the CSS composited
/// result and prevents PDF antialiasing from sampling hidden ink. PDF text
/// rendering mode 3 is defined by ISO 32000-2:2020, 9.3.6; CSS paint order is
/// defined by CSS 2.2 Appendix E: <https://www.w3.org/TR/CSS22/zindex.html>.
/// A coverage record owns one CSS text-paint operation. Its compatible
/// rectangles can share a PDF fill within that operation, but must not remain
/// pending across another record: coalescing coverage from separate line
/// records changes fractional-edge rasterization even when their geometry and
/// color are identical.
#[allow(clippy::too_many_arguments)]
fn write_opaque_text_coverage(
    content: &mut Content,
    page: &crate::Page,
    coverage: &crate::document::paint::page::OpaqueTextCoverage,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    prior_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
    allow_opaque_text_coverage_elision: bool,
    pending_rects: &mut PendingFillRects,
) {
    let Some(line) = page.lines.get(coverage.line_index) else {
        return;
    };
    if pending_rects.ends_with_invisible_text_coverage {
        // Two distinct CSS text records must not share a fill, even when
        // both use invisible PDF text. A following ordinary rectangle may
        // still merge with the preceding record's opaque glyph coverage.
        flush_pending_rects(
            content,
            pending_rects,
            state.color_mode,
            state.active_filter_color_transform,
        );
    }
    let coverage_fully_hidden = allow_opaque_text_coverage_elision
        && opaque_text_coverage_is_fully_hidden_with_state(
            page,
            coverage,
            items,
            item_index,
            later_item_lists,
            Some(state),
        );
    // This record is created only for opaque vector coverage that reproduces
    // the line's glyph ink. Keep the text codes for extraction, but let the
    // retained paths be its sole visible realization. This avoids applying
    // two fractional-edge coverage rules to the same CSS glyph.
    let coverage_replaces_glyph_ink = coverage.path_indices.iter().all(|path_index| {
        page.paths
            .get(*path_index)
            .is_some_and(|path| path.opaque_coverage_rect.is_some())
    });
    let invisible = coverage_fully_hidden || coverage_replaces_glyph_ink;
    // A visible text record is a paint boundary. An invisible record has no
    // PDF paint, so retaining the pending batch lets equal opaque coverage on
    // either side coalesce into one fill without changing CSS paint output.
    if !invisible {
        flush_pending_rects(
            content,
            pending_rects,
            state.color_mode,
            state.active_filter_color_transform,
        );
    }
    write_line_with_visibility(
        content,
        line,
        embedded_fonts,
        state.color_mode,
        state.active_filter_color_transform,
        invisible,
    );
    if coverage_fully_hidden {
        return;
    }
    for path_index in &coverage.path_indices {
        if allow_opaque_text_coverage_elision
            && opaque_text_coverage_path_matches_prior_rect(
                page,
                *path_index,
                items,
                item_index,
                prior_item_lists,
            )
        {
            continue;
        }
        if let Some(path) = page.paths.get(*path_index) {
            if let Some(rect) = opaque_text_coverage_rect_for_batch(path, state.color_mode) {
                if !pending_rects.try_push(&rect) {
                    flush_pending_rects(
                        content,
                        pending_rects,
                        state.color_mode,
                        state.active_filter_color_transform,
                    );
                    let pushed = pending_rects.try_push(&rect);
                    debug_assert!(pushed, "verified opaque coverage rect is mergeable");
                }
            } else {
                // Do not turn clipped, transformed, patterned, or otherwise
                // non-equivalent paths into page-space rectangles.
                flush_pending_rects(
                    content,
                    pending_rects,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
                write_path(
                    content,
                    path,
                    state.vector_paints,
                    state.resources,
                    state.page_size,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
    }
    // Visible text remains a CSS paint boundary. When this record uses
    // invisible text, the companion opaque paths are its only visible paint,
    // so an equal adjacent fill may remain pending and coalesce safely.
    if !invisible {
        flush_pending_rects(
            content,
            pending_rects,
            state.color_mode,
            state.active_filter_color_transform,
        );
    }
    if invisible && !pending_rects.rects.is_empty() {
        pending_rects.ends_with_invisible_text_coverage = true;
    }
}

/// Return whether an opaque text-coverage path is already supplied by a
/// preceding same-color rectangle in the current display-list scope.
///
/// The companion rectangle remains emitted: rectangle culling explicitly
/// preserves a same-color source when this relationship exists, so these two
/// local optimizations cannot remove both paints.
fn opaque_text_coverage_path_matches_prior_rect(
    page: &crate::Page,
    path_index: usize,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    prior_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
) -> bool {
    let Some(path) = page.paths.get(path_index) else {
        return false;
    };
    let (Some(fill), Some(coverage_rect)) = (path.fill, path.opaque_coverage_rect) else {
        return false;
    };
    let coverage =
        crate::document::paint::shapes::RenderedRect::from_paint_rect(coverage_rect, Some(fill));
    let mut prior_rects = Vec::new();
    for item in &items[..item_index] {
        collect_prior_same_color_opaque_rects(page, item, fill, &mut prior_rects);
    }
    for prior_items in prior_item_lists {
        for item in *prior_items {
            collect_prior_same_color_opaque_rects(page, item, fill, &mut prior_rects);
        }
    }
    rect_area_is_covered_by_rects(&coverage, &prior_rects)
}

fn collect_prior_same_color_opaque_rects(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    color: crate::CssColor,
    rects: &mut Vec<crate::document::paint::shapes::RenderedRect>,
) {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => {
            if let Some(rect) = page.rects.get(*index)
                && is_opaque_fill_rect(rect)
                && rect.fill == Some(color)
            {
                rects.push(rect.clone());
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) if is_opaque_fill_rect(rect) && rect.fill == Some(color) => {
            rects.push(rect.clone());
        }
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context)
            if effects_are_rectangular_clips_only(&context.effects) =>
        {
            for band in crate::document::paint::display_list::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_prior_same_color_opaque_rects(page, child, color, rects);
                }
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope)
            if effects_are_rectangular_clips_only(&scope.effects) =>
        {
            for child in &scope.items {
                collect_prior_same_color_opaque_rects(page, child, color, rects);
            }
        }
        _ => {}
    }
}

/// Convert a verified opaque coverage path to a page-space rectangle when its
/// original path semantics are exactly rectangular.
///
/// The full-em classifier can produce rotated text paths as well. Their
/// page-space bounds are not their coverage, so they retain normal path
/// serialization rather than being widened to a bounding rectangle.
fn opaque_text_coverage_rect_for_batch(
    path: &crate::document::paint::paths::RenderedPath,
    color_mode: PdfColorMode,
) -> Option<crate::document::paint::shapes::RenderedRect> {
    let rect = path.opaque_coverage_rect?;
    let fill = path.fill?;
    if !fill.is_opaque()
        || path.clip.is_some()
        || path.stroke.is_some()
        || !matches!(path.fill_paint, Some(RenderedPathPaint::Solid(_)))
        || path.stroke_paint.is_some()
        || path.paint_order != RenderedPathPaintOrder::FillThenStroke
        || path.transform.b() != 0.0
        || path.transform.c() != 0.0
    {
        return None;
    }
    Some(rect_with_output_fill(
        crate::document::paint::shapes::RenderedRect::from_paint_rect(rect, Some(fill)),
        color_mode,
    ))
}

fn display_item_opaque_text_coverage<'a>(
    page: &'a crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
) -> Option<&'a crate::document::paint::page::OpaqueTextCoverage> {
    let crate::document::paint::display_list::PaintDisplayItem::Operation(
        crate::document::paint::page::PaintOperation::OpaqueTextCoverage(index),
    ) = item
    else {
        return None;
    };
    page.opaque_text_coverages.get(*index)
}

fn display_item_line<'a>(
    page: &'a crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
) -> Option<&'a crate::document::paint::text::RenderedLine> {
    let crate::document::paint::display_list::PaintDisplayItem::Operation(
        crate::document::paint::page::PaintOperation::Line(index),
    ) = item
    else {
        return None;
    };
    page.lines.get(*index)
}

/// Return whether every pixel of a line's known glyph ink has a later,
/// effect-free opaque rectangular cover. Missing ink bounds are intentionally
/// not approximated: only a proven cover may suppress visible PDF glyph ink.
fn rendered_line_ink_is_fully_hidden(
    page: &crate::Page,
    line: &crate::document::paint::text::RenderedLine,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    state: Option<&PaintTreeRenderState<'_, '_>>,
) -> bool {
    if line.color.alpha() < 1.0 {
        return false;
    }
    let Some(bounds) = line.glyph_ink_bounds else {
        return false;
    };
    let mut covers = Vec::new();
    for item in &items[item_index + 1..] {
        collect_later_effect_free_opaque_coverage_paths(page, item, state, &mut covers);
    }
    for later_items in later_item_lists {
        for item in *later_items {
            collect_later_effect_free_opaque_coverage_paths(page, item, state, &mut covers);
        }
    }
    rect_area_is_covered_by_rects(
        &crate::document::paint::shapes::RenderedRect::from_paint_rect(
            bounds.paint_rect(),
            Some(crate::CssColor::BLACK),
        ),
        &covers,
    )
}

#[cfg(test)]
fn opaque_text_coverage_is_fully_hidden(
    page: &crate::Page,
    coverage: &crate::document::paint::page::OpaqueTextCoverage,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
) -> bool {
    opaque_text_coverage_is_fully_hidden_with_state(
        page,
        coverage,
        items,
        item_index,
        later_item_lists,
        None,
    )
}

fn opaque_text_coverage_is_fully_hidden_with_state(
    page: &crate::Page,
    coverage: &crate::document::paint::page::OpaqueTextCoverage,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    state: Option<&PaintTreeRenderState<'_, '_>>,
) -> bool {
    let mut covers = Vec::new();
    for item in &items[item_index + 1..] {
        collect_later_effect_free_opaque_coverage_paths(page, item, state, &mut covers);
    }
    for later_items in later_item_lists {
        for item in *later_items {
            collect_later_effect_free_opaque_coverage_paths(page, item, state, &mut covers);
        }
    }
    coverage.path_indices.iter().all(|path_index| {
        page.paths
            .get(*path_index)
            .and_then(|path| path.opaque_coverage_rect)
            .is_some_and(|rect| {
                rect_area_is_covered_by_rects(
                    &crate::document::paint::shapes::RenderedRect::from_paint_rect(
                        rect,
                        Some(crate::CssColor::BLACK),
                    ),
                    &covers,
                )
            })
    })
}

/// Collect proven opaque path coverage that paints in the same compositing
/// context as a preceding opaque-text replacement. Effects are compositing
/// boundaries, so their descendants cannot establish a later direct cover.
fn collect_later_effect_free_opaque_coverage_paths(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    state: Option<&PaintTreeRenderState<'_, '_>>,
    rects: &mut Vec<crate::document::paint::shapes::RenderedRect>,
) {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => {
            let Some(rect) = page.rects.get(*index) else {
                return;
            };
            if is_opaque_fill_rect(rect) {
                rects.push(rect.clone());
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) if is_opaque_fill_rect(rect) => {
            rects.push(rect.clone());
        }
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Path(index),
        ) => {
            if let Some(rect) = page
                .paths
                .get(*index)
                .and_then(|path| path.opaque_coverage_rect)
            {
                rects.push(
                    crate::document::paint::shapes::RenderedRect::from_paint_rect(
                        rect,
                        Some(crate::CssColor::BLACK),
                    ),
                );
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::OpaqueTextCoverage(index),
        ) => {
            let Some(coverage) = page.opaque_text_coverages.get(*index) else {
                return;
            };
            for path_index in &coverage.path_indices {
                if let Some(rect) = page
                    .paths
                    .get(*path_index)
                    .and_then(|path| path.opaque_coverage_rect)
                {
                    rects.push(
                        crate::document::paint::shapes::RenderedRect::from_paint_rect(
                            rect,
                            Some(crate::CssColor::BLACK),
                        ),
                    );
                }
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Image(_),
        ) => {
            if let Some(rect) = state.and_then(|state| display_item_rect(page, item, state))
                && is_opaque_fill_rect(&rect)
            {
                rects.push(rect);
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context)
            if context.effects == crate::document::paint::effects::PaintEffects::default() =>
        {
            for band in crate::document::paint::display_list::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_later_effect_free_opaque_coverage_paths(page, child, state, rects);
                }
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context)
            if effects_are_rectangular_clips_only(&context.effects) =>
        {
            let Some(clips) = rectangular_clips(&context.effects) else {
                return;
            };
            let mut child_rects = Vec::new();
            for band in crate::document::paint::display_list::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_later_effect_free_opaque_coverage_paths(
                        page,
                        child,
                        state,
                        &mut child_rects,
                    );
                }
            }
            // A rectangular clip is not a compositing boundary, but it does
            // limit the usable opaque coverage. Keep only a child rectangle
            // wholly inside every active clip; partial intersections remain
            // conservative rather than being mistaken for page-space cover.
            rects.extend(
                child_rects
                    .into_iter()
                    .filter(|rect| clips.iter().all(|clip| rect_is_within_clip(rect, *clip))),
            );
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope)
            if scope.effects == crate::document::paint::effects::PaintEffects::default() =>
        {
            for child in &scope.items {
                collect_later_effect_free_opaque_coverage_paths(page, child, state, rects);
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope)
            if effects_are_rectangular_clips_only(&scope.effects) =>
        {
            let Some(clips) = rectangular_clips(&scope.effects) else {
                return;
            };
            let mut child_rects = Vec::new();
            for child in &scope.items {
                collect_later_effect_free_opaque_coverage_paths(
                    page,
                    child,
                    state,
                    &mut child_rects,
                );
            }
            rects.extend(
                child_rects
                    .into_iter()
                    .filter(|rect| clips.iter().all(|clip| rect_is_within_clip(rect, *clip))),
            );
        }
        _ => {}
    }
}

fn rect_with_output_fill(
    mut rect: crate::document::paint::shapes::RenderedRect,
    color_mode: PdfColorMode,
) -> crate::document::paint::shapes::RenderedRect {
    rect.fill = rect.fill.map(|fill| output_color(fill, color_mode));
    rect
}

fn display_item_rect(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    state: &PaintTreeRenderState<'_, '_>,
) -> Option<crate::document::paint::shapes::RenderedRect> {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => page.rects.get(*index).cloned(),
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Image(index),
        ) => {
            let image = page.images.get(*index)?;
            let PreparedImageResource::SolidFill(fill) = state
                .page_image_sources
                .get(*index)
                .and_then(|source| state.image_resources.get(source.0))?
            else {
                return None;
            };
            solid_image_fill_as_rect(image, fill)
        }
        _ => None,
    }
}

/// Return a promoted fill that can share an existing CSS rectangle path.
///
/// An untagged sRGB image whose only clip is its destination rectangle has no
/// image-specific PDF state. Routing it through the adjacent-fill batch lets
/// it join an equal CSS background in one `f` operation, preventing a
/// rasterizer seam at a shared edge. Other promoted images use [`write_image`]
/// so real crops, rounded clips, transforms, alpha, and marked content stay
/// scoped exactly as authored.
fn solid_image_fill_as_rect(
    image: &crate::document::paint::images::RenderedImage,
    fill: &SolidImageFill,
) -> Option<crate::document::paint::shapes::RenderedRect> {
    (fill.color_space == crate::color::RasterColorSpace::SRGB
        && image_clip_is_redundant_destination_rect(image)
        && image.actual_text.is_none()
        && image.transform.is_none())
    .then(|| {
        crate::document::paint::shapes::RenderedRect::from_paint_rect(
            image.paint_rect(),
            Some(crate::CssColor::in_space(
                crate::css::CssColorSpace::Srgb,
                fill.components[0] as f32 / 255.0,
                fill.components[1] as f32 / 255.0,
                fill.components[2] as f32 / 255.0,
                1.0,
            )),
        )
    })
}

/// Remove only the fully obscured portions of an opaque rectangular fill.
///
/// This preserves display-list order while inspecting later paint through
/// effect-free stacking-context boundaries. A context with effects stays an
/// atomic compositing boundary. It prevents PDF antialiasing from sampling a
/// color that CSS compositing has already hidden under a later opaque
/// background.
fn visible_rect_after_later_display_rects(
    page: &crate::Page,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    rect: &crate::document::paint::shapes::RenderedRect,
    later_item_lists: &[&[crate::document::paint::display_list::PaintDisplayItem]],
    state: &PaintTreeRenderState<'_, '_>,
) -> Vec<crate::document::paint::shapes::RenderedRect> {
    if rect.preserves_opaque_backdrop || !is_opaque_fill_rect(rect) {
        return vec![rect.clone()];
    }
    let mut covers = Vec::new();
    for item in &items[item_index + 1..] {
        collect_later_opaque_rects(page, item, state, &mut covers);
    }
    for later_items in later_item_lists {
        for item in *later_items {
            collect_later_opaque_rects(page, item, state, &mut covers);
        }
    }
    // PDF output retains the authored paint primitive unless later opaque
    // painting hides *all* of it. Splitting a partially covered primitive
    // changes the retained display-list geometry (and, in particular, loses
    // the continuous gap-rule path that CSS specifies). The fully-covered
    // case is still safe to elide and avoids an unnecessary underpaint.
    // Same-color coverage is visually redundant in ideal vector compositing,
    // but it is still the correct backdrop for the later path's fractional
    // edge. Keep it so the PDF rasterizer cannot turn a solid CSS fill into a
    // seam against the page canvas; only a different later color is hidden by
    // CSS compositing and must be removed before that edge is sampled.
    let output_rect = rect_with_output_fill(rect.clone(), state.color_mode);
    let different_color_covers = covers
        .iter()
        .filter(|cover| cover.fill != output_rect.fill)
        .cloned()
        .collect::<Vec<_>>();
    let opaque_underpaint_culls = different_color_covers
        .iter()
        .filter(|cover| cover.culls_opaque_underpaint)
        .cloned()
        .collect::<Vec<_>>();
    if !opaque_underpaint_culls.is_empty() {
        return visible_after_later_opaque_rects(rect, opaque_underpaint_culls.iter());
    }
    if rect_area_is_covered_by_rects(rect, &different_color_covers) {
        Vec::new()
    } else {
        vec![rect.clone()]
    }
}

/// Return the only opaque rectangle painted by an axis-aligned transform
/// scope, expressed in the scope's parent paint coordinate system.
///
/// This deliberately excludes clips, transparency, filters, and multi-item
/// scopes.  Those can affect the visible shape or compositing of a descendant
/// even when its untransformed bounds appear rectangular.
fn transformed_opaque_rect_container_rect(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    color_mode: PdfColorMode,
) -> Option<crate::document::paint::shapes::RenderedRect> {
    let (effects, items) = match item {
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context) => (
            &context.effects,
            crate::document::paint::display_list::PaintBand::ORDER
                .into_iter()
                .flat_map(|band| context.bands.bands[band.index()].iter())
                .collect::<Vec<_>>(),
        ),
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
            (&scope.effects, scope.items.iter().collect::<Vec<_>>())
        }
        _ => return None,
    };
    let transform = effects.transform?;
    if !transform.preserves_axis_aligned_rectangles()
        || effects.opacity != 1.0
        || effects.needs_group()
        || effects.suppresses_paint()
        || effects.overflow_clip_effect.is_some()
        || effects.absolute_clip.is_some()
        || !matches!(
            effects.clip_path,
            crate::document::paint::effects::PaintClipPathEffect::None
        )
    {
        return None;
    }
    let [item] = items.as_slice() else {
        return None;
    };
    let rect = match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => page.rects.get(*index)?.clone(),
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) => rect.clone(),
        _ => return None,
    };
    if !is_opaque_fill_rect(&rect) {
        return None;
    }
    if let Some(crate::document::paint::contours::OverflowClipEffect::AxisSelective(clip)) =
        effects.overflow_clip_effect
    {
        let bounds = clip.bounds();
        let rect_bounds = rect.paint_rect();
        if (clip.clips_x()
            && (rect_bounds.min_x() < bounds.x()
                || rect_bounds.max_x() > bounds.x() + bounds.width()))
            || (clip.clips_y()
                && (rect_bounds.min_y() < bounds.y()
                    || rect_bounds.max_y() > bounds.y() + bounds.height()))
        {
            return None;
        }
    }
    let transformed_rect = transform
        .apply_clip_to_aabb(
            crate::document::paint::geometry::PaintClip::from_paint_rect(rect.paint_rect()),
        )
        .paint_rect();
    let mut rect = rect_with_output_fill(rect, color_mode);
    rect.set_paint_rect(transformed_rect);
    Some(rect)
}

/// Whether a one-rectangle transformed scope has no visible composited
/// output because later sibling scopes entirely cover it.
///
/// This only compares siblings in the same display-list coordinate system.
/// Ancestor later lists can be transformed relative to the current scope and
/// are intentionally left to ordinary PDF serialization.
fn transformed_opaque_rect_container_is_fully_covered(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    later_items: &[crate::document::paint::display_list::PaintDisplayItem],
    color_mode: PdfColorMode,
) -> bool {
    let Some(rect) = transformed_opaque_rect_container_rect(page, item, color_mode) else {
        return false;
    };
    let covers = later_items
        .iter()
        .filter_map(|item| transformed_opaque_rect_container_rect(page, item, color_mode))
        .filter(|cover| cover.fill != rect.fill)
        .collect::<Vec<_>>();
    rect_area_is_covered_by_rects(&rect, &covers)
}

fn collect_later_opaque_rects(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    state: &PaintTreeRenderState<'_, '_>,
    rects: &mut Vec<crate::document::paint::shapes::RenderedRect>,
) {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => {
            if let Some(rect) = page.rects.get(*index)
                && is_opaque_fill_rect(rect)
            {
                rects.push(rect_with_output_fill(rect.clone(), state.color_mode));
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Image(index),
        ) => {
            let Some(image) = page.images.get(*index) else {
                return;
            };
            let Some(PreparedImageResource::SolidFill(fill)) = state
                .page_image_sources
                .get(*index)
                .and_then(|source| state.image_resources.get(source.0))
            else {
                return;
            };
            if let Some(rect) = solid_image_fill_as_rect(image, fill) {
                // Later-coverage comparisons must use the PDF-output color.
                // Treating every promoted image as black would incorrectly
                // cull a same-color parent fill instead of deduplicating it.
                rects.push(rect_with_output_fill(rect, state.color_mode));
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) if is_opaque_fill_rect(rect) => {
            // Positioned and replayed fragments retain materialized primitives
            // rather than page-operation indices. They are equally opaque
            // later paint for this effect-free coverage analysis.
            rects.push(rect_with_output_fill(rect.clone(), state.color_mode));
        }
        // Text coverage rectangles approximate glyph ink for the dedicated
        // invisible-text optimization. They are not rectangular CSS paint
        // primitives and therefore cannot prove that an authored background
        // rectangle is fully obscured. Using them here can erase borders
        // crossed by a glyph's conservative ink bounds.
        // <https://www.w3.org/TR/CSS2/zindex.html>
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context)
            if context.effects == crate::document::paint::effects::PaintEffects::default() =>
        {
            for band in crate::document::paint::display_list::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_later_opaque_rects(page, child, state, rects);
                }
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope)
            if scope.effects == crate::document::paint::effects::PaintEffects::default() =>
        {
            for child in &scope.items {
                collect_later_opaque_rects(page, child, state, rects);
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope)
            if effects_are_rectangular_clips_only(&scope.effects) =>
        {
            let Some(clips) = rectangular_clips(&scope.effects) else {
                return;
            };
            let start = rects.len();
            for child in &scope.items {
                collect_later_opaque_rects(page, child, state, rects);
            }
            // A retained rectangle that escapes the clip is not equivalent to
            // the visible clipped cover. Only expose covers already wholly
            // contained by every rectangular clip.
            if !rects[start..]
                .iter()
                .all(|rect| clips.iter().all(|clip| rect_is_within_clip(rect, *clip)))
            {
                rects.truncate(start);
            }
        }
        _ => {}
    }
}

/// A white fill at the start of an effect-free root paint stream is redundant
/// when every preceding opaque rectangle either misses it or is fully hidden
/// by it. PDF pages rasterize against a white canvas, so retaining that fill
/// would otherwise introduce an antialiased white-on-white edge.
#[derive(Clone, Copy)]
struct DisplayListPaintPosition<'a> {
    items: &'a [crate::document::paint::display_list::PaintDisplayItem],
    item_index: usize,
    prior_item_lists: &'a [&'a [crate::document::paint::display_list::PaintDisplayItem]],
    later_item_lists: &'a [&'a [crate::document::paint::display_list::PaintDisplayItem]],
}

fn rect_is_unpainted_white_canvas(
    page: &crate::Page,
    position: DisplayListPaintPosition<'_>,
    rect: &crate::document::paint::shapes::RenderedRect,
    state: &PaintTreeRenderState<'_, '_>,
    cull_fully_hidden_backgrounds: bool,
) -> bool {
    if (!position.later_item_lists.is_empty() && !cull_fully_hidden_backgrounds)
        || rect.fill != Some(crate::CssColor::WHITE)
        || !is_opaque_fill_rect(rect)
    {
        return false;
    }
    position.items[..position.item_index]
        .iter()
        .chain(
            position
                .prior_item_lists
                .iter()
                .flat_map(|items| items.iter()),
        )
        .all(|previous| {
            display_item_rect(page, previous, state).is_some_and(|previous| {
                is_opaque_fill_rect(&previous)
                    && (!rects_intersect(&previous, rect)
                        || (cull_fully_hidden_backgrounds
                            && previous.fill != rect.fill
                            && rect_area_is_covered_by_rects(
                                &previous,
                                std::slice::from_ref(rect),
                            )))
            })
        })
}

/// Return whether rectangular clips cannot affect this paint subtree.
///
/// The serializer preserves display-list order exactly. It may omit a clip
/// only when every painted primitive is an in-bounds rectangle and every
/// nested context has no effect beyond another rectangular clip. This avoids
/// an otherwise-visible PDF rasterizer seam at a redundant clipping edge
/// without transforming, merging, or reordering any paint primitive.
#[allow(dead_code)]
fn display_items_are_rects_within_every_clip(
    page: &crate::Page,
    effects: crate::document::paint::effects::PaintEffects,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
) -> bool {
    let clips = effects
        .ordered_steps()
        .iter()
        .filter_map(|step| match step {
            crate::document::paint::effects::PaintEffectStep::Clip(clip) => Some(*clip),
            _ => None,
        })
        .collect::<Vec<_>>();
    !clips.is_empty()
        && items
            .iter()
            .all(|item| display_item_is_rect_within_active_clips(page, item, &clips))
}

#[allow(dead_code)]
fn display_item_is_rect_within_active_clips(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    clips: &[crate::document::paint::geometry::PaintClip],
) -> bool {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => page
            .rects
            .get(*index)
            .is_some_and(|rect| clips.iter().all(|clip| rect_is_within_clip(rect, *clip))),
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) => clips.iter().all(|clip| rect_is_within_clip(rect, *clip)),
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context) => {
            effects_are_rectangular_clips_only(&context.effects)
                && crate::document::paint::display_list::PaintBand::ORDER
                    .into_iter()
                    .all(|band| {
                        context.bands.bands[band.index()].iter().all(|child| {
                            display_item_is_rect_within_active_clips(page, child, clips)
                        })
                    })
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
            effects_are_rectangular_clips_only(&scope.effects)
                && scope
                    .items
                    .iter()
                    .all(|child| display_item_is_rect_within_active_clips(page, child, clips))
        }
        crate::document::paint::display_list::PaintDisplayItem::Link(_) => true,
        crate::document::paint::display_list::PaintDisplayItem::Operation(_)
        | crate::document::paint::display_list::PaintDisplayItem::Primitive(_) => false,
    }
}

fn effects_are_rectangular_clips_only(
    effects: &crate::document::paint::effects::PaintEffects,
) -> bool {
    !effects.needs_group()
        && effects.ordered_steps().iter().all(|step| {
            matches!(
                step,
                crate::document::paint::effects::PaintEffectStep::Clip(_)
            )
        })
}

/// Axis-selective overflow clips constrain geometry but do not establish a
/// compositing group.  Descendants of such a scope share the same clip, so a
/// later opaque rectangle that geometrically covers an earlier one also
/// covers its clipped realization.
fn effects_are_axis_selective_clips_only(
    effects: &crate::document::paint::effects::PaintEffects,
) -> bool {
    !effects.needs_group()
        && effects.ordered_steps().iter().all(|step| {
            matches!(
                step,
                crate::document::paint::effects::PaintEffectStep::AxisSelectiveClip(_)
            )
        })
}

/// Whether this serialization scope has a complete, direct paint-order proof
/// for opaque text coverage elision.
///
/// The page root's Appendix-E band sequence orders every top-level sibling,
/// including recursive negative and in-flow stacking contexts. Descendant
/// contexts intentionally do not inherit that authority: their parent list
/// may establish containment without proving that arbitrary local paint is
/// later composited coverage. A rectangular clip is separately safe because
/// the coverage collector accounts for the clip before accepting a rectangle.
///
/// <https://www.w3.org/TR/CSS22/zindex.html>
fn opaque_text_coverage_elision_allowed_in_context(
    is_page_root: bool,
    effects: &crate::document::paint::effects::PaintEffects,
) -> bool {
    let has_effect_free_rectangular_clip = !effects.ordered_steps().is_empty()
        && (effects_are_rectangular_clips_only(effects)
            || effects_are_axis_selective_clips_only(effects));
    (is_page_root && *effects == crate::document::paint::effects::PaintEffects::default())
        || has_effect_free_rectangular_clip
}

/// Apply a pure rectangular-clip scope while retaining its descendants in the
/// surrounding paint-order analysis. A clip changes geometry but not
/// compositing, so its later opaque descendants can still prove coverage only
/// after this PDF clipping state is installed.
fn write_rectangular_clip_effects(
    content: &mut Content,
    effects: &crate::document::paint::effects::PaintEffects,
    state: &PaintTreeRenderState<'_, '_>,
) {
    debug_assert!(effects_are_rectangular_clips_only(effects));
    for step in effects.ordered_steps() {
        let crate::document::paint::effects::PaintEffectStep::Clip(clip) = step else {
            continue;
        };
        if !clip_is_page_media_box(clip, state) {
            write_rect_clip(content, clip);
        }
    }
}

/// Whether a rectangular scope has no observable clipping effect because all
/// of its direct contents are promoted solid-image rectangles already wholly
/// inside it. Keeping such a scope would create an artificial pending-fill
/// boundary and a fractional seam between adjacent CSS fills.
fn rectangular_clips_are_redundant_for_solid_images(
    page: &crate::Page,
    effects: &crate::document::paint::effects::PaintEffects,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
    state: &PaintTreeRenderState<'_, '_>,
) -> bool {
    let Some(clips) = rectangular_clips(effects) else {
        return false;
    };
    // The page media box already bounds every page content stream. Repeating
    // that same clip is a no-op, but it would split otherwise mergeable
    // opaque fills into separate PDF graphics states and expose a fractional
    // antialias seam at their shared CSS edge.
    if clips
        .iter()
        .all(|clip| clip_is_page_media_box(*clip, state))
    {
        return true;
    }
    !items.is_empty()
        && items.iter().all(|item| {
            matches!(
                item,
                crate::document::paint::display_list::PaintDisplayItem::Operation(
                    crate::document::paint::page::PaintOperation::Image(_)
                )
            ) && display_item_rect(page, item, state)
                .is_some_and(|rect| clips.iter().all(|clip| rect_is_within_clip(&rect, *clip)))
        })
}

fn rect_is_within_clip(
    rect: &crate::document::paint::shapes::RenderedRect,
    clip: crate::document::paint::geometry::PaintClip,
) -> bool {
    let rect = rect.paint_rect();
    rect.min_x() >= clip.x() - 0.01
        && rect.max_x() <= clip.x() + clip.width() + 0.01
        && rect.min_y() >= clip.y() - 0.01
        && rect.max_y() <= clip.y() + clip.height() + 0.01
}

/// Serialize the special case where containment clips a subtree made entirely
/// of opaque rectangular fills. PDF rasterizers otherwise blend a child edge
/// with an ancestor fill that CSS painting has completely hidden. This is
/// deliberately scoped to a real rectangular clip; normal page and display
/// item serialization always retains authored paint order.
#[allow(dead_code)]
fn contained_opaque_rect_layer(
    page: &crate::Page,
    effects: &crate::document::paint::effects::PaintEffects,
    items: &[crate::document::paint::display_list::PaintDisplayItem],
) -> Option<Vec<crate::document::paint::shapes::RenderedRect>> {
    let clips = rectangular_clips(effects)?;
    if clips.is_empty() {
        return None;
    }
    let mut rects = Vec::new();
    for item in items {
        collect_contained_opaque_rects(page, item, &clips, &mut rects)?;
    }
    Some(rects)
}

#[allow(dead_code)]
fn rectangular_clips(
    effects: &crate::document::paint::effects::PaintEffects,
) -> Option<Vec<crate::document::paint::geometry::PaintClip>> {
    if !effects_are_rectangular_clips_only(effects) {
        return None;
    }
    Some(
        effects
            .ordered_steps()
            .iter()
            .filter_map(|step| match step {
                crate::document::paint::effects::PaintEffectStep::Clip(clip) => Some(*clip),
                _ => None,
            })
            .collect(),
    )
}

#[allow(dead_code)]
fn collect_contained_opaque_rects(
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    ancestor_clips: &[crate::document::paint::geometry::PaintClip],
    rects: &mut Vec<crate::document::paint::shapes::RenderedRect>,
) -> Option<()> {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            crate::document::paint::page::PaintOperation::Rect(index),
        ) => {
            let rect = page.rects.get(*index)?.clone();
            if !is_opaque_fill_rect(&rect)
                || !ancestor_clips
                    .iter()
                    .all(|clip| rect_is_within_clip(&rect, *clip))
            {
                return None;
            }
            rects.push(rect);
        }
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::Rect(rect),
        ) => {
            if !is_opaque_fill_rect(rect)
                || !ancestor_clips
                    .iter()
                    .all(|clip| rect_is_within_clip(rect, *clip))
            {
                return None;
            }
            rects.push(rect.clone());
        }
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context) => {
            let mut clips = ancestor_clips.to_vec();
            clips.extend(rectangular_clips(&context.effects)?);
            for band in crate::document::paint::display_list::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_contained_opaque_rects(page, child, &clips, rects)?;
                }
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
            let mut clips = ancestor_clips.to_vec();
            clips.extend(rectangular_clips(&scope.effects)?);
            for child in &scope.items {
                collect_contained_opaque_rects(page, child, &clips, rects)?;
            }
        }
        crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
        crate::document::paint::display_list::PaintDisplayItem::Operation(_)
        | crate::document::paint::display_list::PaintDisplayItem::Primitive(_) => return None,
    }
    Some(())
}

#[allow(dead_code)]
fn write_contained_opaque_rect_layer(
    content: &mut Content,
    rects: Vec<crate::document::paint::shapes::RenderedRect>,
) {
    let mut visible = Vec::new();
    for (index, rect) in rects.iter().enumerate() {
        visible.extend(visible_after_later_opaque_rects(
            rect,
            rects[index + 1..].iter(),
        ));
    }
    visible.sort_by(|left, right| {
        right
            .y()
            .total_cmp(&left.y())
            .then_with(|| left.x().total_cmp(&right.x()))
    });
    for rect in &visible {
        write_rect(
            content,
            rect,
            PdfColorMode::PreserveCssSpace,
            crate::css::BoundedSrgbColorTransform::IDENTITY,
        );
    }
}

fn visible_after_later_opaque_rects<'a>(
    rect: &crate::document::paint::shapes::RenderedRect,
    later: impl Iterator<Item = &'a crate::document::paint::shapes::RenderedRect>,
) -> Vec<crate::document::paint::shapes::RenderedRect> {
    let mut visible = vec![rect.clone()];
    for cover in later.filter(|cover| is_opaque_fill_rect(cover) && cover.fill != rect.fill) {
        visible = visible
            .into_iter()
            .flat_map(|candidate| subtract_opaque_rect_cover(candidate, cover))
            .collect();
        if visible.is_empty() {
            break;
        }
    }
    visible
}

fn subtract_opaque_rect_cover(
    rect: crate::document::paint::shapes::RenderedRect,
    cover: &crate::document::paint::shapes::RenderedRect,
) -> Vec<crate::document::paint::shapes::RenderedRect> {
    let source = rect.paint_rect();
    let cover = cover.paint_rect();
    let x0 = source.min_x().max(cover.min_x());
    let x1 = source.max_x().min(cover.max_x());
    let y0 = source.min_y().max(cover.min_y());
    let y1 = source.max_y().min(cover.max_y());
    if x1 <= x0 || y1 <= y0 {
        return vec![rect];
    }
    let mut fragments = Vec::with_capacity(4);
    let mut push = |x: f32, y: f32, width: f32, height: f32| {
        if width > 0.0 && height > 0.0 {
            let mut fragment = rect.clone();
            fragment.set_paint_rect(crate::document::paint::geometry::PaintRect::new(
                crate::document::paint::geometry::PaintPoint::new(x, y),
                crate::document::paint::geometry::PaintSize::new(width, height),
            ));
            fragments.push(fragment);
        }
    };
    push(
        source.min_x(),
        source.min_y(),
        source.width(),
        y0 - source.min_y(),
    );
    push(source.min_x(), y1, source.width(), source.max_y() - y1);
    push(source.min_x(), y0, x0 - source.min_x(), y1 - y0);
    push(x1, y0, source.max_x() - x1, y1 - y0);
    fragments
}

fn is_opaque_fill_rect(rect: &crate::document::paint::shapes::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.alpha() >= 1.0)
}

fn write_display_item(
    content: &mut Content,
    page: &crate::Page,
    item: &crate::document::paint::display_list::PaintDisplayItem,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match item {
        crate::document::paint::display_list::PaintDisplayItem::Operation(operation) => {
            write_page_operation(content, page, operation, embedded_fonts, state);
        }
        crate::document::paint::display_list::PaintDisplayItem::StackingContext(context) => {
            write_stacking_context(content, page, context, embedded_fonts, state, false);
        }
        crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
            write_effect_scope(content, page, scope, embedded_fonts, state);
        }
        crate::document::paint::display_list::PaintDisplayItem::Primitive(
            crate::document::paint::page::PaintPrimitive::ProjectiveRaster(raster),
        ) => write_projective_raster(content, page, raster, state),
        crate::document::paint::display_list::PaintDisplayItem::Primitive(_)
        | crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
    }
}

/// Emit the finite viewer-visible portion of a raster plane. PDF cannot carry
/// the CSS projective CTM, so this fallback scopes the existing calibrated
/// raster/pattern paint to the clipped projected polygon. Keeping the source
/// resource name means ordinary PDF resource and ICC planning remains shared
/// with non-projective raster paint.
fn write_projective_raster(
    content: &mut Content,
    page: &crate::Page,
    raster: &crate::document::paint::page::ProjectiveRasterPrimitive,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    use crate::document::paint::page::ProjectiveRasterSource;

    if raster.visible_polygon.len() < 3 {
        return;
    }
    match &raster.source {
        ProjectiveRasterSource::Image(image) => {
            let Some(index) = page.images.iter().position(|candidate| candidate == image) else {
                return;
            };
            let Some(resource) = state
                .page_image_sources
                .get(index)
                .and_then(|source| state.image_resources.get(source.0))
            else {
                return;
            };
            if matches!(resource, PreparedImageResource::Transparent) {
                return;
            }
            content.save_state();
            write_projected_polygon_clip(content, &raster.visible_polygon);
            write_image(content, image, index, resource, state.color_policy);
            content.restore_state();
        }
        ProjectiveRasterSource::ImagePattern(pattern) => {
            let Some(index) = page
                .image_patterns
                .iter()
                .position(|candidate| candidate == pattern)
            else {
                return;
            };
            let Some(resource) = state
                .page_image_pattern_sources
                .get(index)
                .and_then(|source| state.image_resources.get(source.0))
            else {
                return;
            };
            if matches!(resource, PreparedImageResource::Transparent) {
                return;
            }
            write_projected_image_pattern(content, &raster.visible_polygon, index);
        }
    }
}

fn write_projected_polygon_clip(
    content: &mut Content,
    polygon: &[crate::document::paint::geometry::PaintPoint],
) {
    let Some((first, rest)) = polygon.split_first() else {
        return;
    };
    let first = crate::document::paint::geometry::paint_point_to_pdf(*first);
    content.move_to(first.x, first.y);
    for point in rest {
        let point = crate::document::paint::geometry::paint_point_to_pdf(*point);
        content.line_to(point.x, point.y);
    }
    content.close_path().clip_nonzero().end_path();
}

fn write_projected_image_pattern(
    content: &mut Content,
    polygon: &[crate::document::paint::geometry::PaintPoint],
    index: usize,
) {
    let Some((first, rest)) = polygon.split_first() else {
        return;
    };
    let first = crate::document::paint::geometry::paint_point_to_pdf(*first);
    content.save_state();
    content
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(&format!("P{}", index + 1)))
        .move_to(first.x, first.y);
    for point in rest {
        let point = crate::document::paint::geometry::paint_point_to_pdf(*point);
        content.line_to(point.x, point.y);
    }
    content.close_path().fill_nonzero().restore_state();
}

fn write_page_operation(
    content: &mut Content,
    page: &crate::Page,
    operation: &crate::document::paint::page::PaintOperation,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match operation {
        crate::document::paint::page::PaintOperation::Rect(index) => {
            if let Some(rect) = page.rects.get(*index) {
                write_rect(
                    content,
                    rect,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::RoundedRect(index) => {
            if let Some(rect) = page.rounded_rects.get(*index) {
                write_rounded_rect(
                    content,
                    rect,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::Path(index) => {
            if let Some(path) = page.paths.get(*index) {
                write_path(
                    content,
                    path,
                    state.vector_paints,
                    state.resources,
                    state.page_size,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::Stroke(index) => {
            if let Some(stroke) = page.strokes.get(*index) {
                write_stroke(
                    content,
                    stroke,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::Image(index) => {
            if let (Some(image), Some(resource)) = (
                page.images.get(*index),
                state
                    .page_image_sources
                    .get(*index)
                    .and_then(|source| state.image_resources.get(source.0)),
            ) {
                write_image(content, image, *index, resource, state.color_policy);
            }
        }
        crate::document::paint::page::PaintOperation::ImagePattern(index) => {
            if let (Some(pattern), Some(resource)) = (
                page.image_patterns.get(*index),
                state
                    .page_image_pattern_sources
                    .get(*index)
                    .and_then(|source| state.image_resources.get(source.0)),
            ) && !matches!(resource, PreparedImageResource::Transparent)
            {
                write_image_pattern(content, pattern, *index);
            }
        }
        crate::document::paint::page::PaintOperation::GradientPattern(index) => {
            if let Some(pattern) = page.gradient_patterns.get(*index) {
                write_gradient_tiling_pattern(
                    content,
                    pattern,
                    state.vector_paints,
                    state.resources,
                    state.page_size,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::SvgPattern(index) => {
            if let Some(pattern) = page.svg_patterns.get(*index) {
                write_svg_tiling_pattern(content, pattern, state);
            }
        }
        crate::document::paint::page::PaintOperation::Line(index) => {
            if let Some(line) = page.lines.get(*index) {
                write_line(
                    content,
                    line,
                    embedded_fonts,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
        }
        crate::document::paint::page::PaintOperation::OpaqueTextCoverage(index) => {
            let Some(coverage) = page.opaque_text_coverages.get(*index) else {
                return;
            };
            if let Some(line) = page.lines.get(coverage.line_index) {
                write_line(
                    content,
                    line,
                    embedded_fonts,
                    state.color_mode,
                    state.active_filter_color_transform,
                );
            }
            for path_index in &coverage.path_indices {
                if let Some(path) = page.paths.get(*path_index) {
                    write_path(
                        content,
                        path,
                        state.vector_paints,
                        state.resources,
                        state.page_size,
                        state.color_mode,
                        state.active_filter_color_transform,
                    );
                }
            }
        }
        crate::document::paint::page::PaintOperation::SvgTextOutline(index) => {
            let Some(outline) = page.svg_text_outlines.get(*index) else {
                return;
            };
            {
                let mut marked_content =
                    content.begin_marked_content_with_properties(Name(b"Span"));
                marked_content
                    .properties()
                    .actual_text(TextStr(outline.actual_text.as_ref()));
            }
            write_effect_scope(content, page, &outline.content, embedded_fonts, state);
            content.end_marked_content();
        }
    }
}

fn write_rect_clip(content: &mut Content, clip: crate::document::paint::geometry::PaintClip) {
    let rect = crate::document::paint::geometry::paint_rect_to_pdf(clip.paint_rect());
    content
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .clip_nonzero()
        .end_path();
}

fn write_axis_selective_clip(
    content: &mut Content,
    clip: crate::document::paint::geometry::AxisSelectivePaintClip,
    state: &PaintTreeRenderState<'_, '_>,
) {
    let page = crate::document::paint::geometry::PaintClip::new(
        0.0,
        0.0,
        state.page_size.width,
        state.page_size.height,
    );
    // The effect's transform has already been installed. Resolve the CSS
    // visible axis against the page boundary in that local coordinate space;
    // using page-space coordinates here would translate or rotate the
    // synthetic finite PDF rectangle together with the descendants.
    let local_page = state
        .active_paint_transform
        .inverse_apply_clip_to_aabb(page)
        .unwrap_or(page);
    write_rect_clip(content, clip.resolved_against_page(local_page));
}

/// PDF clipping applies the non-zero winding rule to every subpath in one
/// path. Appending the visible table-cell rectangles before `W n` therefore
/// retains their union and removes collapsed rowspan holes in a single scope.
fn write_rect_union_clip(
    content: &mut Content,
    clips: &[crate::document::paint::geometry::PaintClip],
) {
    if clips.is_empty() {
        return;
    }
    for clip in clips {
        let rect = crate::document::paint::geometry::paint_rect_to_pdf(clip.paint_rect());
        content.rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        );
    }
    content.clip_nonzero().end_path();
}

/// PDF page media boxes already clip all page content. Re-emitting an equal
/// CSS viewport clip is redundant and can expose an otherwise hidden
/// underpaint through antialiasing at the duplicate boundary.
fn clip_is_page_media_box(
    clip: crate::document::paint::geometry::PaintClip,
    state: &PaintTreeRenderState<'_, '_>,
) -> bool {
    clip_is_page_bounds(clip, state.page_size)
}

fn clip_is_page_bounds(
    clip: crate::document::paint::geometry::PaintClip,
    page_size: PdfSize,
) -> bool {
    nearly_equal(clip.x(), 0.0)
        && nearly_equal(clip.y(), 0.0)
        && nearly_equal(clip.width(), page_size.width)
        && nearly_equal(clip.height(), page_size.height)
}

/// Emit a PDF clipping path for a rounded CSS padding edge.
///
/// CSS paint containment clips descendant ink to the curved padding edge;
/// PDF applies the equivalent path with `W n` inside the captured paint
/// scope: <https://www.w3.org/TR/css-contain-1/#containment-paint> and ISO
/// 32000-1:2008, 8.5.4 "Clipping Path Operators".
fn write_rounded_clip(content: &mut Content, clip: &RenderedRoundedRect) {
    write_rounded_rect_path(content, clip);
    content.clip_nonzero().end_path();
}

fn write_polygon_clip(
    content: &mut Content,
    polygon: &crate::document::paint::effects::RenderedClipPathPolygon,
) {
    let Some((&first, rest)) = polygon.points().split_first() else {
        return;
    };
    let first = crate::document::paint::geometry::paint_point_to_pdf(first);
    content.move_to(first.x, first.y);
    for point in rest {
        let point = crate::document::paint::geometry::paint_point_to_pdf(*point);
        content.line_to(point.x, point.y);
    }
    content.close_path().clip_nonzero().end_path();
}

fn is_mergeable_fill_rect(rect: &crate::document::paint::shapes::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.is_visible())
}

#[derive(Default)]
struct PendingFillRects {
    rects: Vec<crate::document::paint::shapes::RenderedRect>,
    ends_with_invisible_text_coverage: bool,
}

impl PendingFillRects {
    fn try_push(&mut self, rect: &crate::document::paint::shapes::RenderedRect) -> bool {
        if !is_mergeable_fill_rect(rect) {
            return false;
        }
        let Some(first_fill) = self.rects.first().and_then(|first| first.fill) else {
            self.rects.push(rect.clone());
            return true;
        };
        if Some(first_fill) != rect.fill {
            return false;
        }
        for pending in &mut self.rects {
            if merge_adjacent_fill_rect(pending, rect) {
                return true;
            }
        }
        // Repainting the same opaque color over the exact same rectangle has
        // no CSS-visible effect. Emit it once so a PDF rasterizer cannot
        // compound fractional edge coverage from nested backgrounds.
        if self
            .rects
            .iter()
            .any(|pending| pending.paint_rect() == rect.paint_rect())
        {
            return true;
        }
        if self
            .rects
            .iter()
            .any(|pending| rects_intersect(pending, rect))
        {
            // A single nonzero fill of overlapping opaque rectangles is their
            // geometric union. Keeping it in this batch preserves CSS's
            // composited output while preventing PDF rasterizers from
            // antialiasing a shared fractional edge against an earlier fill.
            if first_fill.is_opaque() {
                self.rects.push(rect.clone());
                return true;
            }
            return false;
        }
        self.rects.push(rect.clone());
        true
    }
}

fn flush_pending_rects(
    content: &mut Content,
    pending: &mut PendingFillRects,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    let Some(fill) = pending.rects.first().and_then(|rect| rect.fill) else {
        pending.ends_with_invisible_text_coverage = false;
        return;
    };
    let scoped_alpha = write_alpha_graphics_state(content, fill);
    set_filtered_fill_color(content, fill, color_mode, color_transform);
    for rect in pending.rects.drain(..) {
        let rect = crate::document::paint::geometry::paint_rect_to_pdf(rect.paint_rect());
        content.rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        );
    }
    content.fill_nonzero();
    close_alpha_graphics_state(content, scoped_alpha);
    pending.ends_with_invisible_text_coverage = false;
}

fn merge_adjacent_fill_rect(
    left: &mut crate::document::paint::shapes::RenderedRect,
    right: &crate::document::paint::shapes::RenderedRect,
) -> bool {
    if !is_mergeable_fill_rect(left) || !is_mergeable_fill_rect(right) || left.fill != right.fill {
        return false;
    }
    let same_horizontal_span =
        nearly_equal(left.x(), right.x()) && nearly_equal(left.width(), right.width());
    let same_vertical_span =
        nearly_equal(left.y(), right.y()) && nearly_equal(left.height(), right.height());
    let (x, y, width, height) =
        if same_horizontal_span && nearly_equal(left.y() + left.height(), right.y()) {
            (
                left.x(),
                left.y(),
                left.width(),
                left.height() + right.height(),
            )
        } else if same_horizontal_span && nearly_equal(right.y() + right.height(), left.y()) {
            (
                left.x(),
                right.y(),
                left.width(),
                left.height() + right.height(),
            )
        } else if same_vertical_span && nearly_equal(left.x() + left.width(), right.x()) {
            (
                left.x(),
                left.y(),
                left.width() + right.width(),
                left.height(),
            )
        } else if same_vertical_span && nearly_equal(right.x() + right.width(), left.x()) {
            (
                right.x(),
                left.y(),
                left.width() + right.width(),
                left.height(),
            )
        } else {
            return false;
        };
    left.set_paint_rect(crate::document::paint::geometry::PaintRect::new(
        crate::document::paint::geometry::PaintPoint::new(x, y),
        crate::document::paint::geometry::PaintSize::new(width, height),
    ));
    true
}

fn rects_intersect(
    left: &crate::document::paint::shapes::RenderedRect,
    right: &crate::document::paint::shapes::RenderedRect,
) -> bool {
    left.x() < right.x() + right.width()
        && right.x() < left.x() + left.width()
        && left.y() < right.y() + right.height()
        && right.y() < left.y() + left.height()
}

fn rect_area_is_covered_by_rects(
    rect: &crate::document::paint::shapes::RenderedRect,
    covers: &[crate::document::paint::shapes::RenderedRect],
) -> bool {
    if covers.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let right = rect.x() + rect.width();
    let top = rect.y() + rect.height();
    let mut x_edges = vec![rect.x(), right];
    let mut y_edges = vec![rect.y(), top];
    x_edges.extend(covers.iter().flat_map(|cover| {
        [
            cover.x().clamp(rect.x(), right),
            (cover.x() + cover.width()).clamp(rect.x(), right),
        ]
    }));
    y_edges.extend(covers.iter().flat_map(|cover| {
        [
            cover.y().clamp(rect.y(), top),
            (cover.y() + cover.height()).clamp(rect.y(), top),
        ]
    }));
    sort_unique_edges(&mut x_edges);
    sort_unique_edges(&mut y_edges);

    x_edges.windows(2).all(|x_pair| {
        y_edges.windows(2).all(|y_pair| {
            let cell_left = x_pair[0];
            let cell_right = x_pair[1];
            let cell_bottom = y_pair[0];
            let cell_top = y_pair[1];
            cell_right <= cell_left
                || cell_top <= cell_bottom
                || covers.iter().any(|cover| {
                    cover.x() <= cell_left + 0.001
                        && cover.x() + cover.width() >= cell_right - 0.001
                        && cover.y() <= cell_bottom + 0.001
                        && cover.y() + cover.height() >= cell_top - 0.001
                })
        })
    })
}

fn sort_unique_edges(edges: &mut Vec<f32>) {
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|left, right| nearly_equal(*left, *right));
}

fn nearly_equal(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.001
}

pub(super) fn write_rect(
    content: &mut Content,
    rect: &crate::document::paint::shapes::RenderedRect,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    let pdf_rect = crate::document::paint::geometry::paint_rect_to_pdf(rect.paint_rect());
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        set_filtered_fill_color(content, fill, color_mode, color_transform);
        content
            .rect(
                pdf_rect.origin.x,
                pdf_rect.origin.y,
                pdf_rect.size.width,
                pdf_rect.size.height,
            )
            .fill_nonzero();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content.set_line_width(rect.stroke_width.points());
        set_filtered_stroke_color(content, stroke, color_mode, color_transform);
        content
            .rect(
                pdf_rect.origin.x,
                pdf_rect.origin.y,
                pdf_rect.size.width,
                pdf_rect.size.height,
            )
            .stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect(
    content: &mut Content,
    rect: &RenderedRoundedRect,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        set_filtered_fill_color(content, fill, color_mode, color_transform);
        write_rounded_rect_path(content, rect);
        content.fill_nonzero();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content.set_line_width(rect.stroke_width.points());
        set_filtered_stroke_color(content, stroke, color_mode, color_transform);
        write_rounded_rect_path(content, rect);
        content.stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect_path(content: &mut Content, rect: &RenderedRoundedRect) {
    if !rect.corner_shapes.all_round() {
        let commands = crate::layout::shaped_rect_path_commands(
            rect.paint_rect(),
            rect.radii,
            rect.corner_shapes,
        );
        write_path_commands(content, &commands);
        return;
    }

    // PDF paths use cubic Beziers for arcs. The kappa constant approximates a
    // quarter ellipse, matching the CSS border-radius curve shape closely
    // enough for filled/stroked page graphics.
    const KAPPA: f32 = 0.552_284_8;

    let pdf_rect = crate::document::paint::geometry::paint_rect_to_pdf(rect.paint_rect());
    let x0 = pdf_rect.origin.x;
    let y0 = pdf_rect.origin.y;
    let x1 = pdf_rect.origin.x + pdf_rect.size.width;
    let y1 = pdf_rect.origin.y + pdf_rect.size.height;
    let tl = rect.radii.top_left;
    let tr = rect.radii.top_right;
    let br = rect.radii.bottom_right;
    let bl = rect.radii.bottom_left;

    content.move_to(x0 + bl.x(), y0);
    content.line_to(x1 - br.x(), y0);
    if br.x() > 0.0 || br.y() > 0.0 {
        content.cubic_to(
            x1 - br.x() + br.x() * KAPPA,
            y0,
            x1,
            y0 + br.y() - br.y() * KAPPA,
            x1,
            y0 + br.y(),
        );
    }
    content.line_to(x1, y1 - tr.y());
    if tr.x() > 0.0 || tr.y() > 0.0 {
        content.cubic_to(
            x1,
            y1 - tr.y() + tr.y() * KAPPA,
            x1 - tr.x() + tr.x() * KAPPA,
            y1,
            x1 - tr.x(),
            y1,
        );
    }
    content.line_to(x0 + tl.x(), y1);
    if tl.x() > 0.0 || tl.y() > 0.0 {
        content.cubic_to(
            x0 + tl.x() - tl.x() * KAPPA,
            y1,
            x0,
            y1 - tl.y() + tl.y() * KAPPA,
            x0,
            y1 - tl.y(),
        );
    }
    content.line_to(x0, y0 + bl.y());
    if bl.x() > 0.0 || bl.y() > 0.0 {
        content.cubic_to(
            x0,
            y0 + bl.y() - bl.y() * KAPPA,
            x0 + bl.x() - bl.x() * KAPPA,
            y0,
            x0 + bl.x(),
            y0,
        );
    }
    content.close_path();
}

/// Serialize a generic vector path into a PDF content stream.
///
/// PDF path construction and painting operators are defined in ISO
/// 32000-1:2008, 8.5.2 and 8.5.3. CSS border rings use `f*` when their inner
/// padding-edge subpath must cut out the content area using even-odd filling.
fn write_path(
    content: &mut Content,
    path: &RenderedPath,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    if path.commands.is_empty() {
        return;
    }
    let clipped = path
        .clip
        .as_ref()
        .is_some_and(|clip| !clip.commands.is_empty());
    if clipped {
        content.save_state();
        let clip = path.clip.as_ref().unwrap();
        write_rendered_path_clip(content, clip);
    }
    let transformed =
        path.transform != crate::document::paint::geometry::PaintTransform::identity();
    if transformed {
        content
            .save_state()
            .transform(path.transform.pdf_components());
    }
    match path.paint_order {
        RenderedPathPaintOrder::FillThenStroke => {
            write_path_fill(
                content,
                path,
                vector_paints,
                resources,
                page_size,
                color_mode,
                color_transform,
            );
            write_path_stroke(
                content,
                path,
                vector_paints,
                resources,
                page_size,
                color_mode,
                color_transform,
            );
        }
        RenderedPathPaintOrder::StrokeThenFill => {
            write_path_stroke(
                content,
                path,
                vector_paints,
                resources,
                page_size,
                color_mode,
                color_transform,
            );
            write_path_fill(
                content,
                path,
                vector_paints,
                resources,
                page_size,
                color_mode,
                color_transform,
            );
        }
    }
    if transformed {
        content.restore_state();
    }
    if clipped {
        content.restore_state();
    }
}

fn write_path_fill(
    content: &mut Content,
    path: &RenderedPath,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    let Some(fill) = path.fill_paint.as_ref() else {
        return;
    };
    let Some(scoped_alpha) = write_path_fill_paint(
        content,
        fill,
        vector_paints,
        resources,
        page_size,
        color_mode,
        color_transform,
    ) else {
        return;
    };
    write_path_commands(content, &path.commands);
    match path.fill_rule {
        RenderedPathFillRule::NonZero => content.fill_nonzero(),
        RenderedPathFillRule::EvenOdd => content.fill_even_odd(),
    };
    close_alpha_graphics_state(content, scoped_alpha);
}

fn write_path_stroke(
    content: &mut Content,
    path: &RenderedPath,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    let Some(stroke) = path.stroke_paint.as_ref() else {
        return;
    };
    let Some(scoped_alpha) = write_path_stroke_paint(
        content,
        stroke,
        vector_paints,
        resources,
        page_size,
        color_mode,
        color_transform,
    ) else {
        return;
    };
    let style = &path.stroke_style;
    content
        .set_line_width(path.stroke_width.points())
        .set_line_cap(match style.line_cap {
            RenderedPathLineCap::Butt => LineCapStyle::ButtCap,
            RenderedPathLineCap::Round => LineCapStyle::RoundCap,
            RenderedPathLineCap::Square => LineCapStyle::ProjectingSquareCap,
        })
        .set_line_join(match style.line_join {
            RenderedPathLineJoin::Miter => LineJoinStyle::MiterJoin,
            RenderedPathLineJoin::Round => LineJoinStyle::RoundJoin,
            RenderedPathLineJoin::Bevel => LineJoinStyle::BevelJoin,
        })
        .set_miter_limit(style.miter_limit)
        .set_dash_pattern(style.dash_array.iter().cloned(), style.dash_offset);
    write_path_commands(content, &path.commands);
    content.stroke();
    close_alpha_graphics_state(content, scoped_alpha);
}

fn write_path_fill_paint(
    content: &mut Content,
    paint: &RenderedPathPaint,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) -> Option<bool> {
    match paint {
        RenderedPathPaint::Solid(color) if color.is_visible() => {
            let alpha = write_alpha_graphics_state(content, *color);
            set_filtered_fill_color(content, *color, color_mode, color_transform);
            Some(alpha)
        }
        RenderedPathPaint::Solid(_) => None,
        RenderedPathPaint::Gradient(gradient) => {
            let gradient = transformed_gradient(gradient, color_transform);
            let resource = vector_paints.gradient_resource(&gradient, resources, page_size);
            if let Some(name) = &resource.alpha_gstate_name {
                content.save_state().set_parameters(pdf_name(name));
            }
            content
                .set_fill_color_space(ColorSpaceOperand::Pattern)
                .set_fill_pattern([], pdf_name(&resource.pattern_name));
            Some(resource.alpha_gstate_name.is_some())
        }
        RenderedPathPaint::SvgPattern(pattern) => {
            let alpha = write_alpha_graphics_state(
                content,
                crate::CssColor::rgba(0, 0, 0, pattern.opacity),
            );
            let name = vector_paints.svg_path_tiling_resource(pattern, resources, color_mode);
            content
                .set_fill_color_space(ColorSpaceOperand::Pattern)
                .set_fill_pattern([], pdf_name(&name));
            Some(alpha)
        }
    }
}

fn write_path_stroke_paint(
    content: &mut Content,
    paint: &RenderedPathPaint,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) -> Option<bool> {
    match paint {
        RenderedPathPaint::Solid(color) if color.is_visible() => {
            let alpha = write_alpha_graphics_state(content, *color);
            set_filtered_stroke_color(content, *color, color_mode, color_transform);
            Some(alpha)
        }
        RenderedPathPaint::Solid(_) => None,
        RenderedPathPaint::Gradient(gradient) => {
            let gradient = transformed_gradient(gradient, color_transform);
            let resource = vector_paints.gradient_resource(&gradient, resources, page_size);
            if let Some(name) = &resource.alpha_gstate_name {
                content.save_state().set_parameters(pdf_name(name));
            }
            content
                .set_stroke_color_space(ColorSpaceOperand::Pattern)
                .set_stroke_pattern([], pdf_name(&resource.pattern_name));
            Some(resource.alpha_gstate_name.is_some())
        }
        RenderedPathPaint::SvgPattern(pattern) => {
            let alpha = write_alpha_graphics_state(
                content,
                crate::CssColor::rgba(0, 0, 0, pattern.opacity),
            );
            let name = vector_paints.svg_path_tiling_resource(pattern, resources, color_mode);
            content
                .set_stroke_color_space(ColorSpaceOperand::Pattern)
                .set_stroke_pattern([], pdf_name(&name));
            Some(alpha)
        }
    }
}

fn write_rendered_path_clip(
    content: &mut Content,
    clip: &crate::document::paint::paths::RenderedPathClip,
) {
    write_clip_path(content, &clip.commands, clip.fill_rule);
    for additional_clip in &clip.additional_clips {
        write_clip_path(
            content,
            &additional_clip.commands,
            additional_clip.fill_rule,
        );
    }
}

fn write_clip_path(
    content: &mut Content,
    commands: &[RenderedPathCommand],
    fill_rule: RenderedPathFillRule,
) {
    write_path_commands(content, commands);
    match fill_rule {
        RenderedPathFillRule::NonZero => {
            content.clip_nonzero();
        }
        RenderedPathFillRule::EvenOdd => {
            content.clip_even_odd();
        }
    }
    content.end_path();
}

fn write_path_commands(content: &mut Content, commands: &[RenderedPathCommand]) {
    for command in commands {
        match command.typed_points() {
            RenderedPathCommandPoints::MoveTo(point) => {
                let point = crate::document::paint::geometry::paint_point_to_pdf(point);
                content.move_to(point.x, point.y);
            }
            RenderedPathCommandPoints::LineTo(point) => {
                let point = crate::document::paint::geometry::paint_point_to_pdf(point);
                content.line_to(point.x, point.y);
            }
            RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                let control_1 = crate::document::paint::geometry::paint_point_to_pdf(control_1);
                let control_2 = crate::document::paint::geometry::paint_point_to_pdf(control_2);
                let end = crate::document::paint::geometry::paint_point_to_pdf(end);
                content.cubic_to(
                    control_1.x,
                    control_1.y,
                    control_2.x,
                    control_2.y,
                    end.x,
                    end.y,
                );
            }
            RenderedPathCommandPoints::Close => {
                content.close_path();
            }
        }
    }
}

pub(super) fn write_stroke(
    content: &mut Content,
    stroke: &crate::document::paint::shapes::RenderedStroke,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    if !stroke.color.is_visible() {
        return;
    }
    content.save_state();
    if let Some(resource_name) = paint_alpha_resource_name(stroke.color) {
        content.set_parameters(pdf_name(&resource_name));
    }
    if let Some((dash, gap)) = stroke.dash {
        content.set_dash_pattern([dash, gap], 0.0);
    } else {
        content.set_dash_pattern([], 0.0);
    }
    let (start, end) = stroke.paint_points();
    let start = crate::document::paint::geometry::paint_point_to_pdf(start);
    let end = crate::document::paint::geometry::paint_point_to_pdf(end);
    content.set_line_width(stroke.stroke_width.points());
    set_filtered_stroke_color(content, stroke.color, color_mode, color_transform);
    content
        .move_to(start.x, start.y)
        .line_to(end.x, end.y)
        .stroke()
        .restore_state();
}

pub(super) fn write_image(
    content: &mut Content,
    image: &crate::document::paint::images::RenderedImage,
    index: usize,
    resource: &PreparedImageResource,
    color_policy: &PdfLoweringColorPolicy,
) {
    if let Some(actual_text) = &image.actual_text {
        let mut marked_content = content.begin_marked_content_with_properties(Name(b"Span"));
        marked_content
            .properties()
            .actual_text(TextStr(actual_text.as_ref()));
    }
    match resource {
        PreparedImageResource::Transparent => {}
        PreparedImageResource::SolidFill(fill) => {
            let rect = crate::document::paint::geometry::paint_rect_to_pdf(image.paint_rect());
            content.save_state();
            let omit_destination_clip = image_clip_is_redundant_destination_rect(image);
            if let Some(clip) = image.clip().filter(|_| !omit_destination_clip) {
                write_rendered_path_clip(content, clip);
            }
            debug_assert!(
                image.transform.is_none(),
                "solid image fills require page-space image geometry"
            );
            color_policy.set_raster_fill_color(content, &fill.color_space, fill.components);
            content
                .rect(
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                )
                .fill_nonzero();
            content.restore_state();
        }
        PreparedImageResource::Raster(_) => {
            let rect = crate::document::paint::geometry::paint_rect_to_pdf(image.paint_rect());
            content.save_state();
            if let Some(clip) = image.clip() {
                write_rendered_path_clip(content, clip);
            }
            if let Some(transform) = image.transform {
                content.transform(transform.pdf_components());
            }
            content
                .transform([
                    rect.size.width,
                    0.0,
                    0.0,
                    rect.size.height,
                    rect.origin.x,
                    rect.origin.y,
                ])
                .x_object(pdf_name(&format!("Im{}", index + 1)));
            content.restore_state();
        }
    }
    if image.actual_text.is_some() {
        content.end_marked_content();
    }
}

/// Whether a retained clip is exactly the image destination rectangle.
///
/// A solid-fill replacement already paints only inside this rectangle. Keeping
/// an identical PDF clipping path would make an otherwise equivalent vector
/// fill acquire a separate raster-edge coverage rule, so omit only this
/// provably redundant non-zero clip. All rounded and intersected clips remain
/// active. ISO 32000-2:2020, 8.5.4.
fn clip_is_exact_paint_rect(
    clip: &crate::document::paint::paths::RenderedPathClip,
    rect: crate::document::paint::geometry::PaintRect,
) -> bool {
    if clip.fill_rule != crate::document::paint::paths::RenderedPathFillRule::NonZero
        || !clip.additional_clips.is_empty()
    {
        return false;
    }
    let origin = rect.origin;
    let right = crate::document::paint::geometry::PaintPoint::new(rect.max_x(), origin.y);
    let top_right = crate::document::paint::geometry::PaintPoint::new(rect.max_x(), rect.max_y());
    let top_left = crate::document::paint::geometry::PaintPoint::new(origin.x, rect.max_y());
    matches!(
        clip.commands.as_slice(),
        [
            crate::document::paint::paths::RenderedPathCommand::MoveTo(start),
            crate::document::paint::paths::RenderedPathCommand::LineTo(line_right),
            crate::document::paint::paths::RenderedPathCommand::LineTo(line_top_right),
            crate::document::paint::paths::RenderedPathCommand::LineTo(line_top_left),
            crate::document::paint::paths::RenderedPathCommand::Close,
        ] if *start == origin
            && *line_right == right
            && *line_top_right == top_right
            && *line_top_left == top_left
    )
}

/// Return whether an image has no effective clip beyond its own destination.
///
/// Object fitting records this directly whenever it constructs an uncropped
/// concrete object. The structural path check remains for clips built by
/// older producers, but is intentionally only a fallback: CSS geometry must
/// not depend on exact equality after independent floating-point operations.
fn image_clip_is_redundant_destination_rect(
    image: &crate::document::paint::images::RenderedImage,
) -> bool {
    image.has_destination_rect_clip()
        || image
            .clip()
            .is_none_or(|clip| clip_is_exact_paint_rect(clip, image.paint_rect()))
}

pub(super) fn write_image_pattern(
    content: &mut Content,
    pattern: &crate::document::paint::patterns::RenderedImagePattern,
    index: usize,
) {
    write_transformed_tiling_pattern_fill(
        content,
        pattern.paint_rect(),
        &format!("P{}", index + 1),
        pattern.clip(),
        pattern.transform(),
    );
}

/// Paint a tiling pattern through its destination path.
///
/// The fill path already bounds a PDF pattern paint. Adding a second clip with
/// the same rectangle changes fractional-edge raster coverage without changing
/// CSS background geometry, so only an additional CSS `background-clip` path
/// is installed here. This applies CSS Backgrounds' painting-area rule while
/// preserving authored rounded clips:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-clip>.
/// PDF pattern fills and clipping paths are defined by ISO 32000-2:2020,
/// sections 8.7.4 and 8.5.4.
fn write_tiling_pattern_fill(
    content: &mut Content,
    paint_rect: crate::document::paint::geometry::PaintRect,
    name: &str,
    clip: Option<&crate::document::paint::paths::RenderedPathClip>,
) {
    let rect = crate::document::paint::geometry::paint_rect_to_pdf(paint_rect);
    content.save_state();
    if let Some(clip) = clip {
        write_rendered_path_clip(content, clip);
    }
    content
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(name))
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .fill_nonzero();
    content.restore_state();
}

fn write_gradient_tiling_pattern(
    content: &mut Content,
    pattern: &crate::document::paint::patterns::RenderedGradientPattern,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    page_size: PdfSize,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    let name =
        vector_paints.tiling_gradient_resource(pattern, resources, page_size, color_transform);
    write_transformed_tiling_pattern_fill(
        content,
        pattern.paint_rect(),
        &name,
        pattern.clip(),
        pattern.transform(),
    );
}

/// Paint a source-local tiling pattern through its destination fragmentainer.
fn write_transformed_tiling_pattern_fill(
    content: &mut Content,
    paint_rect: crate::document::paint::geometry::PaintRect,
    name: &str,
    clip: Option<&crate::document::paint::paths::RenderedPathClip>,
    transform: crate::document::paint::geometry::PaintTransform,
) {
    if transform == crate::document::paint::geometry::PaintTransform::identity() {
        write_tiling_pattern_fill(content, paint_rect, name, clip);
        return;
    }
    let rect = crate::document::paint::geometry::paint_rect_to_pdf(paint_rect);
    content.save_state().transform(transform.pdf_components());
    if let Some(clip) = clip {
        write_rendered_path_clip(content, clip);
    }
    content
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(name))
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .fill_nonzero()
        .restore_state();
}

fn write_svg_tiling_pattern(
    content: &mut Content,
    pattern: &crate::document::paint::patterns::RenderedSvgPattern,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let form_id = state.resources.form();
    let form_name = format!("SvgTile{}", state.forms.len() + 1);
    let mut form_content = Content::new();
    for path in &pattern.paths {
        write_path(
            &mut form_content,
            path,
            state.vector_paints,
            state.resources,
            state.page_size,
            state.color_mode,
            state.active_filter_color_transform,
        );
    }
    state.forms.push(FormXObjectRender {
        id: form_id,
        name: form_name.clone(),
        bbox: crate::document::paint::geometry::PaintClip::new(
            0.0,
            0.0,
            pattern.tiling.tile_size.width,
            pattern.tiling.tile_size.height,
        ),
        stream: PdfStreamProgram {
            bytes: form_content.finish().into_vec(),
            resource_uses: PdfStreamResourceUses::default(),
            resolved_resources: None,
        },
        kind: PdfFormKind::Ordinary,
    });
    let id = state.resources.pattern();
    let name = format!("SP{}", state.vector_paints.svg_tilings.len() + 1);
    let mut pattern_content = Content::new();
    pattern_content.x_object(pdf_name(&form_name));
    state.vector_paints.svg_tilings.push(SvgTilingPatternPlan {
        id,
        name: name.clone(),
        form_id,
        form_name: form_name.clone(),
        pattern: pattern.clone(),
        transform: state.active_paint_transform.multiply(
            crate::document::paint::geometry::PaintTransform::translate(
                crate::document::paint::geometry::PaintTranslation::new(
                    pattern.tiling.origin.x,
                    pattern.tiling.origin.y,
                ),
            ),
        ),
        stream: PdfStreamProgram {
            bytes: pattern_content.finish().into_vec(),
            resource_uses: PdfStreamResourceUses {
                xobjects: [(form_name.clone(), PdfXObjectHandle::Form(form_id))].into(),
                ..PdfStreamResourceUses::default()
            },
            resolved_resources: None,
        },
    });
    write_transformed_tiling_pattern_fill(
        content,
        pattern.paint_rect(),
        &name,
        pattern.clip(),
        pattern.transform(),
    );
}

/// Serialize the supported solid-vector subset of an SVG paint-server cell.
/// The enclosing PDF tiling pattern supplies the cell's bounding box and
/// matrix; these paths deliberately remain in SVG user coordinates.
pub(super) fn svg_path_pattern_tile_content(
    pattern: &crate::document::paint::paths::RenderedSvgPathPattern,
    color_mode: PdfColorMode,
    image_indexes: &HashMap<ImageResourceSource, PlannedImageIndex>,
    raster_resolution_dppx: f32,
) -> PdfStreamProgram {
    let mut content = Content::new();
    let mut vector_paints = VectorPaintResources {
        plans: Vec::new(),
        gradients: BTreeMap::new(),
        tilings: Vec::new(),
        svg_tilings: Vec::new(),
        svg_path_tilings: Vec::new(),
        image_indexes: image_indexes.clone(),
        raster_resolution_dppx,
    };
    let mut resources = PdfResourceRegistry::default();
    let mut image_uses = BTreeMap::new();
    write_svg_pattern_scene(
        &mut content,
        &pattern.scene,
        &mut vector_paints,
        &mut resources,
        PdfSize::new(pattern.tile_size.width, pattern.tile_size.height),
        color_mode,
        &mut image_uses,
    );
    debug_assert!(vector_paints.plans.is_empty());
    debug_assert!(vector_paints.tilings.is_empty());
    debug_assert!(vector_paints.svg_tilings.is_empty());
    debug_assert!(vector_paints.svg_path_tilings.is_empty());
    PdfStreamProgram {
        bytes: content.finish().into_vec(),
        resource_uses: PdfStreamResourceUses {
            xobjects: image_uses,
            ..PdfStreamResourceUses::default()
        },
        resolved_resources: None,
    }
}

fn write_svg_pattern_scene(
    content: &mut Content,
    scene: &crate::svg::SvgPaintGroup,
    vector_paints: &mut VectorPaintResources,
    resources: &mut PdfResourceRegistry,
    tile_size: PdfSize,
    color_mode: PdfColorMode,
    image_uses: &mut BTreeMap<String, PdfXObjectHandle>,
) {
    for item in &scene.items {
        match item {
            crate::svg::SvgPaintItem::Path(path) => write_path(
                content,
                path,
                vector_paints,
                resources,
                tile_size,
                color_mode,
                crate::css::BoundedSrgbColorTransform::IDENTITY,
            ),
            crate::svg::SvgPaintItem::RasterImage(image) => {
                let source = crate::pdf::resources::image_source(
                    image,
                    vector_paints.raster_resolution_dppx,
                );
                let Some(index) = vector_paints.image_indexes.get(&source).copied() else {
                    continue;
                };
                let name = format!("Im{}", index.0 + 1);
                let rect = crate::document::paint::geometry::paint_rect_to_pdf(image.paint_rect());
                content.save_state();
                if let Some(clip) = image.clip() {
                    write_rendered_path_clip(content, clip);
                }
                if let Some(transform) = image.transform {
                    content.transform(transform.pdf_components());
                }
                content
                    .transform([
                        rect.size.width,
                        0.0,
                        0.0,
                        rect.size.height,
                        rect.origin.x,
                        rect.origin.y,
                    ])
                    .x_object(pdf_name(&name));
                content.restore_state();
                image_uses.insert(name, PdfXObjectHandle::Image(PdfImageHandle(index.0)));
            }
            crate::svg::SvgPaintItem::Group(group) | crate::svg::SvgPaintItem::NestedSvg(group) => {
                write_svg_pattern_scene(
                    content,
                    group,
                    vector_paints,
                    resources,
                    tile_size,
                    color_mode,
                    image_uses,
                )
            }
            crate::svg::SvgPaintItem::OutlinedText(outlined) => write_svg_pattern_scene(
                content,
                &outlined.content,
                vector_paints,
                resources,
                tile_size,
                color_mode,
                image_uses,
            ),
        }
    }
}

pub(super) fn write_line(
    content: &mut Content,
    line: &crate::document::paint::text::RenderedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
) {
    write_line_with_visibility(
        content,
        line,
        embedded_fonts,
        color_mode,
        color_transform,
        false,
    );
}

/// Serialize a retained CSS text-paint record, optionally without visible ink.
///
/// CSS 2.2 Appendix E requires the record to retain its source paint order;
/// invisible PDF text is used only after later, same-scope opaque coverage has
/// proven that its glyph ink cannot contribute to the composited result. PDF
/// rendering mode 3 preserves glyph codes and `/ToUnicode` extraction while
/// suppressing painting (ISO 32000-2:2020, 9.3.6).
fn write_line_with_visibility(
    content: &mut Content,
    line: &crate::document::paint::text::RenderedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
    invisible: bool,
) {
    let wrote_line = write_rendered_line(
        content,
        line,
        embedded_fonts,
        color_mode,
        color_transform,
        invisible,
    );
    if !wrote_line && !line.text.is_empty() && line.runs.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
    }
}

pub(super) fn write_rendered_line(
    content: &mut Content,
    line: &crate::document::paint::text::RenderedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    color_mode: PdfColorMode,
    color_transform: crate::css::BoundedSrgbColorTransform,
    invisible: bool,
) -> bool {
    if !line.color.is_visible() {
        return true;
    }

    // PDF 2.0 9.4.4 text matrices position each glyph stream in user space.
    // CSS inline layout stores shaped runs at visual offsets inside one line
    // box.  Identity-matrix runs can retain one text line matrix and move to
    // each absolute run origin with `Td`; transformed runs retain the
    // conservative absolute `Tm` emission required by writing-mode transforms.
    let line_origin = crate::document::paint::geometry::paint_point_to_pdf(line.origin());
    let text_runs = pdf_text_runs(line, embedded_fonts.document_font_to_embedded_font.len())
        .collect::<Vec<_>>();
    let uses_synthetic_bold = !invisible
        && text_runs.iter().any(|run| {
            embedded_fonts
                .document_font_synthesis
                .get(run.document_font_id)
                .is_some_and(|synthesis| synthesis.embolden)
        });
    if uses_synthetic_bold {
        // `w` is a graphics-state parameter. Scope it outside BT/ET so the
        // synthetic text does not affect later CSS borders or paths.
        content.save_state();
    }
    let mut text_started = false;
    let mut scoped_alpha = false;
    let mut saw_text_run = false;
    let mut identity_text_line_origin = None::<(f32, f32)>;
    let mut active_font = None::<(usize, f32)>;
    let mut active_text_rendering_mode = if invisible {
        TextRenderingMode::Invisible
    } else {
        TextRenderingMode::Fill
    };
    let mut reuse_text_position = false;
    for (run_index, run) in text_runs.iter().enumerate() {
        saw_text_run = true;
        if run.glyphs.is_empty() {
            log::debug!("empty shaped text line {:?}", line.text);
            reuse_text_position = false;
            continue;
        }
        let Some(embedded_font_index) = embedded_fonts
            .document_font_to_embedded_font
            .get(run.document_font_id)
            .and_then(|index| *index)
        else {
            log::warn!(
                "skipping shaped text run with unmapped document font id {}",
                run.document_font_id
            );
            reuse_text_position = false;
            continue;
        };
        let Some(font) = embedded_fonts.fonts.get(embedded_font_index) else {
            log::warn!(
                "skipping shaped text run with missing embedded font resource {}",
                embedded_font_index
            );
            reuse_text_position = false;
            continue;
        };
        if run
            .glyphs
            .iter()
            .filter_map(RenderedGlyph::painted_id)
            .any(|glyph_id| !font.source_gid_to_cid.contains_key(&glyph_id))
        {
            log::warn!(
                "skipping shaped text run whose glyphs are missing from PDF font CID mapping"
            );
            reuse_text_position = false;
            continue;
        }
        if !text_started {
            scoped_alpha = write_alpha_graphics_state(content, line.color);
            set_filtered_fill_color(content, line.color, color_mode, color_transform);
            content.begin_text();
            if invisible {
                content.set_text_rendering_mode(TextRenderingMode::Invisible);
            }
            text_started = true;
        }
        let synthesis = embedded_fonts
            .document_font_synthesis
            .get(run.document_font_id)
            .copied()
            .unwrap_or_default();
        let synthetic_bold = !invisible && synthesis.embolden;
        let visible_mode = if synthetic_bold {
            TextRenderingMode::FillStroke
        } else if invisible {
            TextRenderingMode::Invisible
        } else {
            TextRenderingMode::Fill
        };
        if visible_mode != active_text_rendering_mode {
            content.set_text_rendering_mode(visible_mode);
            active_text_rendering_mode = visible_mode;
        }
        if synthetic_bold {
            set_filtered_stroke_color(content, line.color, color_mode, color_transform);
            content.set_line_width(run.font_size * SYNTHETIC_BOLD_STROKE_EM);
        }
        let pdf_font_size = quantized_pdf_font_size(run.font_size);
        let run_origin = (line_origin.x + run.x_offset, line_origin.y + run.y_offset);
        let run_text_matrix = pdf_text_matrix(run.text_matrix, synthesis.oblique, run_origin);
        if reuse_text_position {
            debug_assert!(run.text_matrix.is_identity());
            debug_assert!(synthesis.oblique.is_none());
        } else if run.text_matrix.is_identity() && synthesis.oblique.is_none() {
            if let Some((previous_x, previous_y)) = identity_text_line_origin {
                content.next_line(run_origin.0 - previous_x, run_origin.1 - previous_y);
            } else {
                content.set_text_matrix(run_text_matrix);
            }
            identity_text_line_origin = Some(run_origin);
        } else {
            content.set_text_matrix(run_text_matrix);
            identity_text_line_origin = None;
        }
        if active_font != Some((embedded_font_index, pdf_font_size)) {
            content.set_font(pdf_name(&font.resource_name), pdf_font_size);
            active_font = Some((embedded_font_index, pdf_font_size));
        }
        let glyph_metrics = PdfGlyphRunMetrics::new(font, PdfTextFontSize::new(pdf_font_size));
        if let Some(actual_text) = run.actual_text {
            let mut marked_content = content.begin_marked_content_with_properties(Name(b"Span"));
            marked_content
                .properties()
                .actual_text(TextStr(actual_text));
        }
        if glyphs_have_origin_offsets(run.glyphs) {
            write_glyphs_at_origins(
                content,
                pdf_text_matrix_components(run.text_matrix, synthesis.oblique),
                run_origin,
                run.glyphs,
                glyph_metrics,
                invisible,
                visible_mode,
            );
            // Keep the run's logical text origin installed for a following
            // identity run, whose `Td` displacement is relative to it.
            content.set_text_matrix(run_text_matrix);
        } else {
            let continues_to_next = text_runs.get(run_index + 1).is_some_and(|next| {
                let next_synthesis = embedded_fonts
                    .document_font_synthesis
                    .get(next.document_font_id)
                    .copied()
                    .unwrap_or_default();
                pdf_text_runs_can_share_cursor(
                    run,
                    next,
                    synthesis.oblique.is_none(),
                    next_synthesis.oblique.is_none(),
                )
            });
            write_glyphs(
                content,
                run.glyphs,
                glyph_metrics,
                invisible,
                visible_mode,
                continues_to_next,
            );
            reuse_text_position = continues_to_next;
        }
        if glyphs_have_origin_offsets(run.glyphs) {
            reuse_text_position = false;
        }
        if run.actual_text.is_some() {
            content.end_marked_content();
        }
    }
    if text_started {
        if active_text_rendering_mode != TextRenderingMode::Fill {
            content.set_text_rendering_mode(TextRenderingMode::Fill);
        }
        content.end_text();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if uses_synthetic_bold {
        content.restore_state();
    }
    saw_text_run
}

/// Activate a PDF ExtGState for semi-transparent paint.
///
/// PDF 1.4 uses the `gs` operator to load graphics-state parameters,
/// including stroking and nonstroking alpha constants:
/// ISO 32000-1:2008, 8.4.4 "Graphics State Operators" and 11.7.4.3
/// "Constant Shape and Opacity".
fn write_alpha_graphics_state(content: &mut Content, color: CssColor) -> bool {
    if let Some(resource_name) = paint_alpha_resource_name(color) {
        content
            .save_state()
            .set_parameters(pdf_name(&resource_name));
        true
    } else {
        false
    }
}

fn close_alpha_graphics_state(content: &mut Content, scoped_alpha: bool) {
    if scoped_alpha {
        content.restore_state();
    }
}

pub(super) fn quantized_pdf_font_size(font_size: f32) -> f32 {
    // WeasyPrint shapes through Pango, whose public units are fixed at
    // 1024 units per CSS pixel. CSS Values defines 1px as 0.75pt, and PDF
    // text space uses points here, so mirror that quantization at emission to
    // keep glyph rasterization aligned with WeasyPrint.
    let css_px = font_size / crate::css::CSS_PX_TO_PT;
    (css_px * 1024.0).floor() / 1024.0 * crate::css::CSS_PX_TO_PT
}

/// The font size installed by PDF `Tf`, in PDF user-space points.
///
/// This differs from the CSS font size when PDF emission applies its
/// compatibility quantization, so text positioning must use this value rather
/// than the layout-time size.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PdfTextFontSize(f32);

impl PdfTextFontSize {
    fn new(points: f32) -> Self {
        debug_assert!(points.is_finite() && points > 0.0);
        Self(points.max(0.001))
    }

    fn points(self) -> f32 {
        self.0
    }
}

/// A `TJ` displacement in PDF text space.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PdfTextSpaceAdjustment(f32);

impl PdfTextSpaceAdjustment {
    fn for_used_advance(
        pdf_width: Option<PdfTextSpaceWidth>,
        font_size: PdfTextFontSize,
        used_advance: f32,
    ) -> Self {
        let value = match pdf_width {
            Some(width) => {
                // `TJ` first applies the CID's `/W` advance. Convert the
                // layout target directly to PDF text space. Going through
                // points first needlessly loses precision through
                // subtraction of nearly equal advances.
                let adjustment = width.as_pdf_number() - used_advance * 1000.0 / font_size.points();
                let represented_advance = adjustment * font_size.points() / 1000.0;
                let residual =
                    width.points_at(font_size.points()) - used_advance - represented_advance;
                // These values are serialized as f32 PDF operands.  The two
                // equivalent evaluation orders above can differ by a few
                // ULPs after subtracting nearly equal advances, particularly
                // for CFF fonts with non-power-of-two units per em.
                let tolerance = f32::EPSILON
                    * (width.points_at(font_size.points()).abs()
                        + used_advance.abs()
                        + represented_advance.abs())
                    .max(1.0)
                    * 8.0;
                debug_assert!(residual.abs() <= tolerance);
                adjustment
            }
            // An advance-only glyph has no CID width for PDF to apply.
            None => -used_advance * 1000.0 / font_size.points(),
        };
        debug_assert!(value.is_finite());
        Self(value)
    }

    fn is_zero(self) -> bool {
        self.0 == 0.0
    }

    fn value(self) -> f32 {
        self.0
    }
}

/// Per-run PDF metrics shared by glyph encoding and `TJ` positioning.
#[derive(Clone, Copy)]
struct PdfGlyphRunMetrics<'a> {
    source_gid_to_cid: &'a BTreeMap<u16, u16>,
    source_gid_to_width: &'a BTreeMap<u16, PdfTextSpaceWidth>,
    default_width: PdfTextSpaceWidth,
    font_size: PdfTextFontSize,
}

impl<'a> PdfGlyphRunMetrics<'a> {
    fn new(font: &'a EmbeddedFontPlan<'_>, font_size: PdfTextFontSize) -> Self {
        Self {
            source_gid_to_cid: &font.source_gid_to_cid,
            source_gid_to_width: &font.source_gid_to_width,
            default_width: PdfTextSpaceWidth(font.default_width.round() as i32),
            font_size,
        }
    }

    fn cid(self, glyph_id: u16) -> u16 {
        self.source_gid_to_cid[&glyph_id]
    }

    fn adjustment_for(self, glyph: &RenderedGlyph) -> PdfTextSpaceAdjustment {
        let pdf_width = glyph.painted_id().map(|glyph_id| {
            self.source_gid_to_width
                .get(&glyph_id)
                .copied()
                .unwrap_or(self.default_width)
        });
        PdfTextSpaceAdjustment::for_used_advance(pdf_width, self.font_size, glyph.x_advance)
    }
}

/// Whether two already-coalesced identity text runs can retain one PDF text
/// matrix. The layout records remain separate: this only avoids replacing the
/// previous run's serialized text-space advance with a rounded absolute
/// coordinate before painting the next run.
fn pdf_text_runs_can_share_cursor(
    previous: &super::text::PdfTextRun<'_>,
    next: &super::text::PdfTextRun<'_>,
    previous_has_no_oblique_synthesis: bool,
    next_has_no_oblique_synthesis: bool,
) -> bool {
    if !previous.text_matrix.is_identity()
        || !next.text_matrix.is_identity()
        || !previous_has_no_oblique_synthesis
        || !next_has_no_oblique_synthesis
        || glyphs_have_origin_offsets(previous.glyphs)
        || glyphs_have_origin_offsets(next.glyphs)
        || previous
            .glyphs
            .iter()
            .chain(next.glyphs)
            .any(RenderedGlyph::is_painted_by_vector_path)
    {
        return false;
    }

    let previous_end = previous.x_offset
        + previous
            .glyphs
            .iter()
            .map(|glyph| glyph.x_advance)
            .sum::<f32>();
    let scale =
        (previous_end.abs() + next.x_offset.abs() + previous.y_offset.abs() + next.y_offset.abs())
            .max(1.0);
    let tolerance = f32::EPSILON * scale * 8.0;
    (previous_end - next.x_offset).abs() <= tolerance
        && (previous.y_offset - next.y_offset).abs() <= tolerance
}

fn write_glyphs(
    content: &mut Content,
    glyphs: &[RenderedGlyph],
    metrics: PdfGlyphRunMetrics<'_>,
    invisible: bool,
    visible_mode: TextRenderingMode,
    continue_text_cursor: bool,
) {
    debug_assert!(
        !continue_text_cursor || !glyphs.iter().any(RenderedGlyph::is_painted_by_vector_path)
    );
    if glyphs.iter().any(RenderedGlyph::is_painted_by_vector_path) {
        write_glyphs_with_paint_modes(content, glyphs, metrics, invisible, visible_mode);
        return;
    }
    if !needs_positioned_glyphs(glyphs, metrics, continue_text_cursor) {
        let glyph_bytes = glyph_bytes(glyphs, metrics.source_gid_to_cid);
        content.show(Str(&glyph_bytes));
        return;
    }

    let mut positioned = content.show_positioned();
    let mut items = positioned.items();
    for (index, glyph) in glyphs.iter().enumerate() {
        if let Some(glyph_id) = glyph.painted_id() {
            let glyph_bytes = glyph_id_bytes(metrics.cid(glyph_id));
            items.show(Str(&glyph_bytes));
        }
        if index + 1 < glyphs.len() || continue_text_cursor && index + 1 == glyphs.len() {
            // A paint-continuation run starts at this text cursor. Preserve
            // the final adjustment as well as the internal ones, otherwise a
            // freshly rounded absolute position subtly changes glyph coverage.
            let adjustment = metrics.adjustment_for(glyph);
            if !adjustment.is_zero() {
                items.adjust(adjustment.value());
            }
        }
    }
}

/// Emit a run that mixes normal PDF text ink with glyphs whose visible ink is
/// supplied by an equivalent vector path. ISO 32000-2:2020, 9.3.6 defines
/// rendering mode 3 as non-painting text; retaining its glyph codes preserves
/// normal font subsetting, tagging, and ToUnicode extraction.
fn write_glyphs_with_paint_modes(
    content: &mut Content,
    glyphs: &[RenderedGlyph],
    metrics: PdfGlyphRunMetrics<'_>,
    invisible: bool,
    visible_mode: TextRenderingMode,
) {
    let mut mode = if invisible {
        TextRenderingMode::Invisible
    } else {
        visible_mode
    };
    for (index, glyph) in glyphs.iter().enumerate() {
        let next_mode = if invisible || glyph.is_painted_by_vector_path() {
            TextRenderingMode::Invisible
        } else {
            visible_mode
        };
        if next_mode != mode {
            content.set_text_rendering_mode(next_mode);
            mode = next_mode;
        }

        let mut positioned = content.show_positioned();
        let mut items = positioned.items();
        if let Some(glyph_id) = glyph.painted_id() {
            let glyph_bytes = glyph_id_bytes(metrics.cid(glyph_id));
            items.show(Str(&glyph_bytes));
        }
        if index + 1 < glyphs.len() {
            let adjustment = metrics.adjustment_for(glyph);
            if !adjustment.is_zero() {
                items.adjust(adjustment.value());
            }
        }
    }
    if !invisible && mode != visible_mode {
        content.set_text_rendering_mode(visible_mode);
    }
}

/// Paint a shaped run with one text matrix per glyph origin.
///
/// OpenType GPOS can assign an individual glyph a local x/y offset while its
/// advance remains part of the unmodified CSS inline progression. PDF `TJ`
/// adjusts advances but cannot express a per-glyph origin on both axes, so an
/// offset-bearing run must install the selected writing-mode matrix at each
/// glyph's shaped origin. Keeping this at the PDF serialization boundary
/// preserves CSS layout geometry and lets ordinary runs retain compact text
/// operators. See ISO 32000-2:2020, 9.4.4 "Text Space Details".
fn write_glyphs_at_origins(
    content: &mut Content,
    [a, b, c, d]: [f32; 4],
    run_origin: (f32, f32),
    glyphs: &[RenderedGlyph],
    metrics: PdfGlyphRunMetrics<'_>,
    invisible: bool,
    visible_mode: TextRenderingMode,
) {
    let mut pen_x = 0.0;
    let mut mode = if invisible {
        TextRenderingMode::Invisible
    } else {
        visible_mode
    };
    for glyph in glyphs {
        let next_mode = if invisible || glyph.is_painted_by_vector_path() {
            TextRenderingMode::Invisible
        } else {
            visible_mode
        };
        if next_mode != mode {
            content.set_text_rendering_mode(next_mode);
            mode = next_mode;
        }
        if let Some(glyph_id) = glyph.painted_id() {
            let local_origin = crate::document::paint::text::TextRunPoint::new(
                pen_x + glyph.x_offset,
                glyph.y_offset,
            );
            let glyph_origin = pdf_text_matrix_transform_local_point([a, b, c, d], local_origin);
            content.set_text_matrix([
                a,
                b,
                c,
                d,
                run_origin.0 + glyph_origin.x,
                run_origin.1 + glyph_origin.y,
            ]);
            let glyph_bytes = glyph_id_bytes(metrics.cid(glyph_id));
            content.show(Str(&glyph_bytes));
        }
        pen_x += glyph.x_advance;
    }
    if !invisible && mode != visible_mode {
        content.set_text_rendering_mode(visible_mode);
    }
    debug_assert!(pen_x.is_finite());
}

fn glyphs_have_origin_offsets(glyphs: &[RenderedGlyph]) -> bool {
    glyphs
        .iter()
        .any(|glyph| glyph.x_offset.abs() > 0.01 || glyph.y_offset.abs() > 0.01)
}

fn pdf_text_matrix(
    text_matrix: crate::document::paint::text::RenderedTextMatrix,
    synthetic_oblique: Option<crate::document::SyntheticObliqueAngle>,
    origin: (f32, f32),
) -> [f32; 6] {
    let [a, b, c, d] = pdf_text_matrix_components(text_matrix, synthetic_oblique);
    [a, b, c, d, origin.0, origin.1]
}

/// Compose Fontique's faux-oblique result in the shaped run's local space
/// before CSS Writing Modes orients the run for the PDF page. PDF text
/// matrices use the same affine components as the text-run adapter, so a
/// right multiplication by `[1 0 tan(angle) 1]` preserves each run origin
/// and advances while slanting only its glyph ink.
/// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-intro>
/// <https://pdfa.org/resource/iso-32000-2/>
pub(super) fn pdf_text_matrix_components(
    text_matrix: crate::document::paint::text::RenderedTextMatrix,
    synthetic_oblique: Option<crate::document::SyntheticObliqueAngle>,
) -> [f32; 4] {
    let [a, b, c, d] = text_matrix.pdf_components();
    let Some(angle) = synthetic_oblique else {
        return [a, b, c, d];
    };
    let shear = f32::from(angle.degrees()).to_radians().tan();
    [a, b, c + a * shear, d + b * shear]
}

pub(super) fn pdf_text_matrix_transform_local_point(
    [a, b, c, d]: [f32; 4],
    point: crate::document::paint::text::TextRunPoint,
) -> crate::document::paint::text::TextRunPoint {
    crate::document::paint::text::TextRunPoint::new(
        a * point.x + c * point.y,
        b * point.x + d * point.y,
    )
}

fn needs_positioned_glyphs(
    glyphs: &[RenderedGlyph],
    metrics: PdfGlyphRunMetrics<'_>,
    continue_text_cursor: bool,
) -> bool {
    glyphs.iter().enumerate().any(|(index, glyph)| {
        ((index + 1 < glyphs.len() || continue_text_cursor && index + 1 == glyphs.len())
            && !metrics.adjustment_for(glyph).is_zero())
            || glyph.x_offset.abs() > 0.01
            || glyph.y_offset.abs() > 0.01
    })
}

fn glyph_bytes(
    glyphs: &[RenderedGlyph],
    source_gid_to_cid: &std::collections::BTreeMap<u16, u16>,
) -> Vec<u8> {
    glyphs
        .iter()
        .filter_map(RenderedGlyph::painted_id)
        .flat_map(|glyph_id| glyph_id_bytes(source_gid_to_cid[&glyph_id]))
        .collect()
}

fn glyph_id_bytes(glyph_id: u16) -> [u8; 2] {
    glyph_id.to_be_bytes()
}

fn pdf_name(name: &str) -> Name<'_> {
    Name(name.as_bytes())
}

#[cfg(test)]
mod content_tests {
    use std::rc::Rc;

    use super::*;
    use crate::document::paint::effects::{PaintBlendMode, PaintEffectScope, PaintEffects};
    use crate::document::paint::geometry::{PaintPoint, PaintRect, PaintSize, PaintStrokeWidth};
    use crate::document::paint::images::RenderedImage;
    use crate::document::paint::page::{OpaqueTextCoverage, PaintOperation, PaintPrimitive};
    use crate::document::paint::paths::{
        RenderedPath, RenderedPathClip, RenderedPathCommand, RenderedPathFillRule,
    };

    fn test_color_policy() -> PdfLoweringColorPolicy {
        PdfLoweringColorPolicy::new(
            crate::PdfProfile::Pdf,
            &super::super::colors::PdfColorRequirements::from_paint_and_image_sources(
                std::iter::empty(),
                [crate::css::CssColorSpace::Srgb],
                Vec::new(),
            ),
        )
    }

    #[test]
    fn opaque_text_coverage_elision_requires_page_root_or_proven_clip_scope() {
        let default = PaintEffects::default();
        assert!(opaque_text_coverage_elision_allowed_in_context(
            true, &default
        ));
        assert!(!opaque_text_coverage_elision_allowed_in_context(
            false, &default
        ));

        let opacity = PaintEffects {
            opacity: 0.5,
            ..PaintEffects::default()
        };
        assert!(!opaque_text_coverage_elision_allowed_in_context(
            true, &opacity
        ));
        assert!(!opaque_text_coverage_elision_allowed_in_context(
            false, &opacity
        ));
    }

    fn opaque_coverage_path(x: f32, width: f32) -> RenderedPath {
        opaque_coverage_path_with_fill(x, width, crate::CssColor::BLACK)
    }

    fn opaque_coverage_path_with_fill(x: f32, width: f32, fill: crate::CssColor) -> RenderedPath {
        let rect = PaintRect::new(PaintPoint::new(x, 0.0), PaintSize::new(width, 10.0));
        RenderedPath::new(
            vec![
                RenderedPathCommand::MoveTo(PaintPoint::new(x, 0.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(x + width, 0.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(x + width, 10.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(x, 10.0)),
                RenderedPathCommand::Close,
            ],
            Some(fill),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        )
        .with_opaque_coverage_rect(rect)
    }

    fn coverage_page(later_width: f32) -> crate::Page {
        let mut page = crate::Page::new(100.0, 100.0);
        page.paths.push(opaque_coverage_path(0.0, 10.0));
        page.paths.push(opaque_coverage_path(0.0, later_width));
        page.opaque_text_coverages.extend([
            OpaqueTextCoverage {
                line_index: 0,
                path_indices: vec![0],
            },
            OpaqueTextCoverage {
                line_index: 1,
                path_indices: vec![1],
            },
        ]);
        page
    }

    fn coverage_operation(index: usize) -> crate::document::paint::display_list::PaintDisplayItem {
        crate::document::paint::display_list::PaintDisplayItem::Operation(
            PaintOperation::OpaqueTextCoverage(index),
        )
    }

    fn line_with_ink_bounds(width: f32) -> crate::document::paint::text::RenderedLine {
        crate::document::paint::text::RenderedLine::new(
            "x".to_string(),
            0.0,
            10.0,
            10.0,
            None,
            crate::CssColor::BLACK,
            Vec::new(),
        )
        .with_glyph_ink_bounds(Some(
            crate::document::paint::geometry::PaintClip::from_paint_rect(PaintRect::new(
                PaintPoint::new(0.0, 0.0),
                PaintSize::new(width, 10.0),
            )),
        ))
    }

    fn line_operation(index: usize) -> crate::document::paint::display_list::PaintDisplayItem {
        crate::document::paint::display_list::PaintDisplayItem::Operation(PaintOperation::Line(
            index,
        ))
    }

    fn opaque_rect(x: f32, width: f32) -> crate::document::paint::shapes::RenderedRect {
        crate::document::paint::shapes::RenderedRect::new(
            x,
            0.0,
            width,
            10.0,
            Some(crate::CssColor::BLACK),
            None,
            PaintStrokeWidth::ZERO,
        )
    }

    fn visible_rects_after_later_items(
        page: &crate::Page,
        items: &[crate::document::paint::display_list::PaintDisplayItem],
    ) -> Vec<crate::document::paint::shapes::RenderedRect> {
        let mut resources = PdfResourceRegistry::default();
        let mut forms = Vec::new();
        let mut vector_paints = VectorPaintResources::default();
        let color_policy = test_color_policy();
        let state = PaintTreeRenderState {
            resources: &mut resources,
            forms: &mut forms,
            next_form_name: 1,
            form_dependency_scopes: Vec::new(),
            root_form_dependencies: BTreeMap::new(),
            vector_paints: &mut vector_paints,
            page_size: PdfSize::new(page.width(), page.height()),
            color_mode: PdfColorMode::PreserveCssSpace,
            color_policy: &color_policy,
            image_resources: &[],
            page_image_sources: &[],
            page_image_pattern_sources: &[],
            active_paint_transform: crate::document::paint::geometry::PaintTransform::identity(),
            active_filter_color_transform: crate::css::BoundedSrgbColorTransform::IDENTITY,
        };
        visible_rect_after_later_display_rects(page, items, 0, &page.rects[0], &[], &state)
    }

    #[test]
    fn serializer_elides_background_fully_covered_by_replayed_primitive_rect() {
        let mut page = crate::Page::new(100.0, 100.0);
        page.rects.push(opaque_rect(0.0, 10.0));
        let mut later_rect = opaque_rect(0.0, 10.0);
        later_rect.fill = Some(crate::CssColor::rgba(0, 128, 128, 1.0));
        let items = vec![
            crate::document::paint::display_list::PaintDisplayItem::Operation(
                PaintOperation::Rect(0),
            ),
            crate::document::paint::display_list::PaintDisplayItem::Primitive(
                PaintPrimitive::Rect(later_rect),
            ),
        ];

        assert!(visible_rects_after_later_items(&page, &items).is_empty());
    }

    #[test]
    fn serializer_retains_background_for_partial_or_effect_scoped_replayed_rect() {
        let mut page = crate::Page::new(100.0, 100.0);
        page.rects.push(opaque_rect(0.0, 10.0));
        let partial_items = vec![
            crate::document::paint::display_list::PaintDisplayItem::Operation(
                PaintOperation::Rect(0),
            ),
            crate::document::paint::display_list::PaintDisplayItem::Primitive(
                PaintPrimitive::Rect(opaque_rect(0.0, 5.0)),
            ),
        ];
        assert_eq!(
            visible_rects_after_later_items(&page, &partial_items).len(),
            1
        );

        let scoped_items = vec![
            crate::document::paint::display_list::PaintDisplayItem::Operation(
                PaintOperation::Rect(0),
            ),
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(
                PaintEffectScope::new(
                    PaintEffects {
                        opacity: 0.5,
                        ..PaintEffects::default()
                    },
                    None,
                    vec![
                        crate::document::paint::display_list::PaintDisplayItem::Primitive(
                            PaintPrimitive::Rect(opaque_rect(0.0, 10.0)),
                        ),
                    ],
                ),
            ),
        ];
        assert_eq!(
            visible_rects_after_later_items(&page, &scoped_items).len(),
            1
        );
    }

    #[test]
    fn serializer_elides_fully_covered_opaque_text_coverage() {
        let page = coverage_page(10.0);
        let items = vec![coverage_operation(0), coverage_operation(1)];

        assert!(opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &items,
            0,
            &[],
        ));
    }

    #[test]
    fn serializer_proves_coverage_through_a_later_effect_free_stacking_context() {
        let page = coverage_page(10.0);
        let mut negative_z_context = crate::document::paint::stacking::PaintStackingContext {
            source_order: 0,
            stack_level: crate::document::paint::stacking::StackLevel::Integer(-1),
            pdf_paint_boundary: false,
            bands: crate::document::paint::display_list::PaintBandList::default(),
            effects: PaintEffects::default(),
            bounds: None,
        };
        negative_z_context.bands.bands
            [crate::document::paint::display_list::PaintBand::Inline.index()]
        .push(coverage_operation(0));
        let mut later_context = crate::document::paint::stacking::PaintStackingContext {
            source_order: 1,
            stack_level: crate::document::paint::stacking::StackLevel::Auto,
            pdf_paint_boundary: false,
            bands: crate::document::paint::display_list::PaintBandList::default(),
            effects: PaintEffects::default(),
            bounds: None,
        };
        later_context.bands.bands[crate::document::paint::display_list::PaintBand::Inline.index()]
            .push(coverage_operation(1));
        let later = vec![
            crate::document::paint::display_list::PaintDisplayItem::StackingContext(later_context),
        ];

        assert!(opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &negative_z_context.bands.bands
                [crate::document::paint::display_list::PaintBand::Inline.index()],
            0,
            &[&later],
        ));
    }

    #[test]
    fn serializer_retains_partially_covered_opaque_text_coverage() {
        let page = coverage_page(5.0);
        let items = vec![coverage_operation(0), coverage_operation(1)];

        assert!(!opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &items,
            0,
            &[],
        ));
    }

    #[test]
    fn serializer_does_not_cross_effect_boundaries_for_text_coverage() {
        let page = coverage_page(10.0);
        let items = vec![
            coverage_operation(0),
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(
                PaintEffectScope::new(
                    PaintEffects {
                        opacity: 0.5,
                        ..PaintEffects::default()
                    },
                    None,
                    vec![coverage_operation(1)],
                ),
            ),
        ];

        assert!(!opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &items,
            0,
            &[],
        ));
    }

    #[test]
    fn serializer_rejects_non_direct_or_reversed_text_coverage() {
        let page = coverage_page(10.0);

        for effects in [
            PaintEffects {
                opacity: 0.5,
                ..PaintEffects::default()
            },
            PaintEffects {
                blend_mode: PaintBlendMode::Multiply,
                ..PaintEffects::default()
            },
            PaintEffects {
                isolation: true,
                ..PaintEffects::default()
            },
            PaintEffects {
                transform: Some(crate::document::paint::geometry::PaintTransform::translate(
                    crate::document::paint::geometry::PaintTranslation::new(1.0, 0.0),
                )),
                ..PaintEffects::default()
            },
        ] {
            let items = vec![
                coverage_operation(0),
                crate::document::paint::display_list::PaintDisplayItem::EffectScope(
                    PaintEffectScope::new(effects, None, vec![coverage_operation(1)]),
                ),
            ];
            assert!(!opaque_text_coverage_is_fully_hidden(
                &page,
                &page.opaque_text_coverages[0],
                &items,
                0,
                &[],
            ));
        }

        // A preceding context is not later composited paint just because it
        // contains a matching opaque path.
        let mut preceding_context = crate::document::paint::stacking::PaintStackingContext {
            source_order: 0,
            stack_level: crate::document::paint::stacking::StackLevel::Auto,
            pdf_paint_boundary: false,
            bands: crate::document::paint::display_list::PaintBandList::default(),
            effects: PaintEffects::default(),
            bounds: None,
        };
        preceding_context.bands.bands
            [crate::document::paint::display_list::PaintBand::Inline.index()]
        .push(coverage_operation(1));
        let reversed_items = vec![
            crate::document::paint::display_list::PaintDisplayItem::StackingContext(
                preceding_context,
            ),
            coverage_operation(0),
        ];
        assert!(!opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &reversed_items,
            1,
            &[],
        ));
    }

    #[test]
    fn serializer_rejects_coverage_truncated_by_a_rectangular_clip() {
        let page = coverage_page(10.0);
        let items = vec![
            coverage_operation(0),
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(
                PaintEffectScope::new(
                    PaintEffects {
                        overflow_clip_effect: Some(
                            crate::document::paint::contours::OverflowClipEffect::Rect(
                                crate::document::paint::geometry::PaintClip::new(
                                    0.0, 0.0, 5.0, 10.0,
                                ),
                            ),
                        ),
                        ..PaintEffects::default()
                    },
                    None,
                    vec![coverage_operation(1)],
                ),
            ),
        ];

        assert!(!opaque_text_coverage_is_fully_hidden(
            &page,
            &page.opaque_text_coverages[0],
            &items,
            0,
            &[],
        ));
    }

    #[test]
    fn serializer_hides_only_fully_covered_effect_free_lines() {
        let mut page = crate::Page::new(100.0, 100.0);
        page.lines.push(line_with_ink_bounds(10.0));
        page.paths.push(opaque_coverage_path(0.0, 10.0));
        let fully_covered = vec![
            line_operation(0),
            crate::document::paint::display_list::PaintDisplayItem::Operation(
                PaintOperation::Path(0),
            ),
        ];
        assert!(rendered_line_ink_is_fully_hidden(
            &page,
            &page.lines[0],
            &fully_covered,
            0,
            &[],
            None,
        ));

        page.paths[0] = opaque_coverage_path(0.0, 5.0);
        assert!(!rendered_line_ink_is_fully_hidden(
            &page,
            &page.lines[0],
            &fully_covered,
            0,
            &[],
            None,
        ));

        page.paths[0] = opaque_coverage_path(0.0, 10.0);
        let effect_scoped = vec![
            line_operation(0),
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(
                PaintEffectScope::new(
                    PaintEffects {
                        opacity: 0.5,
                        ..PaintEffects::default()
                    },
                    None,
                    vec![
                        crate::document::paint::display_list::PaintDisplayItem::Operation(
                            PaintOperation::Path(0),
                        ),
                    ],
                ),
            ),
        ];
        assert!(!rendered_line_ink_is_fully_hidden(
            &page,
            &page.lines[0],
            &effect_scoped,
            0,
            &[],
            None,
        ));
    }

    #[test]
    fn opaque_text_coverage_rectangles_share_one_pending_fill() {
        let first = opaque_coverage_path(0.0, 10.0);
        let second = opaque_coverage_path(10.0, 10.0);
        let mut pending = PendingFillRects::default();

        for path in [&first, &second] {
            let rect = opaque_text_coverage_rect_for_batch(path, PdfColorMode::PreserveCssSpace)
                .expect("identity opaque full-em rectangle is batchable");
            assert!(pending.try_push(&rect));
        }

        assert_eq!(pending.rects.len(), 1);
        assert_eq!(pending.rects[0].x(), 0.0);
        assert_eq!(pending.rects[0].width(), 20.0);
    }

    #[test]
    fn pending_fills_batch_overlapping_opaque_rectangles_as_one_union() {
        let exact = opaque_rect(0.0, 10.0);
        let partial_overlap = opaque_rect(5.0, 10.0);
        let mut pending = PendingFillRects::default();

        assert!(pending.try_push(&exact));
        assert!(pending.try_push(&exact));
        assert_eq!(pending.rects.len(), 1);
        assert!(pending.try_push(&partial_overlap));
        assert_eq!(pending.rects.len(), 2);
    }

    #[test]
    fn solid_image_coverage_uses_semantic_destination_clip_identity() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0));
        let destination_clip =
            RenderedPathClip::new(Vec::new(), RenderedPathFillRule::NonZero, Vec::new());
        let image = RenderedImage::from_paint_rect(
            rect,
            false,
            1,
            1,
            None,
            false,
            Rc::from([0_u8, 128, 0]),
            None,
            None,
        )
        .with_destination_rect_clip(destination_clip);
        let fill = SolidImageFill {
            color_space: crate::color::RasterColorSpace::SRGB,
            components: [0, 128, 0],
        };

        assert!(solid_image_fill_as_rect(&image, &fill).is_some());
    }

    #[test]
    fn opaque_text_coverage_batching_preserves_paint_boundaries() {
        let black = opaque_coverage_path(0.0, 10.0);
        let red = opaque_coverage_path_with_fill(10.0, 10.0, crate::CssColor::rgba(255, 0, 0, 1.0));
        let rotated = opaque_coverage_path(20.0, 10.0).with_transform(
            crate::document::paint::geometry::PaintTransform::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
        );
        let black_rect =
            opaque_text_coverage_rect_for_batch(&black, PdfColorMode::PreserveCssSpace).unwrap();
        let red_rect =
            opaque_text_coverage_rect_for_batch(&red, PdfColorMode::PreserveCssSpace).unwrap();
        let mut pending = PendingFillRects::default();
        assert!(pending.try_push(&black_rect));
        assert!(
            !pending.try_push(&red_rect),
            "different opaque colors must keep their author paint boundary"
        );
        assert!(
            opaque_text_coverage_rect_for_batch(&rotated, PdfColorMode::PreserveCssSpace).is_none(),
            "a rotated glyph outline must remain an ordinary vector path"
        );
    }

    #[test]
    fn bitmap_glyph_image_writes_actual_text_marked_content() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            false,
            1,
            1,
            None,
            false,
            Rc::from([0_u8, 0, 0]),
            None,
            None,
        )
        .with_actual_text(Rc::from("⚪"));
        let mut content = Content::new();
        let color_policy = test_color_policy();
        write_image(
            &mut content,
            &image,
            0,
            &PreparedImageResource::Raster(ImageResource {
                pixel_width: 1,
                pixel_height: 1,
                interpolate: false,
                color_space: crate::color::RasterColorSpace::SRGB,
                sample_depth: crate::image_store::RasterSampleDepth::Eight,
                payload: ImagePayload::Samples {
                    rgb: vec![0, 0, 0],
                    alpha: None,
                },
            }),
            &color_policy,
        );
        let bytes = content.finish().into_vec();
        let content = String::from_utf8_lossy(&bytes);
        assert!(content.contains("ActualText"));
        assert!(content.contains("/Im1 Do"));
        assert!(content.contains("EMC"));
    }

    #[test]
    fn transparent_image_retains_actual_text_without_a_pdf_paint_operation() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            false,
            1,
            1,
            None,
            true,
            Rc::from([255_u8, 255, 255]),
            Some(Rc::from([0_u8])),
            None,
        )
        .with_actual_text(Rc::from("transparent"));
        let mut content = Content::new();
        let color_policy = test_color_policy();
        write_image(
            &mut content,
            &image,
            0,
            &PreparedImageResource::Transparent,
            &color_policy,
        );
        let bytes = content.finish().into_vec();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("ActualText"));
        assert!(content.contains("EMC"));
        assert!(!content.contains(" Do"));
    }

    #[test]
    fn opaque_uniform_image_writes_an_icc_tagged_fill_without_an_xobject() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            false,
            1,
            1,
            None,
            false,
            Rc::from([0_u8, 128, 0]),
            None,
            None,
        )
        .with_actual_text(Rc::from("green"));
        let mut content = Content::new();
        let color_policy = test_color_policy();
        write_image(
            &mut content,
            &image,
            0,
            &PreparedImageResource::SolidFill(SolidImageFill {
                color_space: crate::color::RasterColorSpace::SRGB,
                components: [0, 128, 0],
            }),
            &color_policy,
        );
        let bytes = content.finish().into_vec();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/CSsRGB cs"));
        assert!(content.contains("10 20 4 5 re"));
        assert!(content.contains("ActualText"));
        assert!(!content.contains(" Do"));
    }

    #[test]
    fn opaque_uniform_image_fill_preserves_a_non_destination_clip() {
        let image = RenderedImage::from_paint_rect(
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            false,
            1,
            1,
            None,
            false,
            Rc::from([0_u8, 128, 0]),
            None,
            None,
        )
        .with_actual_text(Rc::from("green"))
        .with_clip(RenderedPathClip::new(
            vec![
                RenderedPathCommand::MoveTo(PaintPoint::new(11.0, 20.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(14.0, 20.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(14.0, 25.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(11.0, 25.0)),
                RenderedPathCommand::Close,
            ],
            RenderedPathFillRule::NonZero,
            Vec::new(),
        ));
        let mut content = Content::new();
        let color_policy = test_color_policy();
        write_image(
            &mut content,
            &image,
            0,
            &PreparedImageResource::SolidFill(SolidImageFill {
                color_space: crate::color::RasterColorSpace::SRGB,
                components: [0, 128, 0],
            }),
            &color_policy,
        );
        let content = String::from_utf8_lossy(&content.finish().into_vec()).into_owned();

        assert!(content.contains("W\nn"));
        assert!(content.contains("ActualText"));
        assert!(content.contains("/CSsRGB cs"));
    }

    #[test]
    fn tiling_pattern_fill_uses_its_paint_path_without_a_duplicate_clip() {
        let mut content = Content::new();
        write_tiling_pattern_fill(
            &mut content,
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            "P1",
            None,
        );
        let content = String::from_utf8_lossy(&content.finish().into_vec()).into_owned();

        assert!(content.contains("/Pattern cs"));
        assert!(content.contains("/P1 scn"));
        assert!(content.contains("10 20 4 5 re"));
        assert!(content.contains("f\n"));
        assert!(
            !content.contains("W\nn"),
            "the pattern fill path must not first install an identical clip: {content}"
        );
    }

    #[test]
    fn tiling_pattern_fill_retains_an_authored_clip() {
        let clip = RenderedPathClip::new(
            vec![
                RenderedPathCommand::MoveTo(PaintPoint::new(11.0, 20.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(14.0, 20.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(14.0, 25.0)),
                RenderedPathCommand::LineTo(PaintPoint::new(11.0, 25.0)),
                RenderedPathCommand::Close,
            ],
            RenderedPathFillRule::NonZero,
            Vec::new(),
        );
        let mut content = Content::new();
        write_tiling_pattern_fill(
            &mut content,
            PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(4.0, 5.0)),
            "P1",
            Some(&clip),
        );
        let content = String::from_utf8_lossy(&content.finish().into_vec()).into_owned();

        assert_eq!(content.matches("W\nn").count(), 1, "content={content}");
        assert!(content.contains("/P1 scn"));
        assert!(content.contains("10 20 4 5 re"));
    }

    #[test]
    fn pdf_text_space_adjustment_uses_the_serialized_integer_width() {
        let width = PdfTextSpaceWidth::from_font_units(455, 2048);
        assert_eq!(width, PdfTextSpaceWidth(222));

        let font_size = PdfTextFontSize::new(12.0);
        let used_advance = 455.0 * font_size.points() / 2048.0;
        let adjustment =
            PdfTextSpaceAdjustment::for_used_advance(Some(width), font_size, used_advance);

        assert!(!adjustment.is_zero());
        assert!((width.points_at(font_size.points()) - used_advance).abs() < 0.01);
    }

    #[test]
    fn glyphs_with_a_pdf_width_rounding_delta_emit_tj() {
        let source_gid_to_cid = BTreeMap::from([(1, 1), (2, 2)]);
        let source_gid_to_width = BTreeMap::from([
            (1, PdfTextSpaceWidth::from_font_units(455, 2048)),
            (2, PdfTextSpaceWidth(500)),
        ]);
        let metrics = PdfGlyphRunMetrics {
            source_gid_to_cid: &source_gid_to_cid,
            source_gid_to_width: &source_gid_to_width,
            default_width: PdfTextSpaceWidth(0),
            font_size: PdfTextFontSize::new(12.0),
        };
        let used_advance = 455.0 * 12.0 / 2048.0;
        let glyphs = vec![
            RenderedGlyph {
                kind: crate::document::paint::text::RenderedGlyphKind::Paint(1),
                x_advance: used_advance,
                nominal_x_advance: used_advance,
                x_offset: 0.0,
                y_offset: 0.0,
                unicode: "A".to_string(),
            },
            RenderedGlyph {
                kind: crate::document::paint::text::RenderedGlyphKind::Paint(2),
                x_advance: 6.0,
                nominal_x_advance: 6.0,
                x_offset: 0.0,
                y_offset: 0.0,
                unicode: "B".to_string(),
            },
        ];
        let mut content = Content::new();
        content.begin_text();
        write_glyphs(
            &mut content,
            &glyphs,
            metrics,
            false,
            TextRenderingMode::Fill,
            false,
        );
        content.end_text();
        let content = String::from_utf8_lossy(&content.finish().into_vec()).into_owned();

        assert!(content.contains("TJ"), "content={content}");
    }

    #[test]
    fn cursor_continuation_emits_the_final_pdf_text_space_adjustment() {
        let source_gid_to_cid = BTreeMap::from([(1, 1)]);
        let source_gid_to_width =
            BTreeMap::from([(1, PdfTextSpaceWidth::from_font_units(455, 2048))]);
        let metrics = PdfGlyphRunMetrics {
            source_gid_to_cid: &source_gid_to_cid,
            source_gid_to_width: &source_gid_to_width,
            default_width: PdfTextSpaceWidth(0),
            font_size: PdfTextFontSize::new(12.0),
        };
        let glyphs = vec![RenderedGlyph {
            kind: crate::document::paint::text::RenderedGlyphKind::Paint(1),
            x_advance: 455.0 * 12.0 / 2048.0,
            nominal_x_advance: 455.0 * 12.0 / 2048.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "A".to_string(),
        }];

        let mut ordinary = Content::new();
        ordinary.begin_text();
        write_glyphs(
            &mut ordinary,
            &glyphs,
            metrics,
            false,
            TextRenderingMode::Fill,
            false,
        );
        ordinary.end_text();
        let ordinary = String::from_utf8_lossy(&ordinary.finish().into_vec()).into_owned();
        assert!(!ordinary.contains("TJ"), "content={ordinary}");

        let mut continuation = Content::new();
        continuation.begin_text();
        write_glyphs(
            &mut continuation,
            &glyphs,
            metrics,
            false,
            TextRenderingMode::Fill,
            true,
        );
        continuation.end_text();
        let continuation = String::from_utf8_lossy(&continuation.finish().into_vec()).into_owned();
        assert!(continuation.contains("TJ"), "content={continuation}");
    }

    #[test]
    fn pdf_cursor_continuation_rejects_a_gap_or_transformed_run() {
        let glyphs = [RenderedGlyph {
            kind: crate::document::paint::text::RenderedGlyphKind::Paint(1),
            x_advance: 7.0,
            nominal_x_advance: 7.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "A".to_string(),
        }];
        let previous = super::super::text::PdfTextRun {
            document_font_id: 0,
            actual_text: None,
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: crate::document::paint::text::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            glyphs: &glyphs,
        };
        let mut next = super::super::text::PdfTextRun {
            document_font_id: 0,
            actual_text: None,
            x_offset: 7.0,
            y_offset: 0.0,
            text_matrix: crate::document::paint::text::RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            glyphs: &glyphs,
        };

        assert!(pdf_text_runs_can_share_cursor(&previous, &next, true, true));

        next.x_offset = 7.01;
        assert!(!pdf_text_runs_can_share_cursor(
            &previous, &next, true, true
        ));

        next.x_offset = 7.0;
        next.text_matrix =
            crate::document::paint::text::RenderedTextMatrix::from_pdf_linear_components([
                1.0, 0.0, 0.1, 1.0,
            ])
            .unwrap();
        assert!(!pdf_text_runs_can_share_cursor(
            &previous, &next, true, true
        ));
    }
}
