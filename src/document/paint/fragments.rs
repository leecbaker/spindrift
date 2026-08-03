use crate::{css, document::Page};

use super::annotations::RenderedLink;
use super::display_list::{PaintBand, PaintBandList, PaintDisplayItem, PaintDisplayList};
use super::effects::{PaintClipPathEffect, PaintEffectScope, PaintEffects};
use super::geometry::{AxisSelectivePaintClip, PaintBounds, PaintClip, PaintTranslation};
use super::page::PaintPrimitive;
use super::paths::RenderedPathClip;
use super::shapes::RenderedRoundedRect;
use super::stacking::PaintStackingContext;

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

    /// Move a following BFC root's own decoration after the float phase.
    ///
    /// A BFC root that follows a float is an independent formatting context:
    /// its own background and border must remain above the preceding float's
    /// atomic paint.  A global block-before-float bucket would otherwise let
    /// that float repaint a shared edge of the later root.  The inline band is
    /// the first parent-level phase after floats while still preceding
    /// positioned descendants:
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn promote_background_border_after_floats(&mut self) {
        let background_items =
            std::mem::take(&mut self.display_list.bands.bands[PaintBand::BackgroundBorder.index()]);
        if background_items.is_empty() {
            return;
        }
        self.display_list.bands.bands[PaintBand::Inline.index()].extend(background_items);
    }

    /// Re-home an embedded document's page-canvas paint into its embedding
    /// replaced element's in-flow paint phase.
    ///
    /// A child browsing context's page background is local to that viewport;
    /// retaining it in the parent page-background band would place it beneath
    /// the embedding document's own canvas and make it disappear.
    /// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
    pub(crate) fn promote_page_background_to_in_flow_block(&mut self) {
        let page_background =
            std::mem::take(&mut self.display_list.bands.bands[PaintBand::PageBackground.index()]);
        if page_background.is_empty() {
            return;
        }
        let in_flow = &mut self.display_list.bands.bands[PaintBand::InFlowBlock.index()];
        in_flow.splice(0..0, page_background);
    }

    /// Remove positioned stacking contexts from an atomic formatting
    /// fragment so their parent can replay them in its own stacking level.
    ///
    /// Atomic inline layout measures descendants in a temporary local page,
    /// but CSS 2.2 Appendix E paints positioned descendants in the enclosing
    /// stacking context rather than inside the inline-level atomic paint unit.
    /// Keep ordinary fragment paint local while returning only the contexts
    /// that occupy the positioned paint bands.
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    pub(crate) fn take_positioned_stacking_contexts(&mut self) -> Vec<PaintStackingContext> {
        let mut contexts = Vec::new();
        for band in [
            PaintBand::NegativeZ,
            PaintBand::AutoZeroZ,
            PaintBand::PositiveZ,
        ] {
            let items = std::mem::take(&mut self.display_list.bands.bands[band.index()]);
            let retained = items
                .into_iter()
                .filter_map(|item| match item {
                    PaintDisplayItem::StackingContext(context) => {
                        contexts.push(context);
                        None
                    }
                    item => Some(item),
                })
                .collect();
            self.display_list.bands.bands[band.index()] = retained;
        }
        contexts
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.display_list.is_empty() && self.links.is_empty()
    }

    pub(crate) fn first_line_y(&self) -> Option<f32> {
        let mut first_line_y = None;
        self.for_each_flattened_primitive(&mut |primitive| {
            if first_line_y.is_none() {
                match primitive {
                    PaintPrimitive::Line(line)
                    | PaintPrimitive::OpaqueTextCoverage { line, .. } => {
                        first_line_y = Some(line.y());
                    }
                    _ => {}
                }
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

    /// Return this fragment without an overflow scope when recorded paint is
    /// wholly inside that scope. A clip that excludes no ink is not a CSS
    /// paint-order boundary; omitting it avoids PDF seam artifacts and lets
    /// ordinary paint bands remain contiguous.
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
    pub(crate) fn with_contents_effect_scoped_to_rect_if_needed(
        self,
        page: &Page,
        clip: PaintClip,
    ) -> Self {
        let mut bounds: Option<PaintBounds> = None;
        for band in PaintBand::ORDER {
            for item in &self.display_list.bands.bands[band.index()] {
                let Ok(item_bounds) = item.recorded_paint_bounds(page) else {
                    return self.with_contents_effect_scoped_to_rect(clip);
                };
                let Some(item_bounds) = item_bounds else {
                    continue;
                };
                match &mut bounds {
                    Some(bounds) => bounds.include_paint_rect(item_bounds.paint_rect()),
                    None => bounds = Some(PaintBounds::from_paint_rect(item_bounds.paint_rect())),
                }
            }
        }
        for link in &self.links {
            match &mut bounds {
                Some(bounds) => bounds.include_paint_rect(link.paint_rect()),
                None => bounds = Some(PaintBounds::from_paint_rect(link.paint_rect())),
            }
        }
        if bounds
            .map(PaintBounds::into_paint_clip)
            .is_some_and(|bounds| clip.contains(bounds))
        {
            self
        } else {
            self.with_contents_effect_scoped_to_rect(clip)
        }
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
        self.with_contents_effect_scoped_to_clip(clip, None, PaintClipPathEffect::None, false, None)
    }

    /// Scope non-decoration contents with an axis-selective CSS overflow
    /// clip. The visible axis stays unbounded until PDF serialization.
    pub(crate) fn with_contents_effect_scoped_to_axis_selective_rect(
        self,
        clip: AxisSelectivePaintClip,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip(
            clip.bounds(),
            None,
            PaintClipPathEffect::None,
            false,
            Some(clip),
        )
    }

    /// Clip only recorded ink that can reach the overflow edge.
    ///
    /// A rectangular PDF clip antialiases primitives that coincide with its
    /// edge even when their complete ink bounds lie inside it. CSS overflow
    /// does not introduce such a raster edge for descendants that cannot
    /// overflow, so keep those recorded operations in their original paint
    /// band and scope only the intervening runs that need clipping. The runs
    /// preserve Appendix E order around the scoped descendants.
    ///
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    pub(crate) fn with_contents_effect_scoped_to_rect_preserving_contained_ink(
        self,
        page: &Page,
        clip: PaintClip,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip_with_recorded_ink(
            page,
            clip,
            None,
            PaintClipPathEffect::None,
        )
    }

    /// Scope paint-contained contents while preserving the edge coverage of a
    /// primitive that is already wholly inside the containment rectangle.
    pub(crate) fn with_paint_containment_contents_effect_scoped_to_rect(
        self,
        clip: PaintClip,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip(clip, None, PaintClipPathEffect::None, true, None)
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

    /// Insert positioned descendants, omitting the overflow scope only when
    /// the complete recorded subtree is already within it. Positioned
    /// stacking contexts are part of the clipped descendant paint, so their
    /// bounds must participate in this decision as well.
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
    pub(crate) fn with_contents_effect_scoped_to_rect_and_child_contexts_if_needed(
        mut self,
        page: &Page,
        clip: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        for context in child_contexts {
            self.display_list.bands.push_context(context);
        }
        self.display_list.bands.sort_stacking_contexts();
        self.with_contents_effect_scoped_to_rect_if_needed(page, clip)
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
        self.with_contents_effect_scoped_to_clip(
            clip,
            Some(rounded_clip),
            PaintClipPathEffect::None,
            false,
            None,
        )
    }

    /// Clip non-decoration contents to an exact resolved shape contour.
    pub(crate) fn with_contents_effect_scoped_to_path(
        self,
        clip: PaintClip,
        path: RenderedPathClip,
    ) -> Self {
        self.with_contents_effect_scoped_to_clip(
            clip,
            None,
            PaintClipPathEffect::Path(path),
            false,
            None,
        )
    }

    fn with_contents_effect_scoped_to_clip(
        self,
        clip: PaintClip,
        rounded_clip: Option<RenderedRoundedRect>,
        shape_clip: PaintClipPathEffect,
        elide_covered_rectangular_clip: bool,
        axis_selective_clip: Option<AxisSelectivePaintClip>,
    ) -> Self {
        let mut bands = self.display_list.bands;
        let effects = PaintEffects {
            overflow_clip: axis_selective_clip.is_none().then_some(clip),
            axis_selective_overflow_clip: axis_selective_clip,
            rounded_overflow_clip: rounded_clip,
            clip_path: shape_clip,
            ..PaintEffects::default()
        };
        let mut emitted_scope = false;

        for band in PaintBand::ORDER {
            if matches!(
                band,
                PaintBand::BackgroundBorder
                    | PaintBand::TableCellBorder
                    | PaintBand::TableCollapsedBorder
                    | PaintBand::Outline
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
                    PaintEffectScope::new(effects.clone(), Some(clip), overflowing),
                );
                emitted_scope = true;
            }
        }

        let content_links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                if axis_selective_clip.is_some() {
                    return Some(PaintDisplayItem::Link(link));
                }
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(PaintDisplayItem::Link(link))
            })
            .collect::<Vec<_>>();
        if !content_links.is_empty() {
            bands.push_effect_scope_in_band(
                PaintBand::Inline,
                PaintEffectScope::new(effects.clone(), Some(clip), content_links),
            );
            emitted_scope = true;
        }

        // Paint containment establishes a clipping boundary even when size
        // containment leaves its padding box empty. Retain that zero-area
        // scope instead of erasing the semantic effect while pruning the
        // descendants it clips.
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        if elide_covered_rectangular_clip
            && !emitted_scope
            && (clip.width() <= 0.0 || clip.height() <= 0.0)
        {
            bands.push_effect_scope_in_band(
                PaintBand::InFlowBlock,
                PaintEffectScope::new(effects, Some(clip), Vec::new()),
            );
        }

        Self {
            display_list: PaintDisplayList { bands },
            links: Vec::new(),
        }
    }

    /// The recorded-operation variant of
    /// [`Self::with_contents_effect_scoped_to_clip`]. Unlike the primitive
    /// fast path above, this can establish bounds for the deferred operations
    /// in a page paint tree without materializing or reordering them.
    fn with_contents_effect_scoped_to_clip_with_recorded_ink(
        self,
        _page: &Page,
        clip: PaintClip,
        rounded_clip: Option<RenderedRoundedRect>,
        shape_clip: PaintClipPathEffect,
    ) -> Self {
        let mut bands = self.display_list.bands;
        let effects = PaintEffects {
            overflow_clip: Some(clip),
            rounded_overflow_clip: rounded_clip,
            clip_path: shape_clip,
            ..PaintEffects::default()
        };
        let mut emitted_scope = false;

        for band in PaintBand::ORDER {
            if matches!(
                band,
                PaintBand::BackgroundBorder
                    | PaintBand::TableCellBorder
                    | PaintBand::TableCollapsedBorder
                    | PaintBand::Outline
            ) {
                continue;
            }
            let items = std::mem::take(&mut bands.bands[band.index()]);
            if !items.is_empty() {
                bands.bands[band.index()].push(PaintDisplayItem::EffectScope(
                    PaintEffectScope::new(effects.clone(), Some(clip), items),
                ));
                emitted_scope = true;
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
                PaintEffectScope::new(effects.clone(), Some(clip), content_links),
            );
            emitted_scope = true;
        }

        if !emitted_scope && (clip.width() <= 0.0 || clip.height() <= 0.0) {
            bands.push_effect_scope_in_band(
                PaintBand::InFlowBlock,
                PaintEffectScope::new(effects, Some(clip), Vec::new()),
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
            self.display_list.bands.push_effect_scope_in_band(
                band,
                PaintEffectScope::new(effects.clone(), Some(clip), items),
            );
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
                        | PaintBand::TableCellBorder
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

    /// Compatibility wrapper for callers whose fragmentainer block axis is
    /// known to be physical y. New logical-axis-aware replay should use
    /// [`Self::with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow`].
    #[allow(dead_code)]
    pub(crate) fn with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
        self,
        block_clip: PaintClip,
        clip_crossing_ink: bool,
    ) -> Self {
        self.with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
            css::PhysicalAxis::Vertical,
            block_clip,
            clip_crossing_ink,
        )
    }

    /// Clip a captured fragment along one physical fragmentation axis while
    /// retaining authored overflow in the perpendicular axis.
    ///
    /// CSS Fragmentation chooses a logical block range.  Horizontal flows map
    /// that range to physical y, while vertical flows map it to physical x;
    /// callers must therefore provide the resolved physical axis rather than
    /// treating every fragmentainer as a vertical page strip.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(crate) fn with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
        self,
        block_axis: css::PhysicalAxis,
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
        let clip = match block_axis {
            css::PhysicalAxis::Vertical => PaintClip::new(
                bounds.x() - inline_guard,
                block_clip.y(),
                bounds.width() + inline_guard * 2.0,
                block_clip.height(),
            ),
            css::PhysicalAxis::Horizontal => PaintClip::new(
                block_clip.x(),
                bounds.y() - inline_guard,
                block_clip.width(),
                bounds.height() + inline_guard * 2.0,
            ),
        };
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
                    PaintEffectScope::new(effects.clone(), Some(clip), items),
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
        decoration_bands.bands[PaintBand::TableCellBorder.index()] =
            std::mem::take(&mut content_bands.bands[PaintBand::TableCellBorder.index()]);
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::PaintFragment;
    use crate::CssColor;
    use crate::document::paint::annotations::RenderedLink;
    use crate::document::paint::geometry::{
        PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth, PaintTranslation,
    };
    use crate::document::paint::page::PaintPrimitive;
    use crate::document::paint::paths::{
        RenderedPath, RenderedPathClip, RenderedPathCommand, RenderedPathFillRule,
    };
    use crate::document::paint::patterns::{PaintPatternTiling, RenderedImagePattern};
    use crate::document::paint::shapes::RenderedRect;
    use crate::document::paint::text::{
        RenderedGlyph, RenderedGlyphKind, RenderedGlyphs, RenderedLine, RenderedTextMatrix,
        RenderedTextRun,
    };

    #[test]
    fn translated_paint_fragment_preserves_shared_rendered_glyph_storage() {
        let glyphs: RenderedGlyphs = vec![RenderedGlyph {
            kind: RenderedGlyphKind::Paint(42),
            x_advance: 7.0,
            nominal_x_advance: 7.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: "A".to_string(),
        }]
        .into();
        let line = RenderedLine::from_paint_origin(
            "A".to_string(),
            PaintPoint::new(10.0, 20.0),
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![RenderedTextRun {
                text: Rc::from("A"),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                glyphs: Some(glyphs.clone()),
            }],
        );
        let fragment = PaintFragment::from_primitives(vec![PaintPrimitive::Line(line)], Vec::new());

        let translated = fragment.translated(PaintTranslation::new(3.0, -4.0));
        let primitives = translated.flattened_primitives();
        let PaintPrimitive::Line(translated_line) = &primitives[0] else {
            panic!("expected translated line primitive");
        };

        assert_eq!(translated_line.origin(), PaintPoint::new(13.0, 16.0));
        assert!(glyphs.ptr_eq(translated_line.runs[0].glyphs.as_ref().unwrap()));
    }

    #[test]
    fn paint_fragment_translation_moves_paths_patterns_and_links_together() {
        let path = RenderedPath::new(
            vec![RenderedPathCommand::move_to(PaintPoint::new(1.0, 2.0))],
            Some(CssColor::BLACK),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            Some(RenderedPathClip::new(
                vec![RenderedPathCommand::line_to(PaintPoint::new(3.0, 4.0))],
                RenderedPathFillRule::NonZero,
                Vec::new(),
            )),
        );
        let pattern = RenderedImagePattern::from_paint_rect(
            PaintRect::new(PaintPoint::new(5.0, 6.0), PaintSize::new(7.0, 8.0)),
            true,
            PaintPatternTiling::new(
                PaintSize::new(7.0, 8.0),
                PaintSize::new(7.0, 8.0),
                PaintPoint::new(9.0, 10.0),
            ),
            1,
            1,
            true,
            Rc::<[u8]>::from(vec![0, 0, 0]),
            None,
        );
        let link = RenderedLink::from_paint_rect(
            PaintRect::new(PaintPoint::new(11.0, 12.0), PaintSize::new(13.0, 14.0)),
            "https://example.com",
        );
        let translated = PaintFragment::from_primitives(
            vec![
                PaintPrimitive::Path(path),
                PaintPrimitive::ImagePattern(pattern),
            ],
            vec![link],
        )
        .translated(PaintTranslation::new(20.0, -30.0));

        let primitives = translated.flattened_primitives();
        let [
            PaintPrimitive::Path(path),
            PaintPrimitive::ImagePattern(pattern),
        ] = primitives.as_slice()
        else {
            panic!("expected translated path and image pattern");
        };
        assert_eq!(
            path.commands,
            vec![RenderedPathCommand::move_to(PaintPoint::new(21.0, -28.0))]
        );
        assert_eq!(
            path.clip.as_ref().unwrap().commands,
            vec![RenderedPathCommand::line_to(PaintPoint::new(23.0, -26.0))]
        );
        assert_eq!(
            pattern.paint_rect(),
            PaintRect::new(PaintPoint::new(25.0, -24.0), PaintSize::new(7.0, 8.0))
        );
        assert_eq!(pattern.tiling.origin, PaintPoint::new(29.0, -20.0));
        assert_eq!(pattern.tiling.tile_size, PaintSize::new(7.0, 8.0));
        assert_eq!(pattern.tiling.step, PaintSize::new(7.0, 8.0));
        assert_eq!(
            translated.links[0].paint_rect(),
            PaintRect::new(PaintPoint::new(31.0, -18.0), PaintSize::new(13.0, 14.0))
        );
    }

    #[test]
    fn fragmentainer_slice_keeps_monolithic_paint_whole_at_its_block_start() {
        let bounds = PaintClip::new(0.0, 0.0, 100.0, 100.0);
        let fragment = PaintFragment::from_primitives(
            vec![PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                bounds.paint_rect(),
                Some(CssColor::BLACK),
            ))],
            Vec::new(),
        )
        .with_monolithic_fragmentation_scope(bounds);

        let sliced = fragment
            .with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
                PaintClip::new(0.0, 80.0, 100.0, 20.0),
                true,
            );

        let primitives = sliced.flattened_primitives();
        let [PaintPrimitive::Rect(rect)] = primitives.as_slice() else {
            panic!("expected one retained monolithic rectangle");
        };
        assert_eq!(rect.paint_rect(), bounds.paint_rect());
    }

    #[test]
    fn later_fragmentainer_slice_does_not_replay_monolithic_paint() {
        let bounds = PaintClip::new(0.0, 0.0, 100.0, 100.0);
        let fragment = PaintFragment::from_primitives(
            vec![PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                bounds.paint_rect(),
                Some(CssColor::BLACK),
            ))],
            Vec::new(),
        )
        .with_monolithic_fragmentation_scope(bounds);

        let sliced = fragment
            .with_primitives_clipped_to_physical_block_range_preserving_inline_overflow(
                PaintClip::new(0.0, 60.0, 100.0, 20.0),
                true,
            );

        assert!(sliced.flattened_primitives().is_empty());
    }
}
