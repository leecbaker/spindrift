use super::colors::{PdfColorMode, set_fill_color, set_stroke_color};
use super::*;
use crate::document::{
    RenderedGradient, RenderedPathCommandPoints, RenderedPathLineCap, RenderedPathLineJoin,
    RenderedPathPaint, RenderedPathPaintOrder,
};
use pdf_writer::types::{ColorSpaceOperand, LineCapStyle, LineJoinStyle};
use pdf_writer::{Content, Name, Str, TextStr};
use std::collections::BTreeMap;

pub(super) fn page_content_render(
    page: &crate::Page,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    next_object_id: &mut usize,
    color_mode: PdfColorMode,
) -> PageContentRender {
    let mut content = Content::new();
    let mut forms = Vec::new();
    let mut vector_paints = VectorPaintResources::default();
    let mut state = PaintTreeRenderState {
        next_object_id,
        forms: &mut forms,
        next_form_name: 1,
        form_dependency_scopes: Vec::new(),
        vector_paints: &mut vector_paints,
        page_width: page.width(),
        page_height: page.height(),
        color_mode,
    };
    write_paint_tree(
        &mut content,
        page,
        page.paint_tree(),
        embedded_fonts,
        &mut state,
    );
    PageContentRender {
        stream: content.finish().into_vec(),
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
    tree: &crate::document::PagePaintTree,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    write_stacking_context(content, page, &tree.root, embedded_fonts, state);
}

struct PaintTreeRenderState<'a, 'b> {
    next_object_id: &'a mut usize,
    forms: &'b mut Vec<FormXObjectRender>,
    next_form_name: usize,
    form_dependency_scopes: Vec<BTreeMap<String, FormXObjectReference>>,
    vector_paints: &'b mut VectorPaintResources,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
}

impl PaintTreeRenderState<'_, '_> {
    /// Reserve a unique Form resource name before recursively emitting its
    /// contents. Nested effect groups must not derive names from the number of
    /// completed forms, because their parent has not been recorded yet.
    fn reserve_transparency_form(&mut self) -> FormXObjectReference {
        let reference = FormXObjectReference {
            id: *self.next_object_id,
            name: format!("Fm{}", self.next_form_name),
        };
        *self.next_object_id += 1;
        self.next_form_name += 1;
        reference
    }

    fn begin_form_dependencies(&mut self) {
        self.form_dependency_scopes.push(BTreeMap::new());
    }

    fn finish_form_dependencies(&mut self) -> Vec<FormXObjectReference> {
        self.form_dependency_scopes
            .pop()
            .expect("form dependency scope is active")
            .into_values()
            .collect()
    }

    fn record_form_dependency(&mut self, reference: FormXObjectReference) {
        if let Some(dependencies) = self.form_dependency_scopes.last_mut() {
            dependencies.insert(reference.name.clone(), reference);
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
}

impl VectorPaintResources {
    fn svg_path_tiling_resource(
        &mut self,
        pattern: &crate::document::RenderedSvgPathPattern,
        next_object_id: &mut usize,
    ) -> String {
        let name = format!("SVP{}", self.svg_path_tilings.len() + 1);
        let id = *next_object_id;
        *next_object_id += 1;
        self.svg_path_tilings.push(SvgPathTilingPatternPlan {
            id,
            name: name.clone(),
            pattern: pattern.clone(),
        });
        name
    }

    fn tiling_gradient_resource(
        &mut self,
        pattern: &crate::document::RenderedGradientPattern,
        next_object_id: &mut usize,
        page_width: f32,
        page_height: f32,
    ) -> String {
        let shading =
            self.gradient_resource(&pattern.gradient, next_object_id, page_width, page_height);
        let name = format!("GP{}", self.tilings.len() + 1);
        let id = *next_object_id;
        *next_object_id += 1;
        self.tilings.push(GradientTilingPatternPlan {
            id,
            name: name.clone(),
            shading_pattern_name: shading.pattern_name,
            alpha_gstate_name: shading.alpha_gstate_name,
            pattern: pattern.clone(),
        });
        name
    }
}

#[derive(Clone)]
struct GradientPaintResource {
    pattern_name: String,
    alpha_gstate_name: Option<String>,
}

impl VectorPaintResources {
    fn gradient_resource(
        &mut self,
        gradient: &RenderedGradient,
        next_object_id: &mut usize,
        page_width: f32,
        page_height: f32,
    ) -> GradientPaintResource {
        let key = gradient_key(gradient);
        if let Some(resource) = self.gradients.get(&key) {
            return resource.clone();
        }
        let name = format!("SG{}", self.plans.len() + 1);
        let id = *next_object_id;
        *next_object_id += 1;
        let function_count = if gradient.periodic.is_some() || gradient.stops.len() == 2 {
            1
        } else {
            gradient.stops.len()
        };
        let function_ids = (0..function_count)
            .map(|_| {
                let id = *next_object_id;
                *next_object_id += 1;
                id
            })
            .collect();
        let alpha = gradient.has_transparent_stop().then(|| {
            let pattern_id = *next_object_id;
            *next_object_id += 1;
            let alpha_function_ids = (0..function_count)
                .map(|_| {
                    let id = *next_object_id;
                    *next_object_id += 1;
                    id
                })
                .collect();
            let form_id = *next_object_id;
            *next_object_id += 1;
            let ext_gstate_id = *next_object_id;
            *next_object_id += 1;
            GradientAlphaPlan {
                pattern_id,
                pattern_name: format!("SGA{}", self.plans.len() + 1),
                function_ids: alpha_function_ids,
                form_id,
                ext_gstate_id,
                ext_gstate_name: format!("GSsvgAlpha{}", self.plans.len() + 1),
                page_width,
                page_height,
            }
        });
        let resource = GradientPaintResource {
            pattern_name: name.clone(),
            alpha_gstate_name: alpha.as_ref().map(|alpha| alpha.ext_gstate_name.clone()),
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

fn write_stacking_context(
    content: &mut Content,
    page: &crate::Page,
    context: &crate::document::PaintStackingContext,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    if context.effects.suppresses_paint() {
        return;
    }
    if context.effects.needs_group() {
        write_effect_group(content, page, context, embedded_fonts, state);
        return;
    }
    let items = crate::document::PaintBand::ORDER
        .into_iter()
        .flat_map(|band| context.bands.bands[band.index()].iter().cloned())
        .collect::<Vec<_>>();
    // Overflow and containment clips are semantic paint operations.  Keep
    // their scopes even if the currently recorded primitives happen to fit:
    // deferred descendants and later fragment replay can still depend on the
    // clipping boundary.
    let elide_redundant_rect_clips = false;
    let effect_steps = context
        .effects
        .ordered_steps()
        .into_iter()
        .filter(|step| {
            !(elide_redundant_rect_clips
                && matches!(step, crate::document::PaintEffectStep::Clip(_)))
        })
        .collect::<Vec<_>>();
    let scoped = !effect_steps.is_empty();
    if scoped {
        content.save_state();
    }
    for step in effect_steps {
        match step {
            crate::document::PaintEffectStep::Clip(clip)
                if !clip_is_page_media_box(clip, state) =>
            {
                write_rect_clip(content, clip)
            }
            crate::document::PaintEffectStep::RoundedClip(clip) => {
                write_rounded_clip(content, &clip)
            }
            crate::document::PaintEffectStep::Transform(transform) => {
                content.transform(transform.pdf_components());
            }
            crate::document::PaintEffectStep::Clip(_)
            | crate::document::PaintEffectStep::ClipPath(_)
            | crate::document::PaintEffectStep::Filter(_)
            | crate::document::PaintEffectStep::Mask(_)
            | crate::document::PaintEffectStep::Opacity(_)
            | crate::document::PaintEffectStep::Blend(_)
            | crate::document::PaintEffectStep::Isolation => {}
        }
    }
    write_display_items(
        content,
        page,
        &items,
        embedded_fonts,
        state,
        context
            .effects
            .ordered_steps()
            .iter()
            .any(|step| matches!(step, crate::document::PaintEffectStep::Clip(_))),
    );
    if scoped {
        content.restore_state();
    }
}

fn write_effect_scope(
    content: &mut Content,
    page: &crate::Page,
    scope: &crate::document::PaintEffectScope,
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
    // See `write_stacking_context`: a retained clip must not be optimized
    // away merely because this particular recorded subtree is in-bounds.
    let elide_redundant_rect_clips = false;
    let effect_steps = scope
        .effects
        .ordered_steps()
        .into_iter()
        .filter(|step| {
            !(elide_redundant_rect_clips
                && matches!(step, crate::document::PaintEffectStep::Clip(_)))
        })
        .collect::<Vec<_>>();
    let scoped = !effect_steps.is_empty();
    if scoped {
        content.save_state();
    }
    for step in effect_steps {
        match step {
            crate::document::PaintEffectStep::Clip(clip)
                if !clip_is_page_media_box(clip, state) =>
            {
                write_rect_clip(content, clip)
            }
            crate::document::PaintEffectStep::RoundedClip(clip) => {
                write_rounded_clip(content, &clip)
            }
            crate::document::PaintEffectStep::Transform(transform) => {
                content.transform(transform.pdf_components());
            }
            crate::document::PaintEffectStep::Clip(_)
            | crate::document::PaintEffectStep::ClipPath(_)
            | crate::document::PaintEffectStep::Filter(_)
            | crate::document::PaintEffectStep::Mask(_)
            | crate::document::PaintEffectStep::Opacity(_)
            | crate::document::PaintEffectStep::Blend(_)
            | crate::document::PaintEffectStep::Isolation => {}
        }
    }
    write_display_items(
        content,
        page,
        &scope.items,
        embedded_fonts,
        state,
        scope
            .effects
            .ordered_steps()
            .iter()
            .any(|step| matches!(step, crate::document::PaintEffectStep::Clip(_))),
    );
    if scoped {
        content.restore_state();
    }
}

/// Serialize an in-band SVG/CSS effect scope through an isolated Form XObject.
///
/// Group opacity and blend modes apply after all descendants have composed,
/// so painting children one by one would change overlapping SVG geometry.
fn write_effect_scope_group(
    content: &mut Content,
    page: &crate::Page,
    scope: &crate::document::PaintEffectScope,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let reference = state.reserve_transparency_form();
    let bbox = scope.bounds.unwrap_or(crate::document::PaintClip::new(
        0.0,
        0.0,
        state.page_width,
        state.page_height,
    ));
    let mut form_content = Content::new();
    let mut form_scope = scope.clone();
    form_scope.effects = form_scope.effects.without_group_effects();
    state.begin_form_dependencies();
    write_effect_scope(&mut form_content, page, &form_scope, embedded_fonts, state);
    let form_dependencies = state.finish_form_dependencies();
    state.forms.push(FormXObjectRender {
        id: reference.id,
        name: reference.name.clone(),
        form_dependencies,
        bbox,
        stream: form_content.finish().into_vec(),
        transparency_group: true,
    });
    content.save_state();
    if scope.effects.opacity < 1.0 {
        let alpha = crate::Color::TRANSPARENT.with_alpha(scope.effects.opacity);
        if let Some(resource_name) = paint_alpha_resource_name(alpha) {
            content.set_parameters(pdf_name(&resource_name));
        }
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
    context: &crate::document::PaintStackingContext,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let reference = state.reserve_transparency_form();
    let bbox = context.effect_bounds(crate::document::PaintClip::new(
        0.0,
        0.0,
        state.page_width,
        state.page_height,
    ));
    let mut form_content = Content::new();
    let mut form_context = context.clone();
    form_context.effects = form_context.effects.without_group_effects();
    state.begin_form_dependencies();
    write_stacking_context(
        &mut form_content,
        page,
        &form_context,
        embedded_fonts,
        state,
    );
    let form_dependencies = state.finish_form_dependencies();
    state.forms.push(FormXObjectRender {
        id: reference.id,
        name: reference.name.clone(),
        form_dependencies,
        bbox,
        stream: form_content.finish().into_vec(),
        transparency_group: true,
    });
    content.save_state();
    if context.effects.opacity < 1.0 {
        let alpha = crate::Color::TRANSPARENT.with_alpha(context.effects.opacity);
        if let Some(resource_name) = paint_alpha_resource_name(alpha) {
            content.set_parameters(pdf_name(&resource_name));
        }
    }
    if let Some(resource_name) = context.effects.blend_mode.resource_name() {
        content.set_parameters(pdf_name(&resource_name));
    }
    content.x_object(pdf_name(&reference.name));
    state.record_form_dependency(reference);
    content.restore_state();
}

fn write_display_items(
    content: &mut Content,
    page: &crate::Page,
    items: &[crate::document::PaintDisplayItem],
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
    cull_hidden_opaque_background: bool,
) {
    let mut pending_rects = PendingFillRects::default();
    for (item_index, item) in items.iter().enumerate() {
        if let Some(rect) = display_item_rect(page, item) {
            let rects = if cull_hidden_opaque_background || is_opaque_fill_rect(rect) {
                visible_rect_after_later_display_rects(page, items, item_index, rect)
            } else {
                vec![rect.clone()]
            };
            for rect in rects {
                if is_mergeable_fill_rect(&rect) {
                    if !pending_rects.try_push(&rect) {
                        flush_pending_rects(content, &mut pending_rects, state.color_mode);
                        let pushed = pending_rects.try_push(&rect);
                        debug_assert!(pushed);
                    }
                } else {
                    flush_pending_rects(content, &mut pending_rects, state.color_mode);
                    write_rect(content, &rect, state.color_mode);
                }
            }
        } else {
            flush_pending_rects(content, &mut pending_rects, state.color_mode);
            write_display_item(content, page, item, embedded_fonts, state);
        }
    }
    flush_pending_rects(content, &mut pending_rects, state.color_mode);
}

fn display_item_rect<'a>(
    page: &'a crate::Page,
    item: &crate::document::PaintDisplayItem,
) -> Option<&'a crate::RenderedRect> {
    match item {
        crate::document::PaintDisplayItem::Operation(crate::PaintOperation::Rect(index)) => {
            page.rects.get(*index)
        }
        _ => None,
    }
}

/// Remove only the fully obscured portions of an opaque rectangular fill.
///
/// This preserves display-list order and never traverses an effect-free
/// stacking context: containment's stacking boundary remains intact. It
/// prevents PDF antialiasing from sampling a color that CSS compositing has
/// already hidden under a later opaque background.
fn visible_rect_after_later_display_rects(
    page: &crate::Page,
    items: &[crate::document::PaintDisplayItem],
    item_index: usize,
    rect: &crate::RenderedRect,
) -> Vec<crate::RenderedRect> {
    if !is_opaque_fill_rect(rect) {
        return vec![rect.clone()];
    }
    let mut covers = Vec::new();
    for item in &items[item_index + 1..] {
        collect_later_opaque_rects(page, item, &mut covers);
    }
    // PDF output retains the authored paint primitive unless later opaque
    // painting hides *all* of it. Splitting a partially covered primitive
    // changes the retained display-list geometry (and, in particular, loses
    // the continuous gap-rule path that CSS specifies). The fully-covered
    // case is still safe to elide and avoids an unnecessary underpaint.
    if rect_area_is_covered_by_rects(rect, &covers) {
        Vec::new()
    } else {
        vec![rect.clone()]
    }
}

fn collect_later_opaque_rects<'a>(
    page: &'a crate::Page,
    item: &'a crate::document::PaintDisplayItem,
    rects: &mut Vec<&'a crate::RenderedRect>,
) {
    match item {
        crate::document::PaintDisplayItem::Operation(crate::PaintOperation::Rect(index)) => {
            if let Some(rect) = page.rects.get(*index) {
                rects.push(rect);
            }
        }
        crate::document::PaintDisplayItem::StackingContext(context)
            if effects_are_rectangular_clips_only(context.effects) =>
        {
            for band in crate::document::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_later_opaque_rects(page, child, rects);
                }
            }
        }
        crate::document::PaintDisplayItem::EffectScope(scope)
            if effects_are_rectangular_clips_only(scope.effects) =>
        {
            for child in &scope.items {
                collect_later_opaque_rects(page, child, rects);
            }
        }
        _ => {}
    }
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
    effects: crate::document::PaintEffects,
    items: &[crate::document::PaintDisplayItem],
) -> bool {
    let clips = effects
        .ordered_steps()
        .iter()
        .filter_map(|step| match step {
            crate::document::PaintEffectStep::Clip(clip) => Some(*clip),
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
    item: &crate::document::PaintDisplayItem,
    clips: &[crate::document::PaintClip],
) -> bool {
    match item {
        crate::document::PaintDisplayItem::Operation(crate::PaintOperation::Rect(index)) => page
            .rects
            .get(*index)
            .is_some_and(|rect| clips.iter().all(|clip| rect_is_within_clip(rect, *clip))),
        crate::document::PaintDisplayItem::Primitive(crate::document::PaintPrimitive::Rect(
            rect,
        )) => clips.iter().all(|clip| rect_is_within_clip(rect, *clip)),
        crate::document::PaintDisplayItem::StackingContext(context) => {
            effects_are_rectangular_clips_only(context.effects)
                && crate::document::PaintBand::ORDER.into_iter().all(|band| {
                    context.bands.bands[band.index()]
                        .iter()
                        .all(|child| display_item_is_rect_within_active_clips(page, child, clips))
                })
        }
        crate::document::PaintDisplayItem::EffectScope(scope) => {
            effects_are_rectangular_clips_only(scope.effects)
                && scope
                    .items
                    .iter()
                    .all(|child| display_item_is_rect_within_active_clips(page, child, clips))
        }
        crate::document::PaintDisplayItem::Link(_) => true,
        crate::document::PaintDisplayItem::Operation(_)
        | crate::document::PaintDisplayItem::Primitive(_) => false,
    }
}

fn effects_are_rectangular_clips_only(effects: crate::document::PaintEffects) -> bool {
    !effects.needs_group()
        && effects
            .ordered_steps()
            .iter()
            .all(|step| matches!(step, crate::document::PaintEffectStep::Clip(_)))
}

#[allow(dead_code)]
fn rect_is_within_clip(rect: &crate::RenderedRect, clip: crate::document::PaintClip) -> bool {
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
    effects: crate::document::PaintEffects,
    items: &[crate::document::PaintDisplayItem],
) -> Option<Vec<crate::RenderedRect>> {
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
    effects: crate::document::PaintEffects,
) -> Option<Vec<crate::document::PaintClip>> {
    if !effects_are_rectangular_clips_only(effects) {
        return None;
    }
    Some(
        effects
            .ordered_steps()
            .iter()
            .filter_map(|step| match step {
                crate::document::PaintEffectStep::Clip(clip) => Some(*clip),
                _ => None,
            })
            .collect(),
    )
}

#[allow(dead_code)]
fn collect_contained_opaque_rects(
    page: &crate::Page,
    item: &crate::document::PaintDisplayItem,
    ancestor_clips: &[crate::document::PaintClip],
    rects: &mut Vec<crate::RenderedRect>,
) -> Option<()> {
    match item {
        crate::document::PaintDisplayItem::Operation(crate::PaintOperation::Rect(index)) => {
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
        crate::document::PaintDisplayItem::Primitive(crate::document::PaintPrimitive::Rect(
            rect,
        )) => {
            if !is_opaque_fill_rect(rect)
                || !ancestor_clips
                    .iter()
                    .all(|clip| rect_is_within_clip(rect, *clip))
            {
                return None;
            }
            rects.push(rect.clone());
        }
        crate::document::PaintDisplayItem::StackingContext(context) => {
            let mut clips = ancestor_clips.to_vec();
            clips.extend(rectangular_clips(context.effects)?);
            for band in crate::document::PaintBand::ORDER {
                for child in &context.bands.bands[band.index()] {
                    collect_contained_opaque_rects(page, child, &clips, rects)?;
                }
            }
        }
        crate::document::PaintDisplayItem::EffectScope(scope) => {
            let mut clips = ancestor_clips.to_vec();
            clips.extend(rectangular_clips(scope.effects)?);
            for child in &scope.items {
                collect_contained_opaque_rects(page, child, &clips, rects)?;
            }
        }
        crate::document::PaintDisplayItem::Link(_) => {}
        crate::document::PaintDisplayItem::Operation(_)
        | crate::document::PaintDisplayItem::Primitive(_) => return None,
    }
    Some(())
}

#[allow(dead_code)]
fn write_contained_opaque_rect_layer(content: &mut Content, rects: Vec<crate::RenderedRect>) {
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
        write_rect(content, rect, PdfColorMode::PreserveCssSpace);
    }
}

fn visible_after_later_opaque_rects<'a>(
    rect: &crate::RenderedRect,
    later: impl Iterator<Item = &'a crate::RenderedRect>,
) -> Vec<crate::RenderedRect> {
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
    rect: crate::RenderedRect,
    cover: &crate::RenderedRect,
) -> Vec<crate::RenderedRect> {
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
            fragment.set_paint_rect(crate::document::PaintRect::new(
                crate::document::PaintPoint::new(x, y),
                crate::document::PaintSize::new(width, height),
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

fn is_opaque_fill_rect(rect: &crate::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.a >= 1.0)
}

fn write_display_item(
    content: &mut Content,
    page: &crate::Page,
    item: &crate::document::PaintDisplayItem,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match item {
        crate::document::PaintDisplayItem::Operation(operation) => {
            write_page_operation(content, page, operation, embedded_fonts, state);
        }
        crate::document::PaintDisplayItem::StackingContext(context) => {
            write_stacking_context(content, page, context, embedded_fonts, state);
        }
        crate::document::PaintDisplayItem::EffectScope(scope) => {
            write_effect_scope(content, page, scope, embedded_fonts, state);
        }
        crate::document::PaintDisplayItem::Primitive(_)
        | crate::document::PaintDisplayItem::Link(_) => {}
    }
}

fn write_page_operation(
    content: &mut Content,
    page: &crate::Page,
    operation: &crate::PaintOperation,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    match operation {
        crate::PaintOperation::Rect(index) => {
            if let Some(rect) = page.rects.get(*index) {
                write_rect(content, rect, state.color_mode);
            }
        }
        crate::PaintOperation::RoundedRect(index) => {
            if let Some(rect) = page.rounded_rects.get(*index) {
                write_rounded_rect(content, rect, state.color_mode);
            }
        }
        crate::PaintOperation::Path(index) => {
            if let Some(path) = page.paths.get(*index) {
                write_path(
                    content,
                    path,
                    state.vector_paints,
                    state.next_object_id,
                    state.page_width,
                    state.page_height,
                    state.color_mode,
                );
            }
        }
        crate::PaintOperation::Stroke(index) => {
            if let Some(stroke) = page.strokes.get(*index) {
                write_stroke(content, stroke, state.color_mode);
            }
        }
        crate::PaintOperation::Image(index) => {
            if let Some(image) = page.images.get(*index) {
                write_image(content, image, *index);
            }
        }
        crate::PaintOperation::ImagePattern(index) => {
            if let Some(pattern) = page.image_patterns.get(*index) {
                write_image_pattern(content, pattern, *index);
            }
        }
        crate::PaintOperation::GradientPattern(index) => {
            if let Some(pattern) = page.gradient_patterns.get(*index) {
                write_gradient_tiling_pattern(
                    content,
                    pattern,
                    state.vector_paints,
                    state.next_object_id,
                    state.page_width,
                    state.page_height,
                );
            }
        }
        crate::PaintOperation::SvgPattern(index) => {
            if let Some(pattern) = page.svg_patterns.get(*index) {
                write_svg_tiling_pattern(content, pattern, state);
            }
        }
        crate::PaintOperation::Line(index) => {
            if let Some(line) = page.lines.get(*index) {
                write_line(content, line, embedded_fonts, state.color_mode);
            }
        }
    }
}

fn write_rect_clip(content: &mut Content, clip: crate::document::PaintClip) {
    let rect = crate::document::paint_rect_to_pdf(clip.paint_rect());
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

/// PDF page media boxes already clip all page content. Re-emitting an equal
/// CSS viewport clip is redundant and can expose an otherwise hidden
/// underpaint through antialiasing at the duplicate boundary.
fn clip_is_page_media_box(
    clip: crate::document::PaintClip,
    state: &PaintTreeRenderState<'_, '_>,
) -> bool {
    clip_is_page_bounds(clip, state.page_width, state.page_height)
}

fn clip_is_page_bounds(
    clip: crate::document::PaintClip,
    page_width: f32,
    page_height: f32,
) -> bool {
    nearly_equal(clip.x(), 0.0)
        && nearly_equal(clip.y(), 0.0)
        && nearly_equal(clip.width(), page_width)
        && nearly_equal(clip.height(), page_height)
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

fn is_mergeable_fill_rect(rect: &crate::RenderedRect) -> bool {
    rect.stroke.is_none() && rect.fill.is_some_and(|fill| fill.is_visible())
}

#[derive(Default)]
struct PendingFillRects {
    rects: Vec<crate::RenderedRect>,
}

impl PendingFillRects {
    fn try_push(&mut self, rect: &crate::RenderedRect) -> bool {
        if !is_mergeable_fill_rect(rect) {
            return false;
        }
        let Some(first) = self.rects.first() else {
            self.rects.push(rect.clone());
            return true;
        };
        if first.fill != rect.fill {
            return false;
        }
        for pending in &mut self.rects {
            if merge_adjacent_fill_rect(pending, rect) {
                return true;
            }
        }
        if !rect.fill.is_some_and(|fill| fill.a >= 1.0)
            && self
                .rects
                .iter()
                .any(|pending| rects_intersect(pending, rect))
        {
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
) {
    let Some(fill) = pending.rects.first().and_then(|rect| rect.fill) else {
        return;
    };
    let scoped_alpha = write_alpha_graphics_state(content, fill);
    set_fill_color(content, fill, color_mode);
    for rect in pending.rects.drain(..) {
        let rect = crate::document::paint_rect_to_pdf(rect.paint_rect());
        content.rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        );
    }
    content.fill_nonzero();
    close_alpha_graphics_state(content, scoped_alpha);
}

fn merge_adjacent_fill_rect(left: &mut crate::RenderedRect, right: &crate::RenderedRect) -> bool {
    if !is_mergeable_fill_rect(left) || !is_mergeable_fill_rect(right) || left.fill != right.fill {
        return false;
    }
    let (x, y, width, height) = if nearly_equal(left.x(), right.x())
        && nearly_equal(left.width(), right.width())
        && nearly_equal(left.y() + left.height(), right.y())
    {
        (
            left.x(),
            left.y(),
            left.width(),
            left.height() + right.height(),
        )
    } else if nearly_equal(left.y(), right.y())
        && nearly_equal(left.height(), right.height())
        && nearly_equal(left.x() + left.width(), right.x())
    {
        (
            left.x(),
            left.y(),
            left.width() + right.width(),
            left.height(),
        )
    } else {
        return false;
    };
    left.set_paint_rect(crate::document::PaintRect::new(
        crate::document::PaintPoint::new(x, y),
        crate::document::PaintSize::new(width, height),
    ));
    true
}

fn rects_intersect(left: &crate::RenderedRect, right: &crate::RenderedRect) -> bool {
    left.x() < right.x() + right.width()
        && right.x() < left.x() + left.width()
        && left.y() < right.y() + right.height()
        && right.y() < left.y() + left.height()
}

fn rect_area_is_covered_by_rects(
    rect: &crate::RenderedRect,
    covers: &[&crate::RenderedRect],
) -> bool {
    if covers.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let right = rect.x() + rect.width();
    let top = rect.y() + rect.height();
    let mut x_edges = vec![rect.x(), right];
    let mut y_edges = vec![rect.y(), top];
    for cover in covers {
        x_edges.push(cover.x().clamp(rect.x(), right));
        x_edges.push((cover.x() + cover.width()).clamp(rect.x(), right));
        y_edges.push(cover.y().clamp(rect.y(), top));
        y_edges.push((cover.y() + cover.height()).clamp(rect.y(), top));
    }
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
    rect: &crate::RenderedRect,
    color_mode: PdfColorMode,
) {
    let pdf_rect = crate::document::paint_rect_to_pdf(rect.paint_rect());
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        set_fill_color(content, fill, color_mode);
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
        content.set_line_width(rect.stroke_width);
        set_stroke_color(content, stroke, color_mode);
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
) {
    if let Some(fill) = rect.fill
        && fill.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, fill);
        set_fill_color(content, fill, color_mode);
        write_rounded_rect_path(content, rect);
        content.fill_nonzero();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    if let Some(stroke) = rect.stroke
        && stroke.is_visible()
    {
        let scoped_alpha = write_alpha_graphics_state(content, stroke);
        content.set_line_width(rect.stroke_width);
        set_stroke_color(content, stroke, color_mode);
        write_rounded_rect_path(content, rect);
        content.stroke();
        close_alpha_graphics_state(content, scoped_alpha);
    }
}

pub(super) fn write_rounded_rect_path(content: &mut Content, rect: &RenderedRoundedRect) {
    // PDF paths use cubic Beziers for arcs. The kappa constant approximates a
    // quarter ellipse, matching the CSS border-radius curve shape closely
    // enough for filled/stroked page graphics.
    const KAPPA: f32 = 0.552_284_8;

    let pdf_rect = crate::document::paint_rect_to_pdf(rect.paint_rect());
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
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
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
    let transformed = path.transform != crate::document::PaintTransform::identity();
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
                next_object_id,
                page_width,
                page_height,
                color_mode,
            );
            write_path_stroke(
                content,
                path,
                vector_paints,
                next_object_id,
                page_width,
                page_height,
                color_mode,
            );
        }
        RenderedPathPaintOrder::StrokeThenFill => {
            write_path_stroke(
                content,
                path,
                vector_paints,
                next_object_id,
                page_width,
                page_height,
                color_mode,
            );
            write_path_fill(
                content,
                path,
                vector_paints,
                next_object_id,
                page_width,
                page_height,
                color_mode,
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
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
) {
    let Some(fill) = path.fill_paint.as_ref() else {
        return;
    };
    let Some(scoped_alpha) = write_path_fill_paint(
        content,
        fill,
        vector_paints,
        next_object_id,
        page_width,
        page_height,
        color_mode,
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
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
) {
    let Some(stroke) = path.stroke_paint.as_ref() else {
        return;
    };
    let Some(scoped_alpha) = write_path_stroke_paint(
        content,
        stroke,
        vector_paints,
        next_object_id,
        page_width,
        page_height,
        color_mode,
    ) else {
        return;
    };
    let style = &path.stroke_style;
    content
        .set_line_width(path.stroke_width)
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
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
) -> Option<bool> {
    match paint {
        RenderedPathPaint::Solid(color) if color.is_visible() => {
            let alpha = write_alpha_graphics_state(content, *color);
            set_fill_color(content, *color, color_mode);
            Some(alpha)
        }
        RenderedPathPaint::Solid(_) => None,
        RenderedPathPaint::Gradient(gradient) => {
            let resource =
                vector_paints.gradient_resource(gradient, next_object_id, page_width, page_height);
            if let Some(name) = &resource.alpha_gstate_name {
                content.save_state().set_parameters(pdf_name(name));
            }
            content
                .set_fill_color_space(ColorSpaceOperand::Pattern)
                .set_fill_pattern([], pdf_name(&resource.pattern_name));
            Some(resource.alpha_gstate_name.is_some())
        }
        RenderedPathPaint::SvgPattern(pattern) => {
            let alpha =
                write_alpha_graphics_state(content, crate::Color::rgba(0, 0, 0, pattern.opacity));
            let name = vector_paints.svg_path_tiling_resource(pattern, next_object_id);
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
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
    color_mode: PdfColorMode,
) -> Option<bool> {
    match paint {
        RenderedPathPaint::Solid(color) if color.is_visible() => {
            let alpha = write_alpha_graphics_state(content, *color);
            set_stroke_color(content, *color, color_mode);
            Some(alpha)
        }
        RenderedPathPaint::Solid(_) => None,
        RenderedPathPaint::Gradient(gradient) => {
            let resource =
                vector_paints.gradient_resource(gradient, next_object_id, page_width, page_height);
            if let Some(name) = &resource.alpha_gstate_name {
                content.save_state().set_parameters(pdf_name(name));
            }
            content
                .set_stroke_color_space(ColorSpaceOperand::Pattern)
                .set_stroke_pattern([], pdf_name(&resource.pattern_name));
            Some(resource.alpha_gstate_name.is_some())
        }
        RenderedPathPaint::SvgPattern(pattern) => {
            let alpha =
                write_alpha_graphics_state(content, crate::Color::rgba(0, 0, 0, pattern.opacity));
            let name = vector_paints.svg_path_tiling_resource(pattern, next_object_id);
            content
                .set_stroke_color_space(ColorSpaceOperand::Pattern)
                .set_stroke_pattern([], pdf_name(&name));
            Some(alpha)
        }
    }
}

fn write_rendered_path_clip(content: &mut Content, clip: &crate::document::RenderedPathClip) {
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
                let point = crate::document::paint_point_to_pdf(point);
                content.move_to(point.x, point.y);
            }
            RenderedPathCommandPoints::LineTo(point) => {
                let point = crate::document::paint_point_to_pdf(point);
                content.line_to(point.x, point.y);
            }
            RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                let control_1 = crate::document::paint_point_to_pdf(control_1);
                let control_2 = crate::document::paint_point_to_pdf(control_2);
                let end = crate::document::paint_point_to_pdf(end);
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
    stroke: &crate::RenderedStroke,
    color_mode: PdfColorMode,
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
    let start = crate::document::paint_point_to_pdf(start);
    let end = crate::document::paint_point_to_pdf(end);
    content.set_line_width(stroke.width);
    set_stroke_color(content, stroke.color, color_mode);
    content
        .move_to(start.x, start.y)
        .line_to(end.x, end.y)
        .stroke()
        .restore_state();
}

pub(super) fn write_image(content: &mut Content, image: &crate::RenderedImage, index: usize) {
    let rect = crate::document::paint_rect_to_pdf(image.paint_rect());
    if let Some(actual_text) = &image.actual_text {
        let mut marked_content = content.begin_marked_content_with_properties(Name(b"Span"));
        marked_content
            .properties()
            .actual_text(TextStr(actual_text.as_ref()));
    }
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
    if image.actual_text.is_some() {
        content.end_marked_content();
    }
}

pub(super) fn write_image_pattern(
    content: &mut Content,
    pattern: &crate::document::RenderedImagePattern,
    index: usize,
) {
    let rect = crate::document::paint_rect_to_pdf(pattern.paint_rect());
    content.save_state();
    if let Some(clip) = pattern.clip() {
        write_rendered_path_clip(content, clip);
    }
    content
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .clip_nonzero()
        .end_path()
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(&format!("P{}", index + 1)))
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
    pattern: &crate::document::RenderedGradientPattern,
    vector_paints: &mut VectorPaintResources,
    next_object_id: &mut usize,
    page_width: f32,
    page_height: f32,
) {
    let name =
        vector_paints.tiling_gradient_resource(pattern, next_object_id, page_width, page_height);
    let rect = crate::document::paint_rect_to_pdf(pattern.paint_rect());
    content.save_state();
    if let Some(clip) = pattern.clip() {
        write_rendered_path_clip(content, clip);
    }
    content
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .clip_nonzero()
        .end_path()
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(&name))
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .fill_nonzero();
    content.restore_state();
}

fn write_svg_tiling_pattern(
    content: &mut Content,
    pattern: &crate::document::RenderedSvgPattern,
    state: &mut PaintTreeRenderState<'_, '_>,
) {
    let form_id = *state.next_object_id;
    *state.next_object_id += 1;
    let form_name = format!("SvgTile{}", state.forms.len() + 1);
    let mut form_content = Content::new();
    for path in &pattern.paths {
        write_path(
            &mut form_content,
            path,
            state.vector_paints,
            state.next_object_id,
            state.page_width,
            state.page_height,
            state.color_mode,
        );
    }
    state.forms.push(FormXObjectRender {
        id: form_id,
        name: form_name.clone(),
        form_dependencies: Vec::new(),
        bbox: crate::document::PaintClip::new(0.0, 0.0, pattern.tile_width, pattern.tile_height),
        stream: form_content.finish().into_vec(),
        transparency_group: false,
    });
    let id = *state.next_object_id;
    *state.next_object_id += 1;
    let name = format!("SP{}", state.vector_paints.svg_tilings.len() + 1);
    state.vector_paints.svg_tilings.push(SvgTilingPatternPlan {
        id,
        name: name.clone(),
        form_id,
        form_name,
        pattern: pattern.clone(),
    });
    let rect = crate::document::paint_rect_to_pdf(pattern.paint_rect());
    content.save_state();
    if let Some(clip) = pattern.clip() {
        write_rendered_path_clip(content, clip);
    }
    content
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .clip_nonzero()
        .end_path()
        .set_fill_color_space(ColorSpaceOperand::Pattern)
        .set_fill_pattern([], pdf_name(&name))
        .rect(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
        .fill_nonzero();
    content.restore_state();
}

/// Serialize the supported solid-vector subset of an SVG paint-server cell.
/// The enclosing PDF tiling pattern supplies the cell's bounding box and
/// matrix; these paths deliberately remain in SVG user coordinates.
pub(super) fn svg_path_pattern_tile_content(
    pattern: &crate::document::RenderedSvgPathPattern,
    color_mode: PdfColorMode,
) -> Vec<u8> {
    let mut content = Content::new();
    let mut resources = VectorPaintResources::default();
    let mut next_object_id = 0;
    for path in &pattern.paths {
        write_path(
            &mut content,
            path,
            &mut resources,
            &mut next_object_id,
            0.0,
            0.0,
            color_mode,
        );
    }
    debug_assert!(resources.plans.is_empty());
    debug_assert!(resources.tilings.is_empty());
    debug_assert!(resources.svg_tilings.is_empty());
    debug_assert!(resources.svg_path_tilings.is_empty());
    content.finish().into_vec()
}

pub(super) fn write_line(
    content: &mut Content,
    line: &crate::RenderedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    color_mode: PdfColorMode,
) {
    let wrote_line = write_rendered_line(content, line, embedded_fonts, color_mode);
    if !wrote_line && !line.text.is_empty() && line.runs.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
    }
}

pub(super) fn write_rendered_line(
    content: &mut Content,
    line: &crate::RenderedLine,
    embedded_fonts: &EmbeddedFontPlans<'_>,
    color_mode: PdfColorMode,
) -> bool {
    if !line.color.is_visible() {
        return true;
    }

    // PDF 2.0 9.4.4 text matrices position each glyph stream in user space.
    // CSS inline layout stores shaped runs at visual offsets inside one line
    // box.  Identity-matrix runs can retain one text line matrix and move to
    // each absolute run origin with `Td`; transformed runs retain the
    // conservative absolute `Tm` emission required by writing-mode transforms.
    let line_origin = crate::document::paint_point_to_pdf(line.origin());
    let mut text_started = false;
    let mut scoped_alpha = false;
    let mut saw_text_run = false;
    let mut identity_text_line_origin = None::<(f32, f32)>;
    let mut active_font = None::<(usize, f32)>;
    for run in pdf_text_runs(line, embedded_fonts.document_font_to_embedded_font.len()) {
        saw_text_run = true;
        if run.glyphs.is_empty() {
            log::debug!("empty shaped text line {:?}", line.text);
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
            continue;
        };
        let Some(font) = embedded_fonts.fonts.get(embedded_font_index) else {
            log::warn!(
                "skipping shaped text run with missing embedded font resource {}",
                embedded_font_index
            );
            continue;
        };
        if run
            .glyphs
            .iter()
            .any(|glyph| !font.source_gid_to_cid.contains_key(&glyph.id))
        {
            log::warn!(
                "skipping shaped text run whose glyphs are missing from PDF font CID mapping"
            );
            continue;
        }
        if !text_started {
            scoped_alpha = write_alpha_graphics_state(content, line.color);
            set_fill_color(content, line.color, color_mode);
            content.begin_text();
            text_started = true;
        }
        let pdf_font_size = quantized_pdf_font_size(run.font_size);
        let run_origin = (line_origin.x + run.x_offset, line_origin.y + run.y_offset);
        if run.text_matrix.is_identity() {
            if let Some((previous_x, previous_y)) = identity_text_line_origin {
                content.next_line(run_origin.0 - previous_x, run_origin.1 - previous_y);
            } else {
                content.set_text_matrix([1.0, 0.0, 0.0, 1.0, run_origin.0, run_origin.1]);
            }
            identity_text_line_origin = Some(run_origin);
        } else {
            let [a, b, c, d] = run.text_matrix.pdf_components();
            content.set_text_matrix([a, b, c, d, run_origin.0, run_origin.1]);
            identity_text_line_origin = None;
        }
        if active_font != Some((embedded_font_index, pdf_font_size)) {
            content.set_font(pdf_name(&font.resource_name), pdf_font_size);
            active_font = Some((embedded_font_index, pdf_font_size));
        }
        if let Some(actual_text) = run.actual_text {
            let mut marked_content = content.begin_marked_content_with_properties(Name(b"Span"));
            marked_content
                .properties()
                .actual_text(TextStr(actual_text));
        }
        write_glyphs(content, run.font_size, run.glyphs, &font.source_gid_to_cid);
        if run.actual_text.is_some() {
            content.end_marked_content();
        }
    }
    if text_started {
        content.end_text();
        close_alpha_graphics_state(content, scoped_alpha);
    }
    saw_text_run
}

/// Activate a PDF ExtGState for semi-transparent paint.
///
/// PDF 1.4 uses the `gs` operator to load graphics-state parameters,
/// including stroking and nonstroking alpha constants:
/// ISO 32000-1:2008, 8.4.4 "Graphics State Operators" and 11.7.4.3
/// "Constant Shape and Opacity".
fn write_alpha_graphics_state(content: &mut Content, color: Color) -> bool {
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

fn write_glyphs(
    content: &mut Content,
    font_size: f32,
    glyphs: &[RenderedGlyph],
    source_gid_to_cid: &std::collections::BTreeMap<u16, u16>,
) {
    if !needs_positioned_glyphs(glyphs) {
        let glyph_bytes = glyph_bytes(glyphs, source_gid_to_cid);
        content.show(Str(&glyph_bytes));
        return;
    }

    let mut positioned = content.show_positioned();
    let mut items = positioned.items();
    for (index, glyph) in glyphs.iter().enumerate() {
        let glyph_bytes = glyph_id_bytes(source_gid_to_cid[&glyph.id]);
        items.show(Str(&glyph_bytes));
        if index + 1 < glyphs.len() {
            let adjustment =
                ((glyph.nominal_x_advance - glyph.x_advance) * 1000.0) / font_size.max(0.001);
            if adjustment.abs() > 0.01 {
                items.adjust(adjustment);
            }
        }
    }
}

pub(super) fn needs_positioned_glyphs(glyphs: &[RenderedGlyph]) -> bool {
    glyphs.iter().any(|glyph| {
        (glyph.x_advance - glyph.nominal_x_advance).abs() > 0.01
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
        .flat_map(|glyph| glyph_id_bytes(source_gid_to_cid[&glyph.id]))
        .collect()
}

fn glyph_id_bytes(glyph_id: u16) -> [u8; 2] {
    glyph_id.to_be_bytes()
}

fn pdf_name(name: &str) -> Name<'_> {
    Name(name.as_bytes())
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::{PaintPoint, PaintRect, PaintSize, RenderedImage};
    use std::rc::Rc;

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
        write_image(&mut content, &image, 0);
        let bytes = content.finish().into_vec();
        let content = String::from_utf8_lossy(&bytes);
        assert!(content.contains("ActualText"));
        assert!(content.contains("/Im1 Do"));
        assert!(content.contains("EMC"));
    }
}
