use super::*;

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
    /// CSS Transforms and CSS Color opacity create stacking contexts whose
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

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.translated(offset),
            effects: self.effects,
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
        }
    }

    pub(in crate::document) fn into_operation_nodes(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.into_operation_nodes(operations),
            effects: self.effects,
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

    pub(in crate::document) fn push_transformed_links(
        &self,
        parent_transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        let transform = if let Some(transform) = self.effects.transform {
            parent_transform.multiply(transform)
        } else {
            parent_transform
        };
        self.bands.push_transformed_links(transform, links);
    }
}

pub(crate) struct RecordedPaintFragment {
    pub(in crate::document) operations: Vec<PaintOperation>,
    pub(in crate::document) display_list: PaintDisplayList,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintFragment {
    pub(in crate::document) display_list: PaintDisplayList,
    pub links: Vec<RenderedLink>,
}

impl PaintFragment {
    /// Build a fragment from page-local primitives in their current paint order.
    ///
    /// The primitive sequence represents one CSS paint-order band as defined by
    /// CSS 2.2 Appendix E before it is eventually serialized as ordered PDF
    /// drawing operators.
    pub(crate) fn from_primitives(
        primitives: Vec<PaintPrimitive>,
        links: Vec<RenderedLink>,
    ) -> Self {
        Self {
            display_list: PaintDisplayList::from_primitives(primitives),
            links,
        }
    }

    /// Build a fragment whose root is a captured CSS stacking context.
    ///
    /// This preserves the recursive stacking relationship from CSS 2.2
    /// Appendix E until the fragment is flattened for the PDF page content
    /// stream.
    pub(crate) fn from_stacking_context(context: PaintStackingContext) -> Self {
        Self::from_stacking_context_in_band(context.stack_level.paint_band(), context)
    }

    pub(crate) fn from_stacking_context_in_band(
        band: PaintBand,
        context: PaintStackingContext,
    ) -> Self {
        Self {
            display_list: PaintDisplayList {
                bands: {
                    let mut bands = PaintBandList::default();
                    bands.push_context_in_band(band, context);
                    bands
                },
            },
            links: Vec::new(),
        }
    }

    pub(crate) fn flattened_primitives(&self) -> Vec<PaintPrimitive> {
        self.display_list.flattened_primitives()
    }

    pub(crate) fn prepend_primitives_in_band(
        &mut self,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let existing_primitives = self.flattened_primitives();
        let items = primitives
            .into_iter()
            .filter(|primitive| {
                !primitive_is_covered_by_later_opaque_rect(primitive, &existing_primitives)
            })
            .map(PaintDisplayItem::Primitive)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        self.display_list.bands.bands[band.index()].splice(0..0, items);
    }

    pub(crate) fn append_primitives_in_band(
        &mut self,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        self.display_list.bands.extend_band(
            band,
            primitives.into_iter().map(PaintDisplayItem::Primitive),
        );
    }

    /// Move block decorations into the parent's normal-flow block paint band.
    ///
    /// CSS 2.2 Appendix E paints backgrounds and borders of in-flow
    /// non-positioned block descendants in the parent stacking context's block
    /// phase. Lifting only this band avoids making the block atomically cover
    /// later inline painting from earlier siblings:
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn promote_background_border_to_in_flow_block(&mut self) {
        let background_items =
            std::mem::take(&mut self.display_list.bands.bands[PaintBand::BackgroundBorder.index()]);
        if background_items.is_empty() {
            return;
        }
        self.display_list.bands.bands[PaintBand::InFlowBlock.index()]
            .splice(0..0, background_items);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.display_list.is_empty() && self.links.is_empty()
    }

    pub(crate) fn first_line_y(&self) -> Option<f32> {
        self.flattened_primitives()
            .into_iter()
            .find_map(|primitive| match primitive {
                PaintPrimitive::Line(line) => Some(line.y()),
                _ => None,
            })
    }

    pub(crate) fn last_line_y(&self) -> Option<f32> {
        self.flattened_primitives()
            .into_iter()
            .rev()
            .find_map(|primitive| match primitive {
                PaintPrimitive::Line(line) => Some(line.y()),
                _ => None,
            })
    }

    pub(crate) fn bounds(&self) -> Option<PaintClip> {
        let mut bounds: Option<PaintClip> = None;
        for primitive in self.flattened_primitives() {
            let Some(primitive_bounds) = primitive.bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                Some(existing) => existing.union(primitive_bounds),
                None => primitive_bounds,
            });
        }
        for link in &self.links {
            let link_bounds = PaintClip::from_paint_rect(link.paint_rect());
            bounds = Some(match bounds {
                Some(existing) => existing.union(link_bounds),
                None => link_bounds,
            });
        }
        bounds
    }

    pub(crate) fn translated(mut self, offset: PaintVector) -> Self {
        self.display_list = self.display_list.translated(offset);
        self.links = self
            .links
            .into_iter()
            .map(|link| link.translated(offset))
            .collect();
        self
    }

    /// Return a fragment whose non-decoration contents are overflow-clipped
    /// without introducing a stacking context.
    ///
    /// CSS Overflow clips descendants but does not make a normal block an
    /// atomic stacking context. Each existing paint band is therefore wrapped
    /// in-place so Appendix E ordering between sibling block backgrounds and
    /// inline foregrounds remains intact:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn with_contents_effect_scoped_to_rect(self, clip: PaintClip) -> Self {
        let mut bands = self.display_list.bands;
        let effects = PaintEffects {
            overflow_clip: Some(clip),
            ..PaintEffects::default()
        };

        for band in PaintBand::ORDER {
            if matches!(band, PaintBand::BackgroundBorder | PaintBand::Outline) {
                continue;
            }
            let items = std::mem::take(&mut bands.bands[band.index()]);
            if items.is_empty() {
                continue;
            }
            bands
                .push_effect_scope_in_band(band, PaintEffectScope::new(effects, Some(clip), items));
        }

        let content_links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(PaintDisplayItem::Link(link))
            })
            .collect::<Vec<_>>();
        if !content_links.is_empty() {
            bands.push_effect_scope_in_band(
                PaintBand::Inline,
                PaintEffectScope::new(effects, Some(clip), content_links),
            );
        }

        Self {
            display_list: PaintDisplayList { bands },
            links: Vec::new(),
        }
    }

    /// Return a fragment whose contents are overflow-clipped while its own
    /// decorations remain outside the clip.
    ///
    /// CSS Overflow clips a box's contents to the overflow clip edge, while CSS
    /// Backgrounds and Borders paints the box's own background, border, and
    /// outline as the element's decoration. Keeping decoration bands outside
    /// the clipped content context preserves that distinction when layout has
    /// already captured a whole element fragment:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn with_contents_clipped_to_rect(
        self,
        clip: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut content_bands = self.display_list.bands;
        let mut decoration_bands = PaintBandList::default();
        decoration_bands.bands[PaintBand::BackgroundBorder.index()] =
            std::mem::take(&mut content_bands.bands[PaintBand::BackgroundBorder.index()]);
        decoration_bands.bands[PaintBand::Outline.index()] =
            std::mem::take(&mut content_bands.bands[PaintBand::Outline.index()]);

        let content_links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(link)
            })
            .collect::<Vec<_>>();
        let content_fragment = Self {
            display_list: PaintDisplayList {
                bands: content_bands,
            },
            links: content_links,
        };

        if !content_fragment.is_empty() || !child_contexts.is_empty() {
            let content_context =
                PaintStackingContext::from_banded_fragment(content_fragment, child_contexts)
                    .with_effects(PaintEffects {
                        overflow_clip: Some(clip),
                        ..PaintEffects::default()
                    })
                    .with_bounds(clip);
            decoration_bands.push_context_in_band(PaintBand::InFlowBlock, content_context);
        }

        Self {
            display_list: PaintDisplayList {
                bands: decoration_bands,
            },
            links: Vec::new(),
        }
    }

    /// Return a fragment whose flattened public primitive data is clipped to a
    /// rectangular page-local slice.
    ///
    /// Context effects preserve the same clip for PDF output; this helper keeps
    /// `Document` inspection data aligned with fragmented paint:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(crate) fn clipped_to_rect(self, clip: PaintClip) -> Self {
        let primitives = self
            .flattened_primitives()
            .into_iter()
            .filter_map(|primitive| primitive.clipped_to_rect(clip))
            .collect::<Vec<_>>();
        let links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(link)
            })
            .collect::<Vec<_>>();
        Self::from_primitives(primitives, links)
    }
}

pub(in crate::document) fn primitive_is_covered_by_later_opaque_rect(
    primitive: &PaintPrimitive,
    later_primitives: &[PaintPrimitive],
) -> bool {
    let PaintPrimitive::Rect(rect) = primitive else {
        return false;
    };
    if rect.stroke.is_some() || rect.fill.is_none_or(|fill| fill.a < 1.0) {
        return false;
    }
    later_primitives.iter().any(|later| {
        let PaintPrimitive::Rect(later) = later else {
            return false;
        };
        later.stroke.is_none()
            && later.fill.is_some_and(|fill| fill.a >= 1.0)
            && same_rect_geometry(rect, later)
    })
}

pub(in crate::document) fn same_rect_geometry(left: &RenderedRect, right: &RenderedRect) -> bool {
    (left.x() - right.x()).abs() < 0.001
        && (left.y() - right.y()).abs() < 0.001
        && (left.width() - right.width()).abs() < 0.001
        && (left.height() - right.height()).abs() < 0.001
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRect {
    pub(in crate::document) rect: PaintRect,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRect {
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self {
            rect: PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            fill,
            stroke,
            stroke_width,
        }
    }

    pub(crate) fn from_paint_rect(rect: PaintRect, fill: Option<Color>) -> Self {
        Self {
            rect,
            fill,
            stroke: None,
            stroke_width: 0.0,
        }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn set_paint_rect(&mut self, rect: PaintRect) {
        self.rect = rect;
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRect {
    pub(in crate::document) rect: PaintRect,
    pub radii: RenderedRoundedRectRadii,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRoundedRect {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: RenderedRoundedRectRadii,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self::from_paint_rect(
            PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            radii,
            fill,
            stroke,
            stroke_width,
        )
    }

    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        radii: RenderedRoundedRectRadii,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self {
            rect,
            radii,
            fill,
            stroke,
            stroke_width,
        }
    }

    pub fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(self) -> f32 {
        self.rect.size.width
    }

    pub fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }
}

/// A generic PDF path paint primitive used when a CSS feature cannot be
/// represented by a rectangle, rounded rectangle, or single stroke.
///
/// CSS Backgrounds and Borders Level 3 models border areas as curved regions,
/// and PDF content streams represent those regions with path construction and
/// painting operators: <https://www.w3.org/TR/css-backgrounds-3/#borders> and
/// ISO 32000-1:2008, 8.5 "Path Construction and Painting".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPath {
    pub clip: Option<RenderedPathClip>,
    pub commands: Vec<RenderedPathCommand>,
    pub fill: Option<Color>,
    pub fill_rule: RenderedPathFillRule,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedPath {
    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill: Option<Color>,
        fill_rule: RenderedPathFillRule,
        stroke: Option<Color>,
        stroke_width: f32,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            clip,
            commands,
            fill,
            fill_rule,
            stroke,
            stroke_width,
        }
    }

    pub(in crate::document) fn translated(mut self, offset: PaintVector) -> Self {
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested_clip in &mut clip.additional_clips {
                for command in &mut nested_clip.commands {
                    command.translate(offset);
                }
            }
        }
        for command in &mut self.commands {
            command.translate(offset);
        }
        self
    }
}

/// A PDF path clipping scope applied before painting a vector path.
///
/// PDF clipping paths are established with `W`/`W*` and the current path, then
/// later drawing is limited to that region until the graphics state is
/// restored. CSS border side painting uses this to isolate one side of a
/// rounded border ring when side colors or styles differ:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping> and ISO
/// 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClip {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
    pub additional_clips: Vec<RenderedPathClipPath>,
}

impl RenderedPathClip {
    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill_rule: RenderedPathFillRule,
        additional_clips: Vec<RenderedPathClipPath>,
    ) -> Self {
        Self {
            commands,
            fill_rule,
            additional_clips,
        }
    }
}

/// One additional PDF clipping path intersected with an active clip scope.
///
/// CSS rounded patterned borders need the intersection of a side transition
/// region and the rounded border ring. PDF models this by applying multiple
/// clipping paths in sequence within one graphics state:
/// ISO 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClipPath {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
}

impl RenderedPathClipPath {
    pub(crate) fn new(commands: Vec<RenderedPathCommand>, fill_rule: RenderedPathFillRule) -> Self {
        Self {
            commands,
            fill_rule,
        }
    }
}

/// A PDF-compatible path construction command.
///
/// The variants map directly to PDF `m`, `l`, `c`, and `h` operators from ISO
/// 32000-1:2008, 8.5.2 "Path Construction Operators".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderedPathCommand {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

impl RenderedPathCommand {
    pub(crate) fn move_to(point: PaintPoint) -> Self {
        Self::MoveTo(point)
    }

    pub(crate) fn line_to(point: PaintPoint) -> Self {
        Self::LineTo(point)
    }

    pub(crate) fn curve_to(control_1: PaintPoint, control_2: PaintPoint, end: PaintPoint) -> Self {
        Self::CurveTo {
            control_1,
            control_2,
            end,
        }
    }

    pub(crate) fn typed_points(self) -> RenderedPathCommandPoints {
        match self {
            Self::MoveTo(point) => RenderedPathCommandPoints::MoveTo(point),
            Self::LineTo(point) => RenderedPathCommandPoints::LineTo(point),
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            },
            Self::Close => RenderedPathCommandPoints::Close,
        }
    }

    pub(in crate::document) fn translate(&mut self, offset: PaintVector) {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => {
                *point += offset;
            }
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                *control_1 += offset;
                *control_2 += offset;
                *end += offset;
            }
            Self::Close => {}
        }
    }
}

/// Typed paint-space points for a rendered path command.
///
/// The public command enum keeps scalar fields for compatibility, while this
/// view gives the PDF backend explicit paint-space coordinates before the
/// final conversion to PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RenderedPathCommandPoints {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

/// Fill rule for a PDF path.
///
/// PDF defines nonzero winding (`f`) and even-odd (`f*`) fill operators; CSS
/// border rings use even-odd filling so the padding-edge subpath cuts out the
/// content area without depending on subpath winding direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RenderedPathFillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRectRadii {
    pub top_left: RenderedCornerRadius,
    pub top_right: RenderedCornerRadius,
    pub bottom_right: RenderedCornerRadius,
    pub bottom_left: RenderedCornerRadius,
}

impl RenderedRoundedRectRadii {
    pub const ZERO: Self = Self {
        top_left: RenderedCornerRadius::ZERO,
        top_right: RenderedCornerRadius::ZERO,
        bottom_right: RenderedCornerRadius::ZERO,
        bottom_left: RenderedCornerRadius::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedCornerRadius {
    pub(in crate::document) size: PaintSize,
}

impl RenderedCornerRadius {
    pub const ZERO: Self = Self {
        size: PaintSize::new(0.0, 0.0),
    };

    pub fn new(x: f32, y: f32) -> Self {
        Self {
            size: PaintSize::new(x.max(0.0), y.max(0.0)),
        }
    }

    pub fn x(&self) -> f32 {
        self.size.width
    }

    pub fn y(&self) -> f32 {
        self.size.height
    }

    pub(crate) fn inset(&mut self, inset: f32) {
        self.size.width = (self.size.width - inset).max(0.0);
        self.size.height = (self.size.height - inset).max(0.0);
    }

    pub(crate) fn scale(&mut self, factor: f32) {
        self.size.width *= factor;
        self.size.height *= factor;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedStroke {
    pub(in crate::document) start: PaintPoint,
    pub(in crate::document) end: PaintPoint,
    pub width: f32,
    pub color: Color,
    pub dash: Option<(f32, f32)>,
}
