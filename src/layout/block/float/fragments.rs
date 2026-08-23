use super::super::super::*;
use super::model::*;

impl<'a> LayoutBuilder<'a> {
    /// Captures one float's complete page-local paint subtree for replay in
    /// the parent float paint band.
    ///
    /// The captured [`PaintFragment`] retains every paint primitive emitted by
    /// the floated box's decoration, including vector SVG [`PaintPrimitive::Path`]
    /// and tiled [`PaintPrimitive::SvgPattern`] background layers from a
    /// tree-abiding generated pseudo-element.  Re-parenting the existing
    /// display-list nodes, rather than reconstructing a subset of decoration
    /// paint, preserves CSS background clips, transforms, source order, and
    /// the float's ownership across fragmentation.
    ///
    /// <https://www.w3.org/TR/css-backgrounds-3/#layering>
    /// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    /// <https://www.w3.org/TR/css-break-3/>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn build_float_paint_fragment(
        &mut self,
        id: FloatId,
        specified_side: Float,
        page_index: usize,
        side: UsedFloatSide,
        source_order: usize,
        placement: LogicalFloatPlacement,
        outer_inline_extent: MarginBoxLength,
        fallback_bounds: PaintClip,
        style: &ComputedStyle,
        replaced_content_is_clipped: bool,
        fragment: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
        is_fragmented_float: bool,
    ) -> Option<FloatPaintFragment> {
        if fragment.is_empty() && child_contexts.is_empty() {
            return None;
        }

        let child_bounds = union_paint_bounds(child_contexts.iter().filter_map(|context| {
            context.bounds.map(|bounds| {
                context
                    .effects
                    .transform
                    .map(|transform| transform.apply_clip_to_aabb(bounds))
                    .unwrap_or(bounds)
            })
        }));
        let bounds = fragment
            .bounds()
            .map(|bounds| union_paint_bounds(child_bounds.into_iter().chain([bounds])).unwrap())
            .or(child_bounds)
            .unwrap_or(fallback_bounds);
        let mut policy = StackingContextPolicy::for_atomic(style, PaintBand::Float, bounds);
        // The replayed float root is laid out through the ordinary principal
        // effect path, which owns its exact used border-box transform. This
        // outer float wrapper owns float ordering and exclusion only; applying
        // the same CTM here would scale/rotate the subtree twice.
        // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
        if style.has_transform() {
            policy.effects.transform = None;
            policy.effects.suppress_paint = false;
        }
        if replaced_content_is_clipped {
            // CSS overflow clips a box's contents rather than its background
            // and border. Raster replaced-element painting already clips its
            // concrete object to the content box, so a float-wide effect
            // scope would otherwise apply a second PDF clip around decoration.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            policy.effects.clear_overflow_clip_effects();
        }
        let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
            .with_source_order(source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
        // An unsplit float's exclusion is defined by its used margin box, not by the
        // union of the paint primitives that happened to be emitted while
        // replaying the float. A child-only paint fragment can omit the
        // float's border or padding, which would otherwise make a following
        // float start one border-width earlier than its sibling. A fragmented
        // float, conversely, needs the page-local paint bounds for each
        // continuation rather than its full source margin box.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        let rect = if is_fragmented_float {
            PageTopRect::new(
                placement.margin_box.x(),
                bounds.y() + bounds.height(),
                placement.margin_box.width(),
                bounds.height(),
            )
        } else {
            placement.margin_box
        };
        Some(FloatPaintFragment {
            id,
            specified_side,
            page_index,
            side,
            rect,
            outer_inline_extent,
            placement: placement
                .with_margin_box(placement.containing, rect)
                .on_page(page_index),
            area: FloatArea::RECT,
            source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            context,
        })
    }

    pub(in crate::layout) fn push_float_fragment_shape(
        &mut self,
        fragment: &FloatPaintFragment,
        run: &mut FloatRunState,
    ) {
        self.push_float_shape(FloatShape::from_fragment(fragment), run);
    }

    pub(in crate::layout) fn push_float_shape(
        &mut self,
        shape: FloatShape,
        run: &mut FloatRunState,
    ) {
        self.float_contexts
            .last_mut()
            .expect("root float context exists")
            .shapes
            .push(shape.clone());
        run.include_shape(shape);
    }

    /// Materialize speculative paint and page-local effects when normal flow
    /// reaches their target fragmentainer.
    ///
    /// Both fragmented floats and overflowing fixed-size boxes can paint in a
    /// later fragmentainer without advancing their surrounding normal flow.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn apply_pending_fragments_for_current_page(&mut self) {
        let page_index = self.pages.len();
        let mut pending_effects = Vec::new();
        let mut ready_effects = Vec::new();
        for effects in std::mem::take(&mut self.pending_page_side_effects) {
            match effects.page_index.cmp(&page_index) {
                std::cmp::Ordering::Less => {
                    self.apply_pending_page_side_effects_to_materialized_page(effects)
                }
                std::cmp::Ordering::Equal => ready_effects.push(effects),
                std::cmp::Ordering::Greater => pending_effects.push(effects),
            }
        }
        self.pending_page_side_effects = pending_effects;
        for effects in ready_effects {
            self.apply_pending_page_side_effects(effects);
        }

        let mut pending = Vec::new();
        let mut ready = Vec::new();
        for fragment in std::mem::take(&mut self.pending_paint_fragments) {
            match fragment.page_index.cmp(&page_index) {
                std::cmp::Ordering::Less => {
                    self.append_pending_paint_fragment_to_materialized_page(fragment)
                }
                std::cmp::Ordering::Equal => ready.push(fragment.fragment),
                std::cmp::Ordering::Greater => pending.push(fragment),
            }
        }
        self.pending_paint_fragments = pending;
        let had_ready_fragments = !ready.is_empty();
        for fragment in ready {
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
        if had_ready_fragments {
            self.current_page.sort_paint_tree_stacking_contexts();
            // Deferred overflow/float paint can be the only content in this
            // destination page. Keep the page materialized when finalization
            // reaches it, even though ordinary normal-flow layout never did.
            // <https://www.w3.org/TR/css-break-3/#monolithic>
            self.current_page_has_flow_content = true;
            self.current_page.mark_fragmentation_content();
        }
    }

    pub(in crate::layout) fn apply_pending_page_side_effects(
        &mut self,
        effects: PendingPageSideEffects,
    ) {
        merge_named_assignments(&mut self.current_page_named_strings, effects.named_strings);
        merge_named_assignments(
            &mut self.current_page_running_elements,
            effects.running_elements,
        );
        self.current_page.links.extend(effects.links);
    }

    /// Delivers deferred page-local effects after ordinary flow has already
    /// finalized their target page.
    ///
    /// A fragmented float is laid out in an isolated replay. Its following
    /// in-flow siblings may advance the document past a replayed fragment
    /// before the replay's named-string or running-element effects are
    /// committed. Those effects belong to the historical target page rather
    /// than to a newly synthesized later page.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>
    fn apply_pending_page_side_effects_to_materialized_page(
        &mut self,
        effects: PendingPageSideEffects,
    ) {
        let page_index = effects.page_index;
        let named_strings = self
            .page_named_strings
            .get_mut(page_index)
            .expect("materialized page has named-string storage");
        merge_named_assignments(named_strings, effects.named_strings);
        let running_elements = self
            .page_running_elements
            .get_mut(page_index)
            .expect("materialized page has running-element storage");
        merge_named_assignments(running_elements, effects.running_elements);
        self.pages
            .get_mut(page_index)
            .expect("materialized page exists for deferred side effects")
            .links
            .extend(effects.links);
    }

    /// Delivers deferred paint to an already finalized page.
    ///
    /// See [`Self::apply_pending_page_side_effects_to_materialized_page`] for
    /// why an isolated replay may finish after normal flow passed its target.
    fn append_pending_paint_fragment_to_materialized_page(
        &mut self,
        pending: PendingPaintFragment,
    ) {
        let page = self
            .pages
            .get_mut(pending.page_index)
            .expect("materialized page exists for deferred paint");
        page.append_paint_fragment_owned(pending.fragment, PaintTranslation::identity());
        page.sort_paint_tree_stacking_contexts();
        page.mark_fragmentation_content();
    }

    pub(in crate::layout) fn apply_deferred_layout_side_effects(
        &mut self,
        effects: DeferredLayoutSideEffects,
    ) {
        self.bookmarks.extend(effects.bookmarks);
        for (target, page_index) in effects.anchors {
            self.page_anchors.entry(target).or_insert(page_index);
        }
        for (target, source_position) in effects.anchor_source_positions {
            self.page_anchor_source_positions
                .entry(target)
                .or_insert(source_position);
        }
        for (target, text) in effects.anchor_text {
            self.page_anchor_text.entry(target).or_insert(text);
        }
        for (target, counters) in effects.anchor_counters {
            self.page_anchor_counters.entry(target).or_insert(counters);
        }
        let current_page_index = self.pages.len();
        for page_effects in effects.page_effects {
            if page_effects.page_index == current_page_index {
                self.apply_pending_page_side_effects(page_effects);
            } else {
                self.pending_page_side_effects.push(page_effects);
            }
        }
    }

    pub(in crate::layout) fn deferred_layout_side_effects_since(
        &self,
        snapshot: &LayoutSnapshot,
    ) -> DeferredLayoutSideEffects {
        let mut effects = DeferredLayoutSideEffects {
            bookmarks: self
                .bookmarks
                .iter()
                .skip(snapshot.bookmark_count())
                .cloned()
                .collect(),
            anchors: self
                .page_anchors
                .iter()
                .filter(|(target, _)| !snapshot.has_page_anchor_source_position(target))
                .map(|(target, page_index)| (target.clone(), *page_index))
                .collect(),
            anchor_source_positions: self
                .page_anchor_source_positions
                .iter()
                .filter(|(target, _)| !snapshot.has_page_anchor_text(target))
                .map(|(target, position)| (target.clone(), *position))
                .collect(),
            anchor_text: self
                .page_anchor_text
                .iter()
                .filter(|(target, _)| !snapshot.has_page_anchor_counters(target))
                .map(|(target, text)| (target.clone(), text.clone()))
                .collect(),
            anchor_counters: self
                .page_anchor_counters
                .iter()
                .filter(|(target, _)| !snapshot.has_page_anchor(target))
                .map(|(target, counters)| (target.clone(), counters.clone()))
                .collect(),
            page_effects: Vec::new(),
        };

        let first_deferred_page = snapshot.page_count();
        let captured_page_count = self
            .page_named_strings
            .len()
            .max(self.page_running_elements.len())
            .max(self.pages.len());
        for page_index in first_deferred_page..captured_page_count {
            let empty_named = HashMap::new();
            let empty_running = HashMap::new();
            let empty_links = Vec::new();
            let base_named = if page_index == first_deferred_page {
                snapshot.current_page_named_strings()
            } else {
                &empty_named
            };
            let base_running = if page_index == first_deferred_page {
                snapshot.current_page_running_elements()
            } else {
                &empty_running
            };
            let base_links = if page_index == first_deferred_page {
                snapshot.current_page_links()
            } else {
                &empty_links
            };
            let named_strings = self
                .page_named_strings
                .get(page_index)
                .map(|assignments| named_assignment_delta(base_named, assignments))
                .unwrap_or_default();
            let running_elements = self
                .page_running_elements
                .get(page_index)
                .map(|assignments| named_assignment_delta(base_running, assignments))
                .unwrap_or_default();
            let links = self
                .pages
                .get(page_index)
                .map(|page| {
                    if base_links.len() < page.links.len() {
                        page.links[base_links.len()..].to_vec()
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            if !named_strings.is_empty() || !running_elements.is_empty() || !links.is_empty() {
                effects.page_effects.push(PendingPageSideEffects {
                    page_index,
                    named_strings,
                    running_elements,
                    links,
                });
            }
        }

        let current_base_named;
        let current_base_running;
        let current_base_links;
        let (base_named, base_running, base_links) = if self.pages.len() == snapshot.page_count() {
            (
                snapshot.current_page_named_strings(),
                snapshot.current_page_running_elements(),
                snapshot.current_page_links(),
            )
        } else {
            current_base_named = HashMap::new();
            current_base_running = HashMap::new();
            current_base_links = Vec::new();
            (
                &current_base_named,
                &current_base_running,
                &current_base_links,
            )
        };
        let current_named = named_assignment_delta(base_named, &self.current_page_named_strings);
        let current_running =
            named_assignment_delta(base_running, &self.current_page_running_elements);
        let current_links = if base_links.len() < self.current_page.links.len() {
            self.current_page.links[base_links.len()..].to_vec()
        } else {
            Vec::new()
        };
        if !current_named.is_empty() || !current_running.is_empty() || !current_links.is_empty() {
            effects.page_effects.push(PendingPageSideEffects {
                page_index: self.pages.len(),
                named_strings: current_named,
                running_elements: current_running,
                links: current_links,
            });
        }

        effects
    }
}

pub(in crate::layout) fn named_assignment_delta(
    before: &HashMap<String, Vec<NamedStringAssignment>>,
    after: &HashMap<String, Vec<NamedStringAssignment>>,
) -> HashMap<String, Vec<NamedStringAssignment>> {
    let mut delta = HashMap::new();
    for (name, assignments) in after {
        let before_len = before.get(name).map(Vec::len).unwrap_or(0);
        if before_len < assignments.len() {
            delta.insert(name.clone(), assignments[before_len..].to_vec());
        }
    }
    delta
}

pub(in crate::layout) fn merge_named_assignments(
    target: &mut HashMap<String, Vec<NamedStringAssignment>>,
    source: HashMap<String, Vec<NamedStringAssignment>>,
) {
    for (name, mut assignments) in source {
        target.entry(name).or_default().append(&mut assignments);
    }
}

pub(in crate::layout) fn union_paint_bounds(
    bounds: impl IntoIterator<Item = PaintClip>,
) -> Option<PaintClip> {
    bounds.into_iter().fold(None, |acc, bounds| {
        Some(match acc {
            Some(existing) => union_paint_clip(existing, bounds),
            None => bounds,
        })
    })
}

pub(in crate::layout) fn union_paint_clip(left: PaintClip, right: PaintClip) -> PaintClip {
    let x1 = left.x().min(right.x());
    let x2 = (left.x() + left.width()).max(right.x() + right.width());
    let y1 = left.y().min(right.y());
    let y2 = (left.y() + left.height()).max(right.y() + right.height());
    PaintClip::from_paint_rect(paint_space_rect(x1, y1, x2 - x1, y2 - y1))
}
