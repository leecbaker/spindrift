use crate::document::Page;

use super::annotations::RenderedLink;
use super::effects::{
    PaintBlendMode, PaintEffectScope, PaintEffects, PaintFragmentation, RenderedClipPathPolygon,
    ThreeDParticipation,
};
use super::fragments::PaintFragment;
use super::geometry::{
    Affine3dPaintTransform, PaintClip, PaintPoint, PaintTransform, PaintTranslation,
    ProjectedPlane, Projective3dPaintTransform,
};
use super::page::{PaintOperation, PaintPrimitive};
use super::paths::{RenderedPath, RenderedPathCommand, RenderedPathFillRule};
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

    pub(crate) fn transformed_links(&self, page: &Page) -> Vec<RenderedLink> {
        let mut resolved = self.clone();
        resolved.resolve_affine_3d_contexts(page);
        let mut links = Vec::new();
        resolved
            .root
            .push_transformed_links(PaintTransform::identity(), None, &mut links);
        links
    }

    /// Resolve CSS 3D rendering contexts to ordinary retained paint effects
    /// immediately before a backend consumes the tree.
    ///
    /// Keeping this as a tree rewrite preserves CSS Appendix-E order while
    /// avoiding a premature PDF CTM at an intermediate `preserve-3d` element.
    /// <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
    pub(crate) fn resolve_affine_3d_contexts(&mut self, page: &Page) {
        resolve_context_projective_3d(&mut self.root, None, page);
        // Projective lowering must happen first. An affine 3D transform can
        // still carry Z depth (for example `translateZ()`), and flattening it
        // to a PDF CTM before an ancestor's `perspective` is composed loses
        // that depth permanently.
        // <https://drafts.csswg.org/css-transforms-2/#perspective-property>
        resolve_context_affine_3d(&mut self.root, None, page);
    }
}

/// Lower projective planes after affine 3D scene ordering has completed.
///
/// PDF has no projective graphics-state CTM.  Rectangular paint is therefore
/// converted into a visible projected polygon before the content backend
/// consumes it.  Unsupported primitive kinds remain retained rather than
/// being discarded; their dedicated projective lowerers can be added without
/// changing the scene-matrix representation.
fn resolve_context_projective_3d(
    context: &mut PaintStackingContext,
    inherited: Option<Projective3dPaintTransform>,
    page: &Page,
) {
    let (current, descendants) = resolve_projective_effects(&mut context.effects, inherited);
    if current.is_some_and(|transform| {
        !transform.is_invertible()
            || (context.effects.hide_backface && transform.faces_away_from_viewer())
    }) {
        context.effects.suppress_paint = true;
        return;
    }
    for band in PaintBand::ORDER {
        resolve_projective_items(
            &mut context.bands.bands[band.index()],
            current,
            descendants,
            page,
        );
    }
}

fn resolve_scope_projective_3d(
    scope: &mut PaintEffectScope,
    inherited: Option<Projective3dPaintTransform>,
    page: &Page,
) {
    let (current, descendants) = resolve_projective_effects(&mut scope.effects, inherited);
    if current.is_some_and(|transform| {
        !transform.is_invertible()
            || (scope.effects.hide_backface && transform.faces_away_from_viewer())
    }) {
        scope.effects.suppress_paint = true;
        return;
    }
    resolve_projective_items(&mut scope.items, current, descendants, page);
}

/// Consume retained local transforms once an ancestor requires projective
/// lowering.  A PDF CTM would otherwise run after the perspective divide.
fn resolve_projective_effects(
    effects: &mut PaintEffects,
    inherited: Option<Projective3dPaintTransform>,
) -> (
    Option<Projective3dPaintTransform>,
    Option<Projective3dPaintTransform>,
) {
    let needs_projective_lowering = inherited.is_some()
        || effects.projective_3d_transform.is_some()
        || effects.descendant_projective_3d_transform.is_some();
    if !needs_projective_lowering {
        return (None, None);
    }

    let mut local = effects.projective_3d_transform.take();
    if let Some(affine_3d) = effects.affine_3d_transform.take() {
        let affine_3d = affine_3d.into_projective();
        local = Some(local.map_or(affine_3d, |local| local.multiply(affine_3d)));
    }
    if let Some(affine) = effects.transform.take() {
        let affine = Projective3dPaintTransform::from_paint_transform(affine);
        local = Some(local.map_or(affine, |local| local.multiply(affine)));
    }
    let current = match (inherited, local) {
        (Some(parent), Some(local)) => Some(parent.multiply(local)),
        (Some(parent), None) => Some(parent),
        (None, Some(local)) => Some(local),
        (None, None) => None,
    };
    let descendants = match (current, effects.descendant_projective_3d_transform.take()) {
        (Some(current), Some(perspective)) => Some(current.multiply(perspective)),
        (None, Some(perspective)) => Some(perspective),
        (current, None) => current,
    };
    (current, descendants)
}

fn resolve_projective_items(
    items: &mut Vec<PaintDisplayItem>,
    current: Option<Projective3dPaintTransform>,
    descendants: Option<Projective3dPaintTransform>,
    page: &Page,
) {
    let mut lowered = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        match item {
            PaintDisplayItem::Operation(operation) => {
                if let Some(transform) = current {
                    if let Some(primitive) = page.paint_primitive(&operation)
                        && let Some(primitive) =
                            lower_projective_primitive(primitive, Some(transform))
                    {
                        lowered.push(PaintDisplayItem::Primitive(primitive));
                    }
                } else {
                    // Keep ordinary operations indexed into the page's
                    // primitive stores. PDF serialization resolves those
                    // indices to page resources; converting them to retained
                    // primitives is only necessary after perspective
                    // lowering.
                    lowered.push(PaintDisplayItem::Operation(operation));
                }
            }
            PaintDisplayItem::Primitive(primitive) => {
                if let Some(transform) = current {
                    if let Some(primitive) = lower_projective_primitive(primitive, Some(transform))
                    {
                        lowered.push(PaintDisplayItem::Primitive(primitive));
                    }
                } else {
                    lowered.push(PaintDisplayItem::Primitive(primitive));
                }
            }
            PaintDisplayItem::StackingContext(mut context) => {
                resolve_context_projective_3d(&mut context, descendants, page);
                lowered.push(PaintDisplayItem::StackingContext(context));
            }
            PaintDisplayItem::EffectScope(mut scope) => {
                resolve_scope_projective_3d(&mut scope, descendants, page);
                lowered.push(PaintDisplayItem::EffectScope(scope));
            }
            PaintDisplayItem::Link(link) => {
                if let Some(link) = lower_projective_link(link, current) {
                    lowered.push(PaintDisplayItem::Link(link));
                }
            }
        }
    }
    *items = lowered;
}

fn lower_projective_primitive(
    primitive: PaintPrimitive,
    transform: Option<Projective3dPaintTransform>,
) -> Option<PaintPrimitive> {
    let Some(transform) = transform else {
        return Some(primitive);
    };
    match primitive {
        PaintPrimitive::Rect(rect) => {
            let fill = rect.fill;
            let stroke = rect.stroke;
            let stroke_width = rect.stroke_width;
            let rect = rect.paint_rect();
            let points = project_plane_polygon(
                transform,
                &[
                    rect.origin,
                    PaintPoint::new(rect.max_x(), rect.min_y()),
                    PaintPoint::new(rect.max_x(), rect.max_y()),
                    PaintPoint::new(rect.min_x(), rect.max_y()),
                ],
            )?;
            (points.len() >= 3).then(|| {
                let mut commands = Vec::with_capacity(points.len() + 1);
                commands.push(RenderedPathCommand::move_to(points[0]));
                commands.extend(
                    points
                        .iter()
                        .skip(1)
                        .copied()
                        .map(RenderedPathCommand::line_to),
                );
                commands.push(RenderedPathCommand::Close);
                PaintPrimitive::Path(RenderedPath::new(
                    commands,
                    fill,
                    RenderedPathFillRule::NonZero,
                    stroke,
                    stroke_width,
                    None,
                ))
            })
        }
        primitive => Some(primitive),
    }
}

/// PDF link annotations are rectangular, so a projective link is represented
/// by the conservative bounds of its visible projected quadrilateral.  This
/// keeps interactive content aligned with the lowered plane without claiming
/// that PDF supports a non-rectangular projective annotation.
fn lower_projective_link(
    link: RenderedLink,
    transform: Option<Projective3dPaintTransform>,
) -> Option<RenderedLink> {
    let Some(transform) = transform else {
        return Some(link);
    };
    let rect = link.paint_rect();
    let points = project_plane_polygon(
        transform,
        &[
            rect.origin,
            PaintPoint::new(rect.max_x(), rect.min_y()),
            PaintPoint::new(rect.max_x(), rect.max_y()),
            PaintPoint::new(rect.min_x(), rect.max_y()),
        ],
    )?;
    let min_x = points.iter().map(|point| point.x).reduce(f32::min)?;
    let max_x = points.iter().map(|point| point.x).reduce(f32::max)?;
    let min_y = points.iter().map(|point| point.y).reduce(f32::min)?;
    let max_y = points.iter().map(|point| point.y).reduce(f32::max)?;
    Some(RenderedLink::from_paint_rect(
        super::geometry::PaintRect::new(
            PaintPoint::new(min_x, min_y),
            super::geometry::PaintSize::new(max_x - min_x, max_y - min_y),
        ),
        link.target,
    ))
}

fn project_plane_polygon(
    transform: Projective3dPaintTransform,
    points: &[PaintPoint],
) -> Option<Vec<PaintPoint>> {
    match transform.project_plane(points) {
        ProjectedPlane::Visible(plane) | ProjectedPlane::ClippedAtViewer(plane) => {
            Some(plane.polygon)
        }
        ProjectedPlane::BehindViewer => None,
    }
}

fn resolve_context_affine_3d(
    context: &mut PaintStackingContext,
    inherited: Option<Affine3dPaintTransform>,
    page: &Page,
) {
    let local = context.effects.affine_3d_transform;
    let had_local_3d_transform = local.is_some();
    let accumulated = match (inherited, local) {
        (Some(parent), Some(local)) => Some(parent.multiply(local)),
        (Some(parent), None) => Some(parent),
        (None, Some(local)) => Some(local),
        (None, None) => None,
    };
    match context.effects.three_d_participation {
        ThreeDParticipation::Preserve3d => {
            context.effects.affine_3d_transform = None;
            context.effects.transform = None;
            context.effects.suppress_paint =
                accumulated.is_some_and(|transform| !transform.is_invertible());
            resolve_band_list_affine_3d(
                &mut context.bands,
                accumulated,
                context.effects.hide_backface,
                context.bounds,
                page,
            );
        }
        ThreeDParticipation::TransparentLayoutBridge => {
            context.effects.affine_3d_transform = None;
            resolve_band_list_affine_3d(&mut context.bands, inherited, false, context.bounds, page);
        }
        ThreeDParticipation::Flat if inherited.is_some() => {
            context.effects.affine_3d_transform = None;
            let transform =
                accumulated.expect("an inherited 3D context supplies an accumulated matrix");
            let plane_transform = transform.flatten_to_paint_transform();
            // A local 3D transform has just been incorporated into
            // `transform`; a pre-existing 2D transform instead belongs to a
            // flat descendant of the preserved context and must remain part
            // of its final PDF CTM.
            context.effects.transform = if had_local_3d_transform {
                Some(plane_transform)
            } else {
                Some(
                    context
                        .effects
                        .transform
                        .map_or(plane_transform, |local| plane_transform.multiply(local)),
                )
            };
            context.effects.suppress_paint = !transform.is_invertible()
                || (context.effects.hide_backface && transform.faces_away_from_viewer());
            resolve_band_list_affine_3d(&mut context.bands, None, false, context.bounds, page);
        }
        ThreeDParticipation::Flat => {
            context.effects.affine_3d_transform = None;
            if let Some(transform) = local {
                context.effects.transform = Some(transform.flatten_to_paint_transform());
                context.effects.suppress_paint = !transform.is_invertible()
                    || (context.effects.hide_backface && transform.faces_away_from_viewer());
            }
            resolve_band_list_affine_3d(&mut context.bands, None, false, context.bounds, page);
        }
    }
}

fn resolve_band_list_affine_3d(
    bands: &mut PaintBandList,
    inherited: Option<Affine3dPaintTransform>,
    hide_backface: bool,
    plane_bounds: Option<PaintClip>,
    page: &Page,
) {
    if let Some(transform) = inherited {
        // A 3D rendering context is a single shared scene, not twelve
        // independently sorted Appendix-E bands. CSS Transforms gives the
        // context-establishing element one plane containing all ordinary
        // Appendix-E paint, while each affine 3D participant has a separate
        // plane. Keep those categories distinct before Newell ordering.
        // <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
        let source_items = PaintBand::ORDER
            .into_iter()
            .flat_map(|band| std::mem::take(&mut bands.bands[band.index()]))
            .collect::<Vec<_>>();
        let source_items = expand_transparent_layout_bridges(source_items);
        // Preserve the context root's Appendix-E position around retained
        // descendant planes. A single root plane placed at the start would
        // move ordinary in-flow paint ahead of a negative-z descendant (or
        // behind an earlier coplanar participant). Splitting its retained
        // paint only at participant boundaries is equivalent to one identity
        // plane for geometry, while retaining the required paint-order ties.
        // <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
        let mut scene_items = Vec::new();
        let mut root_segment = Vec::new();
        for item in source_items {
            if is_affine_3d_scene_participant(&item) {
                append_affine_3d_root_scene_segment(
                    &mut scene_items,
                    &mut root_segment,
                    plane_bounds,
                    page,
                );
                scene_items.push(item);
            } else {
                root_segment.push(item);
            }
        }
        append_affine_3d_root_scene_segment(
            &mut scene_items,
            &mut root_segment,
            plane_bounds,
            page,
        );
        split_affine_3d_scene_intersections(&mut scene_items, transform, page);
        sort_affine_3d_scene_items(&mut scene_items, transform, plane_bounds, page);
        resolve_items_affine_3d(&mut scene_items, Some(transform), hide_backface, page);
        bands.bands[PaintBand::InFlowBlock.index()] = scene_items;
        return;
    }
    for band in PaintBand::ORDER {
        resolve_items_affine_3d(&mut bands.bands[band.index()], None, hide_backface, page);
    }
}

/// Append one contiguous segment of ordinary context-root paint as an identity
/// scene plane. The segment boundary is semantic paint order, not a clip: its
/// retained bounds stay exact so overflow paint still participates in depth
/// ordering.
fn append_affine_3d_root_scene_segment(
    scene_items: &mut Vec<PaintDisplayItem>,
    root_segment: &mut Vec<PaintDisplayItem>,
    fallback_bounds: Option<PaintClip>,
    page: &Page,
) {
    if root_segment.is_empty() {
        return;
    }
    let root_bounds = retained_scene_items_bounds(root_segment, page)
        .or(fallback_bounds)
        .unwrap_or_else(|| PaintClip::new(0.0, 0.0, page.width(), page.height()));
    let root_plane = PaintEffectScope::new(
        PaintEffects {
            affine_3d_transform: Some(Affine3dPaintTransform::identity()),
            ..PaintEffects::default()
        },
        Some(root_bounds),
        std::mem::take(root_segment),
    );
    scene_items.push(PaintDisplayItem::EffectScope(root_plane));
}

fn is_affine_3d_scene_participant(item: &PaintDisplayItem) -> bool {
    match item {
        PaintDisplayItem::StackingContext(context) => {
            context.effects.affine_3d_transform.is_some()
                // A retained used-flat descendant is a flattening boundary,
                // hence an independently paintable plane in its parent 3D
                // rendering context even when it has no transform of its
                // own.  Putting it in the context-root plane loses its CSS
                // source-order tie against a preserved sibling.
                // <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
                || context.effects.three_d_participation == ThreeDParticipation::Flat
        }
        PaintDisplayItem::EffectScope(scope) => {
            scope.effects.affine_3d_transform.is_some()
                || scope.effects.three_d_participation == ThreeDParticipation::Flat
        }
        PaintDisplayItem::Operation(_)
        | PaintDisplayItem::Primitive(_)
        | PaintDisplayItem::Link(_) => false,
    }
}

/// Elide anonymous layout-only wrappers from a shared CSS 3D scene.
///
/// A transparent bridge carries already-resolved layout coordinates but is
/// neither a CSS box that establishes a plane nor a used `flat` boundary. Its
/// descendants must therefore join the enclosing rendering context directly,
/// where Newell ordering can compare them with sibling planes. This lowering
/// happens only after layout and fragmentation have completed, so removing the
/// retained wrapper cannot affect either layout offsets or fragmentation.
/// <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
fn expand_transparent_layout_bridges(items: Vec<PaintDisplayItem>) -> Vec<PaintDisplayItem> {
    let mut expanded = Vec::with_capacity(items.len());
    for item in items {
        match item {
            PaintDisplayItem::StackingContext(mut context)
                if context.effects.three_d_participation
                    == ThreeDParticipation::TransparentLayoutBridge
                    && context.effects.is_effect_free_transparent_layout_bridge() =>
            {
                debug_assert!(context.effects.affine_3d_transform.is_none());
                debug_assert!(context.effects.transform.is_none());
                let children = PaintBand::ORDER
                    .into_iter()
                    .flat_map(|band| std::mem::take(&mut context.bands.bands[band.index()]))
                    .collect();
                expanded.extend(expand_transparent_layout_bridges(children));
            }
            PaintDisplayItem::EffectScope(scope)
                if scope.effects.three_d_participation
                    == ThreeDParticipation::TransparentLayoutBridge
                    && scope.effects.is_effect_free_transparent_layout_bridge() =>
            {
                debug_assert!(scope.effects.affine_3d_transform.is_none());
                debug_assert!(scope.effects.transform.is_none());
                expanded.extend(expand_transparent_layout_bridges(scope.items));
            }
            item => expanded.push(item),
        }
    }
    expanded
}

fn retained_scene_items_bounds(items: &[PaintDisplayItem], page: &Page) -> Option<PaintClip> {
    items.iter().fold(None, |bounds, item| {
        item.recorded_paint_bounds(page)
            .ok()
            .flatten()
            .map_or(bounds, |item_bounds| {
                Some(union_paint_clips(bounds, item_bounds))
            })
    })
}

fn resolve_scope_affine_3d(
    scope: &mut PaintEffectScope,
    inherited: Option<Affine3dPaintTransform>,
    _hide_backface: bool,
    page: &Page,
) {
    let local = scope.effects.affine_3d_transform;
    let had_local_3d_transform = local.is_some();
    let accumulated = match (inherited, local) {
        (Some(parent), Some(local)) => Some(parent.multiply(local)),
        (Some(parent), None) => Some(parent),
        (None, Some(local)) => Some(local),
        (None, None) => None,
    };
    match scope.effects.three_d_participation {
        ThreeDParticipation::Preserve3d => {
            scope.effects.affine_3d_transform = None;
            scope.effects.transform = None;
            scope.effects.suppress_paint =
                accumulated.is_some_and(|transform| !transform.is_invertible());
            resolve_items_affine_3d(
                &mut scope.items,
                accumulated,
                scope.effects.hide_backface,
                page,
            );
        }
        ThreeDParticipation::TransparentLayoutBridge => {
            scope.effects.affine_3d_transform = None;
            resolve_items_affine_3d(&mut scope.items, inherited, false, page);
        }
        ThreeDParticipation::Flat if inherited.is_some() => {
            scope.effects.affine_3d_transform = None;
            let transform =
                accumulated.expect("an inherited 3D context supplies an accumulated matrix");
            let plane_transform = transform.flatten_to_paint_transform();
            scope.effects.transform = if had_local_3d_transform {
                Some(plane_transform)
            } else {
                Some(
                    scope
                        .effects
                        .transform
                        .map_or(plane_transform, |local| plane_transform.multiply(local)),
                )
            };
            scope.effects.suppress_paint = !transform.is_invertible()
                || (scope.effects.hide_backface && transform.faces_away_from_viewer());
            resolve_items_affine_3d(&mut scope.items, None, false, page);
        }
        ThreeDParticipation::Flat => {
            scope.effects.affine_3d_transform = None;
            if let Some(transform) = local {
                scope.effects.transform = Some(transform.flatten_to_paint_transform());
                scope.effects.suppress_paint = !transform.is_invertible()
                    || (scope.effects.hide_backface && transform.faces_away_from_viewer());
            }
            resolve_items_affine_3d(&mut scope.items, None, false, page);
        }
    }
}

fn resolve_items_affine_3d(
    items: &mut Vec<PaintDisplayItem>,
    inherited: Option<Affine3dPaintTransform>,
    hide_backface: bool,
    page: &Page,
) {
    let Some(transform) = inherited else {
        for item in items {
            match item {
                PaintDisplayItem::StackingContext(context) => {
                    resolve_context_affine_3d(context, None, page)
                }
                PaintDisplayItem::EffectScope(scope) => {
                    resolve_scope_affine_3d(scope, None, false, page)
                }
                PaintDisplayItem::Operation(_)
                | PaintDisplayItem::Primitive(_)
                | PaintDisplayItem::Link(_) => {}
            }
        }
        return;
    };

    let plane_transform = transform.flatten_to_paint_transform();
    let suppress_paint =
        !transform.is_invertible() || (hide_backface && transform.faces_away_from_viewer());
    let mut scene_items = expand_transparent_layout_bridges(std::mem::take(items));
    split_affine_3d_scene_intersections(&mut scene_items, transform, page);
    sort_affine_3d_scene_items(&mut scene_items, transform, None, page);
    let mut resolved = Vec::with_capacity(scene_items.len());
    for item in scene_items {
        match item {
            PaintDisplayItem::StackingContext(mut context) => {
                resolve_context_affine_3d(&mut context, Some(transform), page);
                resolved.push(PaintDisplayItem::StackingContext(context));
            }
            PaintDisplayItem::EffectScope(mut scope) => {
                resolve_scope_affine_3d(&mut scope, Some(transform), hide_backface, page);
                resolved.push(PaintDisplayItem::EffectScope(scope));
            }
            PaintDisplayItem::Operation(_)
            | PaintDisplayItem::Primitive(_)
            | PaintDisplayItem::Link(_) => {
                let effects = PaintEffects {
                    transform: Some(plane_transform),
                    suppress_paint,
                    ..PaintEffects::default()
                };
                resolved.push(PaintDisplayItem::EffectScope(PaintEffectScope::new(
                    effects,
                    None,
                    vec![item],
                )));
            }
        }
    }
    *items = resolved;
}

/// Order independent affine descendant planes back-to-front before the PDF
/// backend flattens them. This is the non-intersecting fast path of the CSS
/// Transforms Newell scene ordering algorithm; coplanar planes retain CSS
/// painting order through the stable sort.
/// <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
fn sort_affine_3d_scene_items(
    items: &mut [PaintDisplayItem],
    inherited: Affine3dPaintTransform,
    fallback_bounds: Option<PaintClip>,
    page: &Page,
) {
    let planes = items
        .iter()
        .enumerate()
        .map(|(index, item)| affine_3d_scene_plane(item, inherited, index, page))
        .collect::<Vec<_>>();
    let mut edges = vec![Vec::new(); items.len()];
    let mut indegree = vec![0_usize; items.len()];
    for left in 0..items.len() {
        for right in left + 1..items.len() {
            let order = match (&planes[left], &planes[right]) {
                (Some(left_plane), Some(right_plane)) => {
                    classify_affine_3d_planes(left_plane, right_plane)
                }
                _ => {
                    let left_depth =
                        affine_3d_scene_item_depth(&items[left], inherited, fallback_bounds, page);
                    let right_depth =
                        affine_3d_scene_item_depth(&items[right], inherited, fallback_bounds, page);
                    match left_depth.partial_cmp(&right_depth) {
                        Some(std::cmp::Ordering::Less) => Affine3dPlaneOrder::Behind,
                        Some(std::cmp::Ordering::Greater) => Affine3dPlaneOrder::InFront,
                        _ => Affine3dPlaneOrder::Coplanar,
                    }
                }
            };
            let (from, to) = match order {
                Affine3dPlaneOrder::Behind => (left, right),
                Affine3dPlaneOrder::InFront => (right, left),
                Affine3dPlaneOrder::Coplanar | Affine3dPlaneOrder::Split => continue,
            };
            edges[from].push(to);
            indegree[to] += 1;
        }
    }
    let mut remaining = vec![true; items.len()];
    let mut order = Vec::with_capacity(items.len());
    while order.len() < items.len() {
        let next = (0..items.len())
            .find(|&index| remaining[index] && indegree[index] == 0)
            .or_else(|| (0..items.len()).find(|&index| remaining[index]));
        let Some(next) = next else { break };
        remaining[next] = false;
        order.push(next);
        for &successor in &edges[next] {
            indegree[successor] = indegree[successor].saturating_sub(1);
        }
    }
    let original = items.to_vec();
    for (destination, source) in items.iter_mut().zip(order) {
        *destination = planes[source]
            .as_ref()
            .and_then(|plane| plane.paint.items.first())
            .cloned()
            .unwrap_or_else(|| original[source].clone());
    }
}

fn affine_3d_scene_item_depth(
    item: &PaintDisplayItem,
    inherited: Affine3dPaintTransform,
    fallback_bounds: Option<PaintClip>,
    page: &Page,
) -> Option<f32> {
    let (local, bounds, scene_clip) = match item {
        PaintDisplayItem::StackingContext(context) => (
            context.effects.affine_3d_transform,
            context.bounds,
            context.effects.scene_plane_clip,
        ),
        PaintDisplayItem::EffectScope(scope) => (
            scope.effects.affine_3d_transform,
            scope.bounds,
            scope.effects.scene_plane_clip,
        ),
        PaintDisplayItem::Operation(_)
        | PaintDisplayItem::Primitive(_)
        | PaintDisplayItem::Link(_) => (None, fallback_bounds, None),
    };
    let transform = local.map_or(inherited, |local| inherited.multiply(local));
    let bounds = item
        .recorded_paint_bounds(page)
        .ok()
        .flatten()
        .or(bounds)
        .or(fallback_bounds);
    let center = scene_clip.map_or_else(
        || {
            PaintPoint::new(
                bounds.map_or(0.0, |bounds| bounds.x() + bounds.width() * 0.5),
                bounds.map_or(0.0, |bounds| bounds.y() + bounds.height() * 0.5),
            )
        },
        polygon_centroid,
    );
    Some(transform.depth_at(center))
}

fn polygon_centroid(polygon: RenderedClipPathPolygon) -> PaintPoint {
    let points = polygon.points();
    let (x, y) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), point| (x + point.x, y + point.y));
    PaintPoint::new(x / points.len() as f32, y / points.len() as f32)
}

/// A retained participant in one affine 3D rendering context.
///
/// The polygon remains in the participant's local paint coordinates.  It is
/// converted to a PDF clipping path only after its accumulated transform is
/// flattened, which prevents an internal scene split from replacing authored
/// overflow or `clip-path` clips.
/// <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
#[derive(Debug, Clone)]
struct Affine3dScenePlane {
    /// Retained local paint for this independently orderable scene plane.
    /// The scene compiler keeps this scope in page-local coordinates until
    /// Newell ordering and fragment clipping have completed.
    paint: PaintEffectScope,
    local_polygon: RenderedClipPathPolygon,
    accumulated_transform: Affine3dPaintTransform,
    paint_order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Affine3dPlaneOrder {
    Behind,
    InFront,
    Coplanar,
    Split,
}

fn split_affine_3d_scene_intersections(
    items: &mut Vec<PaintDisplayItem>,
    inherited: Affine3dPaintTransform,
    page: &Page,
) {
    // Fragmentation is iterative: splitting one pair can expose another
    // pair.  The fixed bound is the number of original participants squared;
    // each successful pass replaces exactly one pair with convex fragments.
    let original_len = items.len();
    for _ in 0..original_len.saturating_mul(original_len) {
        let Some((left, right, left_fragments, right_fragments)) =
            next_affine_3d_scene_split(items, inherited, page)
        else {
            break;
        };
        let mut replacement = Vec::with_capacity(items.len() + 2);
        for (index, item) in std::mem::take(items).into_iter().enumerate() {
            if index == left {
                replacement.extend(
                    left_fragments
                        .iter()
                        .filter_map(|polygon| item_with_scene_plane_clip(item.clone(), *polygon)),
                );
            } else if index == right {
                replacement.extend(
                    right_fragments
                        .iter()
                        .filter_map(|polygon| item_with_scene_plane_clip(item.clone(), *polygon)),
                );
            } else {
                replacement.push(item);
            }
        }
        *items = replacement;
    }
}

fn next_affine_3d_scene_split(
    items: &[PaintDisplayItem],
    inherited: Affine3dPaintTransform,
    page: &Page,
) -> Option<(
    usize,
    usize,
    [RenderedClipPathPolygon; 2],
    [RenderedClipPathPolygon; 2],
)> {
    for left_index in 0..items.len() {
        let left = affine_3d_scene_plane(&items[left_index], inherited, left_index, page)?;
        for (right_index, right_item) in items.iter().enumerate().skip(left_index + 1) {
            let right = affine_3d_scene_plane(right_item, inherited, right_index, page)?;
            if classify_affine_3d_planes(&left, &right) != Affine3dPlaneOrder::Split {
                continue;
            }
            let left_fragments = split_scene_plane_against(&left, &right)?;
            let right_fragments = split_scene_plane_against(&right, &left)?;
            return Some((left_index, right_index, left_fragments, right_fragments));
        }
    }
    None
}

fn affine_3d_scene_plane(
    item: &PaintDisplayItem,
    inherited: Affine3dPaintTransform,
    paint_order: usize,
    page: &Page,
) -> Option<Affine3dScenePlane> {
    let (local, bounds, clip) = match item {
        PaintDisplayItem::StackingContext(context) => (
            context.effects.affine_3d_transform?,
            context.bounds?,
            context.effects.scene_plane_clip,
        ),
        PaintDisplayItem::EffectScope(scope) => (
            scope.effects.affine_3d_transform?,
            scope.bounds?,
            scope.effects.scene_plane_clip,
        ),
        PaintDisplayItem::Operation(_)
        | PaintDisplayItem::Primitive(_)
        | PaintDisplayItem::Link(_) => {
            return None;
        }
    };
    let paint = PaintEffectScope::new(PaintEffects::default(), Some(bounds), vec![item.clone()]);
    let retained_bounds = PaintDisplayItem::EffectScope(scene_plane_local_measure(item, bounds))
        .recorded_paint_bounds(page)
        .ok()
        .flatten()
        .unwrap_or(bounds);
    let local_polygon = clip.unwrap_or_else(|| rectangle_polygon(retained_bounds));
    Some(Affine3dScenePlane {
        paint: PaintEffectScope {
            bounds: Some(retained_bounds),
            ..paint
        },
        local_polygon,
        accumulated_transform: inherited.multiply(local),
        paint_order,
    })
}

/// Clone one scene participant for local retained-bounds measurement. The
/// participant's affine transform belongs to
/// [`Affine3dScenePlane::accumulated_transform`], not to this temporary
/// measurement scope; clearing it here lets the page-backed query measure
/// direct retained operations in element coordinates.
fn scene_plane_local_measure(item: &PaintDisplayItem, bounds: PaintClip) -> PaintEffectScope {
    let mut item = item.clone();
    match &mut item {
        PaintDisplayItem::StackingContext(context) => {
            context.effects.affine_3d_transform = None;
            context.effects.three_d_participation = ThreeDParticipation::Flat;
        }
        PaintDisplayItem::EffectScope(scope) => {
            scope.effects.affine_3d_transform = None;
            scope.effects.three_d_participation = ThreeDParticipation::Flat;
        }
        PaintDisplayItem::Operation(_)
        | PaintDisplayItem::Primitive(_)
        | PaintDisplayItem::Link(_) => {}
    }
    PaintEffectScope::new(PaintEffects::default(), Some(bounds), vec![item])
}

fn rectangle_polygon(bounds: PaintClip) -> RenderedClipPathPolygon {
    RenderedClipPathPolygon::new(&[
        bounds.bottom_left(),
        bounds.bottom_right(),
        bounds.top_right(),
        bounds.top_left(),
    ])
    .expect("a rectangle has four vertices")
}

fn classify_affine_3d_planes(
    left: &Affine3dScenePlane,
    right: &Affine3dScenePlane,
) -> Affine3dPlaneOrder {
    let left_projected = projected_polygon(left);
    let right_projected = projected_polygon(right);
    let overlap = intersect_convex_polygons(&left_projected, &right_projected);
    if overlap.len() < 3 {
        return Affine3dPlaneOrder::Coplanar;
    }
    let mut has_front = false;
    let mut has_back = false;
    for point in overlap {
        let Some(left_depth) = left.accumulated_transform.depth_at_projected(point) else {
            return Affine3dPlaneOrder::Coplanar;
        };
        let Some(right_depth) = right.accumulated_transform.depth_at_projected(point) else {
            return Affine3dPlaneOrder::Coplanar;
        };
        match signed_depth(left_depth - right_depth) {
            1 => has_front = true,
            -1 => has_back = true,
            0 => {}
            _ => unreachable!(),
        }
    }
    match (has_front, has_back) {
        (true, true) => Affine3dPlaneOrder::Split,
        (true, false) => Affine3dPlaneOrder::InFront,
        (false, true) => Affine3dPlaneOrder::Behind,
        (false, false) => {
            // A depth-equal plane retains CSS paint order, as required by
            // the Newell model. `paint_order` is deliberately stored in the
            // plane record even though the stable caller owns that tie.
            let _ = (left.paint_order, right.paint_order);
            Affine3dPlaneOrder::Coplanar
        }
    }
}

fn projected_polygon(plane: &Affine3dScenePlane) -> Vec<PaintPoint> {
    debug_assert!(plane.paint.bounds.is_some());
    let transform = plane.accumulated_transform.flatten_to_paint_transform();
    plane
        .local_polygon
        .points()
        .iter()
        .map(|point| transform.apply_point(*point))
        .collect()
}

/// Convex Sutherland-Hodgman clipping in the shared flattened context plane.
/// The 3D plane polygons originate as rectangles and each subsequent Newell
/// split preserves convexity, so this is sufficient without a general-path
/// boolean dependency.
fn intersect_convex_polygons(subject: &[PaintPoint], clip: &[PaintPoint]) -> Vec<PaintPoint> {
    if subject.len() < 3 || clip.len() < 3 {
        return Vec::new();
    }
    let clip_clockwise = polygon_signed_area(clip) < 0.0;
    let mut output = subject.to_vec();
    for (&start, &end) in clip
        .iter()
        .zip(clip.iter().cycle().skip(1))
        .take(clip.len())
    {
        let input = std::mem::take(&mut output);
        if input.is_empty() {
            break;
        }
        let mut previous = *input.last().expect("nonempty input");
        let mut previous_inside = point_inside_clip_edge(previous, start, end, clip_clockwise);
        for current in input {
            let current_inside = point_inside_clip_edge(current, start, end, clip_clockwise);
            if current_inside != previous_inside
                && let Some(intersection) = line_intersection(previous, current, start, end)
            {
                output.push(intersection);
            }
            if current_inside {
                output.push(current);
            }
            previous = current;
            previous_inside = current_inside;
        }
    }
    output
}

fn polygon_signed_area(points: &[PaintPoint]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(start, end)| start.x * end.y - end.x * start.y)
        .sum::<f32>()
        * 0.5
}

fn point_inside_clip_edge(
    point: PaintPoint,
    start: PaintPoint,
    end: PaintPoint,
    clip_clockwise: bool,
) -> bool {
    let cross = (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x);
    if clip_clockwise {
        cross <= 1e-4
    } else {
        cross >= -1e-4
    }
}

fn line_intersection(
    start: PaintPoint,
    end: PaintPoint,
    clip_start: PaintPoint,
    clip_end: PaintPoint,
) -> Option<PaintPoint> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let clip_dx = clip_end.x - clip_start.x;
    let clip_dy = clip_end.y - clip_start.y;
    let denominator = dx * clip_dy - dy * clip_dx;
    (denominator.abs() > 1e-6).then(|| {
        let t =
            ((clip_start.x - start.x) * clip_dy - (clip_start.y - start.y) * clip_dx) / denominator;
        PaintPoint::new(start.x + dx * t, start.y + dy * t)
    })
}

fn signed_depth(value: f32) -> i8 {
    const EPSILON: f32 = 1e-4;
    if value > EPSILON {
        1
    } else if value < -EPSILON {
        -1
    } else {
        0
    }
}

fn split_scene_plane_against(
    plane: &Affine3dScenePlane,
    other: &Affine3dScenePlane,
) -> Option<[RenderedClipPathPolygon; 2]> {
    let mut front = Vec::new();
    let mut back = Vec::new();
    let points = plane.local_polygon.points();
    for (&start, &end) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let start_depth = plane_depth_difference(plane, other, start)?;
        let end_depth = plane_depth_difference(plane, other, end)?;
        clip_segment_to_depth_half_planes(
            start,
            end,
            start_depth,
            end_depth,
            &mut front,
            &mut back,
        );
    }
    Some([
        RenderedClipPathPolygon::new(&front)?,
        RenderedClipPathPolygon::new(&back)?,
    ])
}

fn plane_depth_difference(
    plane: &Affine3dScenePlane,
    other: &Affine3dScenePlane,
    point: PaintPoint,
) -> Option<f32> {
    let projected = plane
        .accumulated_transform
        .flatten_to_paint_transform()
        .apply_point(point);
    Some(
        plane.accumulated_transform.depth_at(point)
            - other.accumulated_transform.depth_at_projected(projected)?,
    )
}

fn clip_segment_to_depth_half_planes(
    start: PaintPoint,
    end: PaintPoint,
    start_depth: f32,
    end_depth: f32,
    front: &mut Vec<PaintPoint>,
    back: &mut Vec<PaintPoint>,
) {
    let start_side = signed_depth(start_depth);
    let end_side = signed_depth(end_depth);
    if start_side >= 0 {
        front.push(start);
    }
    if start_side <= 0 {
        back.push(start);
    }
    if (start_side < 0 && end_side > 0) || (start_side > 0 && end_side < 0) {
        let t = start_depth / (start_depth - end_depth);
        let intersection = PaintPoint::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        front.push(intersection);
        back.push(intersection);
    }
}

fn item_with_scene_plane_clip(
    mut item: PaintDisplayItem,
    polygon: RenderedClipPathPolygon,
) -> Option<PaintDisplayItem> {
    match &mut item {
        PaintDisplayItem::StackingContext(context) => {
            context.effects.scene_plane_clip = Some(polygon);
        }
        PaintDisplayItem::EffectScope(scope) => {
            scope.effects.scene_plane_clip = Some(polygon);
        }
        PaintDisplayItem::Operation(_)
        | PaintDisplayItem::Primitive(_)
        | PaintDisplayItem::Link(_) => {
            return None;
        }
    }
    Some(item)
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

    pub(crate) fn contains_affine_3d_transform(&self) -> bool {
        self.bands
            .iter()
            .flatten()
            .any(PaintDisplayItem::contains_affine_3d_transform)
    }

    /// Whether this fragment still contains a CSS 3D matrix whose final
    /// projected bounds are unknown. Overflow-scope elision must not inspect
    /// its unprojected recorded bounds.
    pub(crate) fn contains_unresolved_3d_transform(&self) -> bool {
        self.bands
            .iter()
            .flatten()
            .any(PaintDisplayItem::contains_unresolved_3d_transform)
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
        clip: Option<PaintClip>,
        links: &mut Vec<RenderedLink>,
    ) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                item.push_transformed_links(transform, clip, links);
            }
        }
    }

    /// Slice primitive nodes at a fragmentainer boundary while preserving
    /// paint groups whose principal boxes are monolithic.
    ///
    /// CSS Fragmentation moves a monolithic box as a unit (or lets it overflow
    /// an empty fragmentainer); it does not clip and replay successive pieces
    /// of that box in later fragmentainers:
    /// <https://www.w3.org/TR/css-break-3/#monolithic>.
    pub(crate) fn sliced_primitives_to_fragmentainer_rect(mut self, clip: PaintClip) -> Self {
        self.slice_primitives_to_fragmentainer_rect(clip);
        self
    }

    fn slice_primitives_to_fragmentainer_rect(&mut self, clip: PaintClip) {
        for items in &mut self.bands {
            *items = std::mem::take(items)
                .into_iter()
                .filter_map(|item| item.sliced_primitives_to_fragmentainer_rect(clip))
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
    fn contains_affine_3d_transform(&self) -> bool {
        match self {
            Self::StackingContext(context) => {
                context.effects.affine_3d_transform.is_some()
                    || context.bands.contains_affine_3d_transform()
            }
            Self::EffectScope(scope) => {
                scope.effects.affine_3d_transform.is_some()
                    || scope.items.iter().any(Self::contains_affine_3d_transform)
            }
            Self::Operation(_) | Self::Primitive(_) | Self::Link(_) => false,
        }
    }

    fn contains_unresolved_3d_transform(&self) -> bool {
        match self {
            Self::StackingContext(context) => {
                context.effects.affine_3d_transform.is_some()
                    || context.effects.projective_3d_transform.is_some()
                    || context.effects.descendant_projective_3d_transform.is_some()
                    || context.bands.contains_unresolved_3d_transform()
            }
            Self::EffectScope(scope) => {
                scope.effects.affine_3d_transform.is_some()
                    || scope.effects.projective_3d_transform.is_some()
                    || scope.effects.descendant_projective_3d_transform.is_some()
                    || scope
                        .items
                        .iter()
                        .any(Self::contains_unresolved_3d_transform)
            }
            Self::Operation(_) | Self::Primitive(_) | Self::Link(_) => false,
        }
    }

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
                context.effects.overflow_clip_effect.is_some()
                    || context.bands.contains_overflow_clip()
            }
            Self::EffectScope(scope) => {
                scope.effects.overflow_clip_effect.is_some()
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

    fn sliced_primitives_to_fragmentainer_rect(self, clip: PaintClip) -> Option<Self> {
        match self {
            Self::Primitive(primitive) => primitive.clipped_to_rect(clip).map(Self::Primitive),
            Self::StackingContext(mut context) => {
                context.bands.slice_primitives_to_fragmentainer_rect(clip);
                context.bounds = context.bounds.and_then(|bounds| bounds.intersect(clip));
                (!context.bands.is_empty()).then_some(Self::StackingContext(context))
            }
            Self::EffectScope(mut scope) => {
                if scope.fragmentation == PaintFragmentation::Monolithic {
                    return scope
                        .bounds
                        .is_some_and(|bounds| monolithic_block_start_is_in_slice(bounds, clip))
                        .then_some(Self::EffectScope(scope));
                }
                scope.items = scope
                    .items
                    .into_iter()
                    .filter_map(|item| item.sliced_primitives_to_fragmentainer_rect(clip))
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
        clip: Option<PaintClip>,
        links: &mut Vec<RenderedLink>,
    ) {
        match self {
            Self::Link(link) => {
                let link = link.transformed(transform);
                if let Some(clip) = clip {
                    if let Some(link) = link.clipped_to(clip) {
                        links.push(link);
                    }
                } else {
                    links.push(link);
                }
            }
            Self::StackingContext(context) => {
                context.push_transformed_links(transform, clip, links)
            }
            Self::EffectScope(scope) => scope.push_transformed_links(transform, clip, links),
            Self::Operation(_) | Self::Primitive(_) => {}
        }
    }
}

pub(in crate::document) fn recorded_context_paint_bounds<'a>(
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
    for clip in [effects.absolute_clip, effects.overflow_clip_bounds()]
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
        && effects.affine_3d_transform.is_none()
        && effects.three_d_participation == ThreeDParticipation::Flat
        && !effects.suppress_paint
        && effects.overflow_clip_effect.is_none()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::paint::stacking::StackLevel;
    use crate::document::paint::{contours::OverflowClipEffect, shapes::RenderedRect};

    fn test_plane(transform: Affine3dPaintTransform) -> Affine3dScenePlane {
        let bounds = PaintClip::new(-10.0, -10.0, 20.0, 20.0);
        Affine3dScenePlane {
            paint: PaintEffectScope::new(PaintEffects::default(), Some(bounds), Vec::new()),
            local_polygon: rectangle_polygon(bounds),
            accumulated_transform: transform,
            paint_order: 0,
        }
    }

    fn affine_matrix(matrix: [[f32; 3]; 4]) -> Affine3dPaintTransform {
        let [
            [m11, m12, m13],
            [m21, m22, m23],
            [m31, m32, m33],
            [m41, m42, m43],
        ] = matrix;
        Affine3dPaintTransform::try_from_transform(euclid::Transform3D::new(
            m11, m12, m13, 0.0, m21, m22, m23, 0.0, m31, m32, m33, 0.0, m41, m42, m43, 1.0,
        ))
        .expect("test matrix is affine and invertible")
    }

    #[test]
    fn retained_affine_3d_detection_ignores_ordinary_paint() {
        let mut bands = PaintBandList::default();
        bands.push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(0));

        assert!(!bands.contains_affine_3d_transform());
    }

    #[test]
    fn projective_resolution_keeps_untransformed_operations_indexed_into_the_page() {
        let mut items = vec![PaintDisplayItem::Operation(PaintOperation::Image(0))];

        resolve_projective_items(&mut items, None, None, &Page::new(100.0, 100.0));

        assert!(matches!(
            items.as_slice(),
            [PaintDisplayItem::Operation(PaintOperation::Image(0))]
        ));
    }

    #[test]
    fn perspective_promotes_translate_z_before_affine_lowering() {
        let child_bounds = PaintClip::new(100.0, 140.0, 60.0, 60.0);
        let mut child_bands = PaintBandList::default();
        child_bands.extend_band(
            PaintBand::InFlowBlock,
            [PaintDisplayItem::Primitive(PaintPrimitive::Rect(
                RenderedRect::from_paint_rect(child_bounds.paint_rect(), None),
            ))],
        );
        let child = PaintStackingContext::with_bands(StackLevel::Auto, child_bands)
            .with_effects(PaintEffects {
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 50.0],
                ])),
                ..PaintEffects::default()
            })
            .with_bounds(child_bounds);
        let perspective = Projective3dPaintTransform::from_transform(euclid::Transform3D::new(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -0.01, 0.0, 0.0, 0.0, 1.0,
        ));
        let mut bands = PaintBandList::default();
        bands.push_context_in_band(PaintBand::InFlowBlock, child);
        let mut root =
            PaintStackingContext::with_bands(StackLevel::Auto, bands).with_effects(PaintEffects {
                descendant_projective_3d_transform: Some(perspective),
                ..PaintEffects::default()
            });

        resolve_context_projective_3d(&mut root, None, &Page::new(300.0, 300.0));

        let [PaintDisplayItem::StackingContext(child)] =
            root.bands.bands[PaintBand::InFlowBlock.index()].as_slice()
        else {
            panic!("the perspective owner keeps its child context");
        };
        assert!(child.effects.affine_3d_transform.is_none());
        assert!(child.effects.transform.is_none());
        assert!(matches!(
            child.bands.bands[PaintBand::InFlowBlock.index()].as_slice(),
            [PaintDisplayItem::Primitive(PaintPrimitive::Path(_))]
        ));
    }

    #[test]
    fn transparent_overflow_scope_is_retained_in_a_3d_scene() {
        let clip = PaintClip::new(0.0, 0.0, 50.0, 50.0);
        let scope = PaintEffectScope::new(
            PaintEffects::transparent_overflow_scope(OverflowClipEffect::Rect(clip)),
            Some(clip),
            Vec::new(),
        );

        let expanded =
            expand_transparent_layout_bridges(vec![PaintDisplayItem::EffectScope(scope)]);
        assert!(matches!(
            expanded.as_slice(),
            [PaintDisplayItem::EffectScope(scope)]
                if scope.effects.overflow_clip_effect == Some(OverflowClipEffect::Rect(clip))
        ));
    }

    #[test]
    fn retained_affine_3d_detection_reaches_nested_contexts() {
        let child = PaintStackingContext::with_bands(StackLevel::Auto, PaintBandList::default())
            .with_effects(PaintEffects {
                affine_3d_transform: Some(Affine3dPaintTransform::identity()),
                ..PaintEffects::default()
            });
        let mut bands = PaintBandList::default();
        bands.push_context_in_band(PaintBand::AutoZeroZ, child);

        assert!(bands.contains_affine_3d_transform());
    }

    #[test]
    fn transparent_layout_bridge_expands_retained_3d_descendants_into_parent_scene() {
        let child = PaintStackingContext::with_bands(StackLevel::Auto, PaintBandList::default())
            .with_effects(PaintEffects {
                affine_3d_transform: Some(Affine3dPaintTransform::identity()),
                ..PaintEffects::default()
            });
        let bridge = PaintEffectScope::new(
            PaintEffects {
                three_d_participation: ThreeDParticipation::TransparentLayoutBridge,
                ..PaintEffects::default()
            },
            None,
            vec![PaintDisplayItem::StackingContext(child)],
        );

        let expanded =
            expand_transparent_layout_bridges(vec![PaintDisplayItem::EffectScope(bridge)]);
        assert!(matches!(
            expanded.as_slice(),
            [PaintDisplayItem::StackingContext(context)]
                if context.effects.affine_3d_transform.is_some()
        ));
    }

    #[test]
    fn retained_flat_scope_is_a_scene_plane_boundary() {
        let scope = PaintEffectScope::new(
            PaintEffects {
                three_d_participation: ThreeDParticipation::Flat,
                ..PaintEffects::default()
            },
            None,
            Vec::new(),
        );

        assert!(is_affine_3d_scene_participant(
            &PaintDisplayItem::EffectScope(scope)
        ));
    }

    #[test]
    fn affine_scene_keeps_ordinary_segments_on_both_sides_of_a_participant() {
        let mut bands = PaintBandList::default();
        bands.push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(1));
        let participant =
            PaintStackingContext::with_bands(StackLevel::Auto, PaintBandList::default())
                .with_effects(PaintEffects {
                    affine_3d_transform: Some(Affine3dPaintTransform::identity()),
                    ..PaintEffects::default()
                })
                .with_bounds(PaintClip::new(0.0, 0.0, 10.0, 10.0));
        bands.push_context_in_band(PaintBand::InFlowBlock, participant);
        bands.push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(2));

        resolve_band_list_affine_3d(
            &mut bands,
            Some(Affine3dPaintTransform::identity()),
            false,
            None,
            &Page::new(100.0, 100.0),
        );

        let scene = &bands.bands[PaintBand::InFlowBlock.index()];
        assert!(matches!(
            &scene[0],
            PaintDisplayItem::EffectScope(PaintEffectScope { items, .. })
                if matches!(items.as_slice(), [PaintDisplayItem::Operation(PaintOperation::Rect(1))])
        ));
        assert!(matches!(scene[1], PaintDisplayItem::StackingContext(_)));
        assert!(matches!(
            &scene[2],
            PaintDisplayItem::EffectScope(PaintEffectScope { items, .. })
                if matches!(items.as_slice(), [PaintDisplayItem::Operation(PaintOperation::Rect(2))])
        ));
    }

    #[test]
    fn transparent_bridge_descendant_joins_parent_depth_ordering() {
        let bounds = PaintClip::new(0.0, 0.0, 10.0, 10.0);
        let mut behind_bands = PaintBandList::default();
        behind_bands.push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(1));
        let behind = PaintStackingContext::with_bands(StackLevel::Auto, behind_bands)
            .with_effects(PaintEffects {
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, -10.0],
                ])),
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        let bridge = PaintEffectScope::new(
            PaintEffects {
                three_d_participation: ThreeDParticipation::TransparentLayoutBridge,
                ..PaintEffects::default()
            },
            Some(bounds),
            vec![PaintDisplayItem::StackingContext(behind)],
        );
        let mut in_front_bands = PaintBandList::default();
        in_front_bands.push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(2));
        let in_front = PaintStackingContext::with_bands(StackLevel::Auto, in_front_bands)
            .with_effects(PaintEffects {
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 10.0],
                ])),
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        let mut bands = PaintBandList::default();
        // Source order deliberately puts the front plane first. Newell
        // ordering must still paint the bridged back plane first.
        bands.push_context_in_band(PaintBand::InFlowBlock, in_front);
        bands.push_effect_scope_in_band(PaintBand::InFlowBlock, bridge);

        resolve_band_list_affine_3d(
            &mut bands,
            Some(Affine3dPaintTransform::identity()),
            false,
            Some(bounds),
            &Page::new(100.0, 100.0),
        );

        let mut operations = Vec::new();
        bands.push_flattened_operations(&mut operations);
        assert_eq!(
            operations,
            vec![PaintOperation::Rect(1), PaintOperation::Rect(2)]
        );
    }

    #[test]
    fn flat_descendant_uses_the_accumulated_matrix_for_backface_visibility() {
        let bounds = PaintClip::new(0.0, 0.0, 10.0, 10.0);
        let child = PaintStackingContext::with_bands(StackLevel::Auto, PaintBandList::default())
            .with_effects(PaintEffects {
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [3.0, 0.0, 0.0],
                ])),
                hide_backface: true,
                three_d_participation: ThreeDParticipation::Flat,
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        let mut bands = PaintBandList::default();
        bands.push_context_in_band(PaintBand::InFlowBlock, child);
        let mut root = PaintStackingContext::with_bands(StackLevel::Auto, bands)
            .with_effects(PaintEffects {
                // A Z reflection makes the child's accumulated m33 negative,
                // while the X translation verifies that the final PDF CTM is
                // composed through the preserve-3d ancestor.
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, -1.0],
                    [5.0, 0.0, 0.0],
                ])),
                three_d_participation: ThreeDParticipation::Preserve3d,
                ..PaintEffects::default()
            })
            .with_bounds(bounds);

        resolve_context_affine_3d(&mut root, None, &Page::new(100.0, 100.0));

        let [PaintDisplayItem::StackingContext(child)] =
            root.bands.bands[PaintBand::InFlowBlock.index()].as_slice()
        else {
            panic!("the flat child remains the context's only retained item");
        };
        assert!(child.effects.suppress_paint);
        let transform = child
            .effects
            .transform
            .expect("a flat descendant receives its accumulated PDF CTM");
        assert_eq!(transform.e(), 8.0);
        assert_eq!(transform.f(), 0.0);
    }

    #[test]
    fn singular_preserve_3d_context_suppresses_its_scene() {
        let mut root = PaintStackingContext::with_bands(StackLevel::Auto, PaintBandList::default())
            .with_effects(PaintEffects {
                affine_3d_transform: Some(affine_matrix([
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                ])),
                three_d_participation: ThreeDParticipation::Preserve3d,
                ..PaintEffects::default()
            });

        resolve_context_affine_3d(&mut root, None, &Page::new(100.0, 100.0));

        assert!(root.effects.suppress_paint);
    }

    #[test]
    fn affine_scene_plane_classification_keeps_coplanar_source_order() {
        let plane = test_plane(Affine3dPaintTransform::identity());

        assert_eq!(
            classify_affine_3d_planes(&plane, &plane),
            Affine3dPlaneOrder::Coplanar
        );
    }

    #[test]
    fn affine_scene_plane_classification_orders_constant_depth() {
        let front = test_plane(affine_matrix([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 10.0],
        ]));
        let back = test_plane(affine_matrix([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -10.0],
        ]));

        assert_eq!(
            classify_affine_3d_planes(&front, &back),
            Affine3dPlaneOrder::InFront
        );
        assert_eq!(
            classify_affine_3d_planes(&back, &front),
            Affine3dPlaneOrder::Behind
        );
    }

    #[test]
    fn affine_scene_plane_classification_splits_varying_depth() {
        let horizontal = test_plane(Affine3dPaintTransform::identity());
        // A valid affine 3D shear places one half of the plane in front of
        // the shared page plane and the other half behind it.
        let sloped = test_plane(affine_matrix([
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        ]));

        assert_eq!(
            classify_affine_3d_planes(&horizontal, &sloped),
            Affine3dPlaneOrder::Split
        );
        assert!(split_scene_plane_against(&horizontal, &sloped).is_some());
        assert!(split_scene_plane_against(&sloped, &horizontal).is_some());
    }
}
