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

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
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

    pub(in crate::document) fn primitive_node_copy(&self, page: &Page) -> Self {
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

pub(crate) struct RecordedPaintFragment {
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

    pub(in crate::document) fn for_each_flattened_primitive<'a>(
        &'a self,
        f: &mut impl FnMut(&'a PaintPrimitive),
    ) {
        self.display_list.for_each_flattened_primitive(f);
    }

    pub(crate) fn prepend_primitives_in_band(
        &mut self,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let items = primitives
            .into_iter()
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

    /// Prepend one fragment-local decoration as an indivisible paint unit.
    ///
    /// With `box-decoration-break: clone`, border and padding can extend
    /// beyond the fragmentainer's content capacity. Anonymous-column slicing
    /// selects the decoration by its fragment block-start and retains the
    /// complete cloned box instead of interpreting that overflow as more flow.
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>
    pub(crate) fn prepend_monolithic_primitives_in_band(
        &mut self,
        band: PaintBand,
        bounds: PaintClip,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let items = primitives
            .into_iter()
            .map(PaintDisplayItem::Primitive)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        self.display_list.bands.bands[band.index()].insert(
            0,
            PaintDisplayItem::EffectScope(PaintEffectScope::monolithic(bounds, items)),
        );
    }

    pub(crate) fn append_monolithic_primitives_in_band(
        &mut self,
        band: PaintBand,
        bounds: PaintClip,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let items = primitives
            .into_iter()
            .map(PaintDisplayItem::Primitive)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        self.display_list.bands.bands[band.index()].push(PaintDisplayItem::EffectScope(
            PaintEffectScope::monolithic(bounds, items),
        ));
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
        let in_flow = &mut self.display_list.bands.bands[PaintBand::InFlowBlock.index()];
        in_flow.splice(0..0, background_items);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.display_list.is_empty() && self.links.is_empty()
    }

    pub(crate) fn first_line_y(&self) -> Option<f32> {
        let mut first_line_y = None;
        self.for_each_flattened_primitive(&mut |primitive| {
            if first_line_y.is_none()
                && let PaintPrimitive::Line(line) = primitive
            {
                first_line_y = Some(line.y());
            }
        });
        first_line_y
    }

    pub(crate) fn bounds(&self) -> Option<PaintClip> {
        let mut bounds: Option<PaintBounds> = None;
        self.for_each_flattened_primitive(&mut |primitive| {
            let Some(primitive_bounds) = primitive.bounds() else {
                return;
            };
            match &mut bounds {
                Some(bounds) => bounds.include_paint_rect(primitive_bounds.paint_rect()),
                None => bounds = Some(PaintBounds::from_paint_rect(primitive_bounds.paint_rect())),
            }
        });
        for link in &self.links {
            match &mut bounds {
                Some(bounds) => bounds.include_paint_rect(link.paint_rect()),
                None => bounds = Some(PaintBounds::from_paint_rect(link.paint_rect())),
            }
        }
        bounds.map(PaintBounds::into_paint_clip)
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
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
        self.with_contents_effect_scoped_to_clip(clip, None, false)
    }

    /// Scope paint-contained contents while preserving the edge coverage of a
    /// primitive that is already wholly inside the containment rectangle.
    pub(crate) fn with_paint_containment_contents_effect_scoped_to_rect(
        self,
        clip: PaintClip,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip(clip, None, true)
    }

    /// Inserts captured positioned descendants into their normal paint bands,
    /// then scopes the table contents with an in-band overflow effect.
    ///
    /// This keeps overflow clipping out of an extra stacking-context layer.
    /// Table fragments can therefore commit each page piece independently
    /// rather than recursively wrapping the preceding fragment state.
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clipping> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    pub(crate) fn with_contents_effect_scoped_to_rect_and_child_contexts(
        mut self,
        clip: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        for context in child_contexts {
            self.display_list.bands.push_context(context);
        }
        self.display_list.bands.sort_stacking_contexts();
        self.with_contents_effect_scoped_to_rect(clip)
    }

    /// Clip non-decoration contents to a rounded padding edge.
    ///
    /// CSS paint containment uses the padding edge including its curved
    /// corners, while the rectangular clip remains useful for bounds and link
    /// intersection bookkeeping:
    /// <https://www.w3.org/TR/css-contain-1/#containment-paint>.
    pub(crate) fn with_contents_effect_scoped_to_rounded_rect(
        self,
        clip: PaintClip,
        rounded_clip: RenderedRoundedRect,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip(clip, Some(rounded_clip), false)
    }

    fn with_contents_effect_scoped_to_clip(
        self,
        clip: PaintClip,
        rounded_clip: Option<RenderedRoundedRect>,
        elide_covered_rectangular_clip: bool,
    ) -> Self {
        let mut bands = self.display_list.bands;
        let effects = PaintEffects {
            overflow_clip: Some(clip),
            rounded_overflow_clip: rounded_clip,
            ..PaintEffects::default()
        };

        for band in PaintBand::ORDER {
            if matches!(
                band,
                PaintBand::BackgroundBorder | PaintBand::TableCollapsedBorder | PaintBand::Outline
            ) {
                continue;
            }
            let items = std::mem::take(&mut bands.bands[band.index()]);
            if items.is_empty() {
                continue;
            }
            let (inside, overflowing): (Vec<_>, Vec<_>) = if elide_covered_rectangular_clip {
                items
                    .into_iter()
                    .partition(|item| item.is_wholly_contained_by_rect(clip))
            } else {
                (Vec::new(), items)
            };
            // Preserve a primitive's original edge coverage when it is known
            // to be wholly inside the containment rectangle. Everything that
            // can overflow (or whose final bounds are deferred) remains in
            // the ordinary PDF clip scope.
            for item in inside {
                bands.bands[band.index()].push(item);
            }
            if !overflowing.is_empty() {
                bands.push_effect_scope_in_band(
                    band,
                    PaintEffectScope::new(effects, Some(clip), overflowing),
                );
            }
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

    /// Apply a rectangular paint effect to every band in this captured
    /// fragment.
    ///
    /// This is used when the captured fragment is entirely descendant paint
    /// of an outer principal box (for example a table caption captured apart
    /// from the table box). Unlike `with_contents_effect_scoped_to_rect`, no
    /// band here represents the outer box's own decoration.
    /// <https://www.w3.org/TR/css-contain-1/#containment-paint>
    pub(crate) fn with_effect_scoped_to_rect_all_bands(mut self, clip: PaintClip) -> Self {
        let effects = PaintEffects {
            overflow_clip: Some(clip),
            ..PaintEffects::default()
        };
        for band in PaintBand::ORDER {
            let items = std::mem::take(&mut self.display_list.bands.bands[band.index()]);
            if items.is_empty() {
                continue;
            }
            self.display_list
                .bands
                .push_effect_scope_in_band(band, PaintEffectScope::new(effects, Some(clip), items));
        }
        let links = std::mem::take(&mut self.links)
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(PaintDisplayItem::Link(link))
            })
            .collect::<Vec<_>>();
        if !links.is_empty() {
            self.display_list.bands.push_effect_scope_in_band(
                PaintBand::Inline,
                PaintEffectScope::new(effects, Some(clip), links),
            );
        }
        self
    }

    /// Return an exact overflow clip already attached by the formatting
    /// context that knew the principal box's used padding-box geometry.
    ///
    /// A containing stacking-context wrapper may need to extend this clip to
    /// captured positioned descendants. Reading the in-band scope avoids
    /// reconstructing the edge from descendant ink bounds.
    pub(crate) fn top_level_contents_overflow_clip(&self) -> Option<PaintClip> {
        PaintBand::ORDER
            .into_iter()
            .filter(|band| {
                !matches!(
                    band,
                    PaintBand::BackgroundBorder
                        | PaintBand::TableCollapsedBorder
                        | PaintBand::Outline
                )
            })
            .flat_map(|band| &self.display_list.bands.bands[band.index()])
            .find_map(|item| match item {
                PaintDisplayItem::EffectScope(scope) => scope.effects.overflow_clip,
                _ => None,
            })
    }

    pub(crate) fn contains_overflow_clip(&self) -> bool {
        self.display_list.bands.contains_overflow_clip()
    }

    pub(crate) fn with_primitives_clipped_to_rect_preserving_structure(
        mut self,
        clip: PaintClip,
    ) -> Self {
        self.display_list.bands = self.display_list.bands.clipped_primitives_to_rect(clip);
        self.links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(link)
            })
            .collect();
        self
    }

    /// Clip a captured fragment only in the physical page block direction.
    ///
    /// Anonymous columns fragment content in their block axis, but CSS
    /// Multicol requires content extending outside a column box in the inline
    /// axis to remain visible unless the multicol container's own overflow
    /// later clips it. Deriving the inline extent from the captured fragment
    /// preserves arbitrarily nested overflow while retaining stacking and
    /// effect scopes:
    /// <https://www.w3.org/TR/css-multicol-1/#overflow-inside-multicol>.
    pub(crate) fn with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
        self,
        block_clip: PaintClip,
        clip_crossing_ink: bool,
    ) -> Self {
        let Some(bounds) = self.bounds() else {
            return self;
        };
        // A small guard on both sides keeps zero-inline-size paths and glyph
        // bounds inside the geometrical intersection without imposing any
        // authored inline clip.
        let inline_guard = 1.0;
        let clip = PaintClip::new(
            bounds.x() - inline_guard,
            block_clip.y(),
            bounds.width() + inline_guard * 2.0,
            block_clip.height(),
        );
        let mut fragment = self;
        fragment.display_list.bands = fragment
            .display_list
            .bands
            .sliced_primitives_to_fragmentainer_rect(clip);
        if clip_crossing_ink
            && !fragment
                .display_list
                .bands
                .contains_monolithic_fragmentation()
        {
            let effects = PaintEffects {
                overflow_clip: Some(clip),
                ..PaintEffects::default()
            };
            for band in PaintBand::ORDER {
                let items = std::mem::take(&mut fragment.display_list.bands.bands[band.index()]);
                if items.is_empty() {
                    continue;
                }
                fragment.display_list.bands.push_effect_scope_in_band(
                    band,
                    PaintEffectScope::new(effects, Some(clip), items),
                );
            }
        }
        fragment.links = fragment
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(link)
            })
            .collect();
        fragment
    }

    /// Retain a principal box's paint as one fragmentation unit.
    ///
    /// Size containment makes the principal box monolithic. Wrapping each
    /// existing paint band in-place preserves CSS Appendix E ordering while
    /// allowing anonymous column slicing to select the box once and keep its
    /// background, border, contents, and outline together:
    /// <https://www.w3.org/TR/css-contain-1/#containment-size> and
    /// <https://www.w3.org/TR/css-break-3/#monolithic>.
    pub(crate) fn with_monolithic_fragmentation_scope(mut self, bounds: PaintClip) -> Self {
        for band in PaintBand::ORDER {
            let items = std::mem::take(&mut self.display_list.bands.bands[band.index()]);
            if items.is_empty() {
                continue;
            }
            self.display_list
                .bands
                .push_effect_scope_in_band(band, PaintEffectScope::monolithic(bounds, items));
        }
        if !self.links.is_empty() {
            let links = std::mem::take(&mut self.links)
                .into_iter()
                .map(PaintDisplayItem::Link)
                .collect();
            self.display_list.bands.push_effect_scope_in_band(
                PaintBand::Inline,
                PaintEffectScope::monolithic(bounds, links),
            );
        }
        self
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
        decoration_bands.bands[PaintBand::TableCollapsedBorder.index()] =
            std::mem::take(&mut content_bands.bands[PaintBand::TableCollapsedBorder.index()]);
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
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

#[allow(dead_code)]
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

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
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
    pub(crate) transform: PaintTransform,
    pub commands: Vec<RenderedPathCommand>,
    pub fill: Option<Color>,
    pub(crate) fill_paint: Option<RenderedPathPaint>,
    pub fill_rule: RenderedPathFillRule,
    pub stroke: Option<Color>,
    pub(crate) stroke_paint: Option<RenderedPathPaint>,
    pub stroke_width: f32,
    pub(crate) stroke_style: RenderedPathStrokeStyle,
    pub(crate) paint_order: RenderedPathPaintOrder,
}

/// A vector paint source for a [`RenderedPath`].
///
/// Gradient paint servers are retained as typed geometry instead of being
/// sampled into raster pixels. PDF axial and radial shadings provide the
/// corresponding vector primitive for SVG and CSS Images gradients: SVG 2,
/// 13.2; CSS Images 3, 3.4 and 3.5; and ISO 32000-2:2020, 8.7.4.3.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedPathPaint {
    Solid(Color),
    Gradient(RenderedGradient),
    SvgPattern(RenderedSvgPathPattern),
}

impl RenderedPathPaint {
    fn solid_color(&self) -> Option<Color> {
        match self {
            Self::Solid(color) => Some(*color),
            Self::Gradient(_) | Self::SvgPattern(_) => None,
        }
    }
}

/// A vector SVG paint-server tile applied while its target path's local CTM
/// is active.
///
/// SVG 2 patterns repeat their children in the target element's user space.
/// Keeping this distinct from a CSS background SVG tile means PDF emission can
/// apply the path's CTM exactly once to both the geometry and the pattern:
/// <https://www.w3.org/TR/SVG2/pservers.html#Patterns>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedSvgPathPattern {
    pub(crate) tile_width: f32,
    pub(crate) tile_height: f32,
    pub(crate) origin: PaintPoint,
    pub(crate) transform: PaintTransform,
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) opacity: f32,
}

/// A normalized linear or radial gradient in the path's local paint space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedGradient {
    pub(crate) kind: RenderedGradientKind,
    /// The common component space used by every color stop.
    pub(crate) color_space: crate::css::ColorSpace,
    pub(crate) stops: Vec<RenderedGradientStop>,
    /// A single CSS repeating-gradient cycle evaluated by a PDF calculator
    /// function. SVG and finite CSS gradients leave this unset.
    pub(crate) periodic: Option<Box<RenderedPeriodicGradient>>,
    /// Maps the gradient's local coordinates to paint-space coordinates.
    ///
    /// This is SVG's `gradientTransform` for SVG gradients, and represents
    /// the affine ellipse transform for CSS radial gradients.
    pub(crate) transform: PaintTransform,
}

impl RenderedGradient {
    pub(crate) fn has_transparent_stop(&self) -> bool {
        self.periodic
            .as_ref()
            .map_or(&self.stops, |periodic| &periodic.stops)
            .iter()
            .any(|stop| !stop.color.is_opaque())
    }
}

/// One resolved CSS repeating-gradient cycle in paint-space units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedPeriodicGradient {
    pub(crate) stops: Vec<RenderedGradientStop>,
    pub(crate) start: f32,
    pub(crate) period: f32,
    pub(crate) domain_length: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RenderedGradientKind {
    Linear {
        start: PaintPoint,
        end: PaintPoint,
    },
    Radial {
        start_center: PaintPoint,
        start_radius: f32,
        end_center: PaintPoint,
        end_radius: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedGradientStop {
    pub(crate) offset: f32,
    pub(crate) color: Color,
    /// Exponent for the interval beginning at this stop. CSS transition hints
    /// map directly to PDF Type 2 exponential interpolation functions.
    /// CSS Images 3, 3.4.2 defines the exponent as `log_H(.5)`.
    pub(crate) interpolation_exponent: f32,
}

/// Stroke state for a vector path.
///
/// PDF's line cap, join, miter and dash graphics-state parameters correspond
/// directly to SVG's `stroke-*` properties: ISO 32000-1:2008, 8.4.3 and SVG
/// 2, 13.5.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenderedPathStrokeStyle {
    pub(crate) line_cap: RenderedPathLineCap,
    pub(crate) line_join: RenderedPathLineJoin,
    pub(crate) miter_limit: f32,
    pub(crate) dash_array: Vec<f32>,
    pub(crate) dash_offset: f32,
}

impl Default for RenderedPathStrokeStyle {
    fn default() -> Self {
        Self {
            line_cap: RenderedPathLineCap::Butt,
            line_join: RenderedPathLineJoin::Miter,
            miter_limit: 10.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedPathLineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderedPathLineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RenderedPathPaintOrder {
    #[default]
    FillThenStroke,
    StrokeThenFill,
}

#[allow(dead_code)]
impl RenderedPath {
    /// Return a conservative page-space bounding box for this path.
    ///
    /// SVG paths retain local commands plus a paint transform. Consumers that
    /// inspect rendered output therefore need transformed geometry rather than
    /// raw command coordinates. Bézier control points bound their curve, so
    /// this is conservative for curved segments.
    pub fn bounds(&self) -> Option<PaintRect> {
        let mut left = f32::INFINITY;
        let mut bottom = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut top = f32::NEG_INFINITY;
        let mut include = |point: PaintPoint| {
            let point = self.transform.apply_point(point);
            left = left.min(point.x);
            bottom = bottom.min(point.y);
            right = right.max(point.x);
            top = top.max(point.y);
        };
        for command in &self.commands {
            match command {
                RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                    include(*point);
                }
                RenderedPathCommand::CurveTo {
                    control_1,
                    control_2,
                    end,
                } => {
                    include(*control_1);
                    include(*control_2);
                    include(*end);
                }
                RenderedPathCommand::Close => {}
            }
        }
        (left.is_finite() && bottom.is_finite() && right.is_finite() && top.is_finite()).then(
            || {
                PaintRect::new(
                    PaintPoint::new(left, bottom),
                    PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
                )
            },
        )
    }

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
            transform: PaintTransform::identity(),
            commands,
            fill,
            fill_paint: fill.map(RenderedPathPaint::Solid),
            fill_rule,
            stroke,
            stroke_paint: stroke.map(RenderedPathPaint::Solid),
            stroke_width,
            stroke_style: RenderedPathStrokeStyle::default(),
            paint_order: RenderedPathPaintOrder::default(),
        }
    }

    pub(crate) fn with_paints(
        mut self,
        fill: Option<RenderedPathPaint>,
        stroke: Option<RenderedPathPaint>,
    ) -> Self {
        self.fill = fill.as_ref().and_then(RenderedPathPaint::solid_color);
        self.stroke = stroke.as_ref().and_then(RenderedPathPaint::solid_color);
        self.fill_paint = fill;
        self.stroke_paint = stroke;
        self
    }

    pub(crate) fn with_stroke_style(mut self, stroke_style: RenderedPathStrokeStyle) -> Self {
        self.stroke_style = stroke_style;
        self
    }

    pub(crate) fn with_paint_order(mut self, paint_order: RenderedPathPaintOrder) -> Self {
        self.paint_order = paint_order;
        self
    }

    pub(crate) fn with_transform(mut self, transform: PaintTransform) -> Self {
        self.transform = transform;
        self
    }

    /// Conservative paint-space bounds for the path's transformed geometry.
    ///
    /// This includes path control points, which is sufficient for paint-order
    /// inspection and replaced-element tests; PDF clipping remains represented
    /// independently by [`RenderedPathClip`].
    pub fn paint_bounds(&self) -> Option<PaintRect> {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut include = |point: PaintPoint| {
            let point = self.transform.apply_point(point);
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        };
        for command in &self.commands {
            match *command {
                RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                    include(point);
                }
                RenderedPathCommand::CurveTo {
                    control_1,
                    control_2,
                    end,
                } => {
                    include(control_1);
                    include(control_2);
                    include(end);
                }
                RenderedPathCommand::Close => {}
            }
        }
        (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()).then(
            || {
                PaintRect::new(
                    PaintPoint::new(min_x, min_y),
                    PaintSize::new(max_x - min_x, max_y - min_y),
                )
            },
        )
    }

    pub(in crate::document) fn translated(mut self, offset: PaintTranslation) -> Self {
        let transformed = self.transform != PaintTransform::identity();
        if transformed {
            self.transform = PaintTransform::translate(offset).multiply(self.transform);
        }
        for paint in [&mut self.fill_paint, &mut self.stroke_paint]
            .into_iter()
            .flatten()
        {
            if let RenderedPathPaint::Gradient(gradient) = paint {
                gradient.transform = PaintTransform::translate(offset).multiply(gradient.transform);
            }
        }
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
        if !transformed {
            for command in &mut self.commands {
                command.translate(offset);
            }
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

    pub(in crate::document) fn translate(&mut self, offset: PaintTranslation) {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => {
                *point = offset.transform_point(*point);
            }
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                *control_1 = offset.transform_point(*control_1);
                *control_2 = offset.transform_point(*control_2);
                *end = offset.transform_point(*end);
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
