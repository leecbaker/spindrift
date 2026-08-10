use super::*;
use crate::layout::assets::{
    BorderPaint, ResolvedBackgroundImagePaint, has_opaque_square_normal_border,
};

/// One visible destination slice of a continuously decorated fragmented box.
///
/// `source_border_rect` is the border box of the unfragmented (for
/// `box-decoration-break: slice`) box. `destination_border_rect` is solely
/// the visible fragment's paint and clipping geometry.  The owner supplies
/// the already-projected translation between those coordinate spaces, which
/// keeps physical page coordinates out of the fragmentation contract.
///
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
/// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FragmentedDecorationSlice {
    source_border_rect: PaintRect,
    destination_border_rect: PaintRect,
    source_to_destination: PaintTranslation,
    owns_block_start: bool,
    owns_block_end: bool,
}

impl FragmentedDecorationSlice {
    pub(in crate::layout) fn new(
        source_border_rect: PaintRect,
        destination_border_rect: PaintRect,
        source_to_destination: PaintTranslation,
        owns_block_start: bool,
        owns_block_end: bool,
    ) -> Self {
        Self {
            source_border_rect,
            destination_border_rect,
            source_to_destination,
            owns_block_start,
            owns_block_end,
        }
    }

    /// The area used to resolve `background-position` and `background-size`.
    /// The destination rectangle deliberately remains a clip: it never
    /// becomes the positioning area for a sliced decoration.
    pub(in crate::layout) fn positioning_border_rect(
        self,
        decoration_break: css::BoxDecorationBreak,
    ) -> PaintRect {
        if decoration_break == css::BoxDecorationBreak::Clone {
            self.destination_border_rect
        } else {
            self.source_to_destination
                .transform_rect(&self.source_border_rect)
        }
    }

    pub(in crate::layout) fn owns_block_start(self) -> bool {
        self.owns_block_start
    }

    pub(in crate::layout) fn owns_block_end(self) -> bool {
        self.owns_block_end
    }
}

/// Builds CSS outline paint primitives for a box border area.
///
/// CSS UI paints outlines outside the border edge and excludes them from box
/// sizing. The renderer models this as a synthetic border expanded by the
/// used outline offset and outline width, then emits it in CSS 2.2 Appendix E's
/// final outline paint band:
/// <https://www.w3.org/TR/css-ui-3/#outline-props> and
/// <https://www.w3.org/TR/CSS22/zindex.html>.
pub(in crate::layout) fn outline_primitives_for_border_rect(
    border_rect: PaintRect,
    style: &ComputedStyle,
) -> Vec<PaintPrimitive> {
    if style.outline_width <= 0.0 || style.outline_style.suppresses_used_width() {
        return Vec::new();
    }
    if border_rect.size.width <= 0.0 || border_rect.size.height <= 0.0 {
        return Vec::new();
    }

    if let Some(paths) = border_shape_outline_paths(border_rect, style) {
        return paths.into_iter().map(PaintPrimitive::Path).collect();
    }

    let mut outline_style = style.clone();
    outline_style.background.background_color = css::BackgroundColor::TRANSPARENT;
    outline_style.background.background_image = css::ComputedImage::None;
    outline_style.background.background_layers.clear();
    outline_style.border_image = css::BorderImage::initial();
    // CSS outlines are their own final paint step. Reusing the decoration
    // helper must not replay the element's box shadows around the synthetic
    // outline border.
    // <https://www.w3.org/TR/css-ui-3/#outline-props>
    outline_style.box_shadow.clear();
    outline_style.border_width = style.outline_width;
    outline_style.border_widths = css::Edges {
        top: style.outline_width,
        right: style.outline_width,
        bottom: style.outline_width,
        left: style.outline_width,
    };
    outline_style.border_width_values = css::CssEdges::all(
        css::ComputedLengthPercentage::from_points(style.outline_width),
    );
    outline_style.border_color = style.outline_color;
    outline_style.border_colors = css::BorderColors {
        top: style.outline_color,
        right: style.outline_color,
        bottom: style.outline_color,
        left: style.outline_color,
    };
    outline_style.border_styles = css::BorderStyles {
        top: style.outline_style,
        right: style.outline_style,
        bottom: style.outline_style,
        left: style.outline_style,
    };

    let outset = style.outline_offset.length_points() + style.outline_width;
    let (rects, rounded_rects, paths, strokes) = block_paint_ops(
        expanded_outline_paint_rect(border_rect, outset),
        &outline_style,
    );
    let mut primitives = Vec::new();
    primitives.extend(rects.into_iter().map(PaintPrimitive::Rect));
    primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
    primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
    primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
    primitives
}

fn expanded_outline_paint_rect(border_rect: PaintRect, outset: f32) -> PaintRect {
    paint_space_rect(
        border_rect.origin.x - outset,
        border_rect.origin.y - outset,
        border_rect.size.width + outset * 2.0,
        border_rect.size.height + outset * 2.0,
    )
}

impl<'a> LayoutBuilder<'a> {
    /// Collect positioned descendants emitted after `start_index` for one page.
    ///
    /// Real stacking contexts capture their positioned descendants into the
    /// scoped paint tree, while pseudo contexts such as inline-block painting
    /// can intentionally let those descendants escape:
    /// <https://www.w3.org/TR/css-position-3/#painting-order> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(super) fn positioned_child_contexts_since(
        &mut self,
        start_index: usize,
        page_index: usize,
        policy: &StackingContextPolicy,
    ) -> Vec<PaintStackingContext> {
        if !policy.captures_positioned_descendants || start_index >= self.positioned_layers.len() {
            return Vec::new();
        }
        self.positioned_layers
            .split_off(start_index)
            .into_iter()
            .filter(|layer| layer.page_index == page_index)
            .map(|layer| layer.context.with_links(layer.links))
            .collect()
    }

    /// Replace current-page paint emitted since `checkpoint` with one scoped
    /// paint-tree context in the requested parent band.
    ///
    /// CSS 2.2 Appendix E paints formatting units through ordered bands inside
    /// stacking contexts. This helper preserves the captured fragment's local
    /// bands while making the unit durable in the page paint tree:
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(super) fn scope_current_page_paint_since(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        bounds: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
        effects: PaintEffects,
    ) -> bool {
        // A formatting context can have emitted only primitive operations so
        // far; in that case no retained page paint tree exists yet. Capture
        // through the durable checkpoint rather than requiring a tree, so an
        // atomic effect (notably a containment clip) owns both primitive-only
        // and already-retained descendants.
        let fragment = self
            .current_page
            .take_paint_fragment_since(checkpoint.clone());
        if fragment.is_empty() && child_contexts.is_empty() {
            return false;
        }
        let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(bounds);
        self.current_page
            .replace_paint_tree_since_with_context(checkpoint, band, context);
        true
    }

    /// Replace current-page paint emitted since `checkpoint` using a full
    /// stacking-context policy.
    ///
    /// Flex items and table fragments derive their parent band, stack level,
    /// captured descendant behavior, and effects from `StackingContextPolicy`;
    /// using this helper keeps those policy decisions applied consistently:
    /// <https://www.w3.org/TR/css-position-3/#painting-order> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(super) fn scope_current_page_paint_since_with_policy(
        &mut self,
        checkpoint: &PaintCheckpoint,
        policy: StackingContextPolicy,
        bounds: PaintClip,
        child_contexts: Vec<PaintStackingContext>,
    ) -> bool {
        let fragment = self
            .current_page
            .take_paint_fragment_since(checkpoint.clone());
        self.scope_current_page_fragment_with_policy(
            checkpoint,
            policy,
            bounds,
            fragment,
            child_contexts,
        )
    }

    /// Replace current-page paint since `checkpoint` with a prepared fragment
    /// wrapped according to a stacking-context policy.
    ///
    /// Table fragmentation may need to augment the captured fragment with
    /// collapsed borders before it can be scoped, while flex item fragments can
    /// scope the raw checkpoint capture. Both still use the same CSS stacking
    /// policy fields when replacing page paint:
    /// <https://www.w3.org/TR/css-position-3/#painting-order> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(super) fn scope_current_page_fragment_with_policy(
        &mut self,
        checkpoint: &PaintCheckpoint,
        policy: StackingContextPolicy,
        bounds: PaintClip,
        fragment: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> bool {
        if fragment.is_empty() && child_contexts.is_empty() {
            return false;
        }
        let context = PaintStackingContext::from_banded_fragment_with_stack_level(
            policy.stack_level,
            fragment,
            child_contexts,
        )
        .with_source_order(self.next_paint_source_order())
        .with_effects(policy.effects)
        .with_bounds(bounds);
        self.current_page.replace_paint_tree_since_with_context(
            checkpoint,
            policy.parent_band,
            context,
        );
        true
    }

    /// Scope paint emitted since `checkpoint` as an atomic/effect box.
    ///
    /// CSS Transforms, CSS CssColor opacity, CSS Overflow clipping, replaced
    /// elements, inline-blocks, and table fragments all require descendants to
    /// paint as one isolated unit in the parent stacking order. This helper
    /// centralizes effect resolution so table/replaced/atomic callers do not
    /// accidentally drop transforms, opacity groups, overflow clips, links, or
    /// child stacking contexts:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>,
    /// <https://www.w3.org/TR/css-color-4/#transparency>, and
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
    pub(super) fn scope_current_page_atomic_paint_since(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        bounds: PaintClip,
        style: &ComputedStyle,
        child_contexts: Vec<PaintStackingContext>,
    ) -> bool {
        let policy = StackingContextPolicy::for_atomic(style, band, bounds);
        self.scope_current_page_paint_since_with_policy(checkpoint, policy, bounds, child_contexts)
    }

    /// Scope a replaced principal box as one monolithic paint subtree.
    ///
    /// CSS Fragmentation treats replaced elements as monolithic. Their own
    /// background/border and replaced content therefore have to be sliced or
    /// moved as one unit; leaving only the replaced-content primitive atomic
    /// can duplicate the background across columns.
    /// <https://www.w3.org/TR/css-break-3/#monolithic>
    pub(super) fn scope_current_page_replaced_paint_since(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        bounds: PaintClip,
        style: &ComputedStyle,
    ) -> bool {
        if self.float_paint_capture_depth > 0 {
            // The enclosing float promotes this complete subtree into the
            // Float paint band after layout. Scoping it here would remove it
            // from that capture and leave the replaced image behind in the
            // parent in-flow band.
            return false;
        }
        let fragment = self
            .current_page
            .paint_tree_fragment_since(checkpoint)
            .with_monolithic_fragmentation_scope(bounds);
        let mut policy = StackingContextPolicy::for_atomic(style, band, bounds);
        // Overflow clips a box's contents, not its own background and border.
        // Replaced-element painters establish the concrete-object crop while
        // producing their content primitive, so wrapping the whole atomic
        // fragment here would incorrectly clip the principal decoration too.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        policy.effects.clear_overflow_clip_effects();
        self.scope_current_page_fragment_with_policy(
            checkpoint,
            policy,
            bounds,
            fragment,
            Vec::new(),
        )
    }

    /// Scope an embedded SVG root as its required isolated SVG stacking
    /// context.
    ///
    /// This captures the CSS box decoration emitted before the SVG scene and
    /// the scene itself in one atomic group. The root's SVG viewport clipping
    /// remains on the scene primitives, so the principal box decoration is
    /// not incorrectly clipped by CSS overflow.
    /// <https://www.w3.org/TR/SVG2/render.html#EstablishingANewStackingContext>
    /// <https://www.w3.org/TR/SVG2/render.html#ParentCompositing>
    pub(super) fn scope_current_page_inline_svg_root_paint_since(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        bounds: PaintClip,
        style: &ComputedStyle,
    ) -> bool {
        if self.float_paint_capture_depth > 0 {
            return false;
        }
        let fragment = self
            .current_page
            .paint_tree_fragment_since(checkpoint)
            .with_monolithic_fragmentation_scope(bounds);
        let mut policy = StackingContextPolicy::for_inline_svg_root(style, band, bounds);
        // Overflow clips a box's contents, not its own background and border.
        // The inline SVG scene owns its viewport clip before it enters this
        // root compositing group.
        policy.effects.clear_overflow_clip_effects();
        self.scope_current_page_fragment_with_policy(
            checkpoint,
            policy,
            bounds,
            fragment,
            Vec::new(),
        )
    }

    pub(super) fn push_rect_in_band(&mut self, band: PaintBand, rect: RenderedRect) {
        if let Some(rect) = self.clip_rendered_rect(rect) {
            self.current_page.push_rect_in_band(band, rect);
        }
    }

    pub(super) fn push_rounded_rect_in_band(&mut self, band: PaintBand, rect: RenderedRoundedRect) {
        self.current_page.push_rounded_rect_in_band(band, rect);
    }

    pub(super) fn push_path_in_band(&mut self, band: PaintBand, path: RenderedPath) {
        self.current_page.push_path_in_band(band, path);
    }

    pub(super) fn push_svg_group_in_band(
        &mut self,
        band: PaintBand,
        group: crate::svg::SvgPaintGroup,
    ) {
        self.current_page.push_svg_group_in_band(band, group);
    }

    pub(super) fn box_background_primitives(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        self.box_background_primitives_with_background_image_areas(border_rect, border_rect, style)
    }

    /// Paint a fragment's decoration while resolving its image layers from a
    /// possibly different source positioning area.
    ///
    /// CSS Backgrounds slices a fragmented box's background image from the
    /// unfragmented positioning area, while the fragment itself remains the
    /// image's clip and border-paint area.  Generated pseudo-floats use this
    /// same path as principal boxes, so their vector SVG paths and patterns
    /// retain their normal background ordering when float paint is captured.
    ///
    /// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
    /// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
    /// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    pub(super) fn box_background_primitives_with_background_image_areas(
        &self,
        border_rect: PaintRect,
        positioning_border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        let background_images = self.resolved_background_image_paint_with_paint_areas(
            positioning_border_rect,
            border_rect,
            style,
        );
        self.box_background_primitives_with_resolved_images(border_rect, style, background_images)
    }

    fn box_background_primitives_with_resolved_images(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
        background_images: ResolvedBackgroundImagePaint,
    ) -> Vec<PaintPrimitive> {
        let border_paint = self.border_image_paint(border_rect, style);
        let mut normal_border_style = style.clone();
        // Normal and replacement borders are emitted in their own final
        // phase. The shared decoration phases must never let an authored
        // border-image affect their background or shadow primitives.
        normal_border_style.border_image.source = css::ComputedImage::None;
        let border_insets = used_border_widths(&normal_border_style);
        let defer_border_disjoint_images = background_images.border_disjoint_tile_geometry
            && matches!(&border_paint, BorderPaint::UseNormalBorder)
            && has_opaque_square_normal_border(style)
            && !style.box_shadow.iter().any(|shadow| shadow.inset);
        let mut background_images = background_images.primitives;
        let mut primitives = Vec::new();
        self.append_box_paint_phase(
            &mut primitives,
            block_paint_ops_with_phases(
                border_rect,
                &normal_border_style,
                border_insets,
                true,
                true,
                false,
                false,
            ),
        );
        if !defer_border_disjoint_images {
            primitives.append(&mut background_images);
        }
        self.append_box_paint_phase(
            &mut primitives,
            block_paint_ops_with_phases(
                border_rect,
                &normal_border_style,
                border_insets,
                false,
                false,
                true,
                false,
            ),
        );
        match border_paint {
            BorderPaint::UseNormalBorder => self.append_box_paint_phase(
                &mut primitives,
                block_paint_ops_with_phases(
                    border_rect,
                    &normal_border_style,
                    border_insets,
                    false,
                    false,
                    false,
                    true,
                ),
            ),
            BorderPaint::ReplaceNormalBorder {
                primitives: border_primitives,
            } => primitives.extend(border_primitives),
        }
        if defer_border_disjoint_images {
            primitives.append(&mut background_images);
        }
        primitives
    }

    /// Split a box with a normal CSS border into background and border phases.
    /// This lets HTML's rendered-legend model subtract its margin rectangle
    /// from border primitives without clipping backgrounds beneath the legend.
    /// Border-image replacement stays on its dedicated asset path.
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
    pub(super) fn split_background_and_normal_border_primitives(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Option<(
        Vec<PaintPrimitive>,
        Vec<PaintPrimitive>,
        Vec<PaintPrimitive>,
    )> {
        if style.border_image.source.is_image()
            || matches!(
                self.border_image_paint(border_rect, style),
                BorderPaint::ReplaceNormalBorder { .. }
            )
        {
            return None;
        }
        let mut normal_border_style = style.clone();
        normal_border_style.border_image.source = css::ComputedImage::None;
        let insets = used_border_widths(&normal_border_style);
        let background_images =
            self.resolved_background_image_paint_with_paint_areas(border_rect, border_rect, style);
        let defer_border_disjoint_images = background_images.border_disjoint_tile_geometry
            && has_opaque_square_normal_border(style)
            && !style.box_shadow.iter().any(|shadow| shadow.inset);
        let mut background_images = background_images.primitives;
        let mut backgrounds = Vec::new();
        let background_phase = block_paint_ops_with_phases(
            border_rect,
            &normal_border_style,
            insets,
            true,
            true,
            false,
            false,
        );
        self.append_box_paint_phase(&mut backgrounds, background_phase);
        if !defer_border_disjoint_images {
            backgrounds.append(&mut background_images);
        }
        let inset_shadow_phase = block_paint_ops_with_phases(
            border_rect,
            &normal_border_style,
            insets,
            false,
            false,
            true,
            false,
        );
        self.append_box_paint_phase(&mut backgrounds, inset_shadow_phase);
        let mut borders = Vec::new();
        let border_phase = block_paint_ops_with_phases(
            border_rect,
            &normal_border_style,
            insets,
            false,
            false,
            false,
            true,
        );
        self.append_box_paint_phase(&mut borders, border_phase);
        let deferred_images = if defer_border_disjoint_images {
            background_images
        } else {
            Vec::new()
        };
        Some((backgrounds, borders, deferred_images))
    }

    /// Clip rectangular border primitives to the fieldset regions visible
    /// around a rendered legend. Non-rectangular primitives retain their
    /// dedicated paint paths; those paths are handled by the corresponding
    /// rounded, patterned, and border-image exclusions as they acquire the
    /// same phase boundary.
    pub(super) fn clip_rectangular_border_primitives(
        &self,
        primitives: Vec<PaintPrimitive>,
        visible_regions: &[PaintRect],
    ) -> Vec<PaintPrimitive> {
        let mut clipped = Vec::new();
        for primitive in primitives {
            match primitive {
                PaintPrimitive::Rect(rect) => {
                    for region in visible_regions {
                        if let Some(intersection) = rect.paint_rect().intersection(region) {
                            let mut part = rect.clone();
                            part.set_paint_rect(intersection);
                            clipped.push(PaintPrimitive::Rect(part));
                        }
                    }
                }
                primitive => clipped.push(primitive),
            }
        }
        clipped
    }

    fn append_box_paint_phase(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        (rects, rounded_rects, paths, strokes): (
            Vec<RenderedRect>,
            Vec<RenderedRoundedRect>,
            Vec<RenderedPath>,
            Vec<RenderedStroke>,
        ),
    ) {
        primitives.extend(
            rects
                .into_iter()
                .filter_map(|rect| self.clip_rendered_rect(rect))
                .map(PaintPrimitive::Rect),
        );
        primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
        primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
        primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
    }

    pub(super) fn box_outline_primitives(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        outline_primitives_for_border_rect(border_rect, style)
    }

    pub(super) fn push_primitive_in_band(&mut self, band: PaintBand, primitive: PaintPrimitive) {
        match primitive {
            PaintPrimitive::Rect(rect) => self.push_rect_in_band(band, rect),
            PaintPrimitive::RoundedRect(rect) => self.push_rounded_rect_in_band(band, rect),
            PaintPrimitive::Path(path) => self.push_path_in_band(band, path),
            PaintPrimitive::Stroke(stroke) => self.push_stroke_in_band(band, stroke),
            PaintPrimitive::Image(image) => self.push_image_in_band(band, image),
            PaintPrimitive::ImagePattern(pattern) => self.push_image_pattern_in_band(band, pattern),
            PaintPrimitive::GradientPattern(pattern) => {
                self.current_page
                    .push_gradient_pattern_in_band(band, pattern);
            }
            PaintPrimitive::SvgPattern(pattern) => {
                self.current_page.push_svg_pattern_in_band(band, pattern);
            }
            PaintPrimitive::Line(line) => self.push_line_in_band(band, line),
            PaintPrimitive::OpaqueTextCoverage { line, paths } => {
                self.current_page
                    .push_opaque_text_coverage_in_band(band, line, paths);
            }
        }
    }

    pub(super) fn extend_strokes_in_band(
        &mut self,
        band: PaintBand,
        strokes: impl IntoIterator<Item = RenderedStroke>,
    ) {
        for stroke in strokes {
            self.push_stroke_in_band(band, stroke);
        }
    }

    pub(super) fn push_stroke_in_band(&mut self, band: PaintBand, stroke: RenderedStroke) {
        self.current_page.push_stroke_in_band(band, stroke);
    }

    pub(super) fn push_image(&mut self, image: RenderedImage) {
        self.push_image_in_band(PaintBand::InFlowBlock, image);
    }

    pub(super) fn push_image_in_band(&mut self, band: PaintBand, image: RenderedImage) {
        self.current_page.push_image_in_band(band, image);
    }

    pub(super) fn push_image_pattern_in_band(
        &mut self,
        band: PaintBand,
        pattern: RenderedImagePattern,
    ) {
        self.current_page.push_image_pattern_in_band(band, pattern);
    }

    pub(super) fn push_line_in_band(&mut self, band: PaintBand, line: RenderedLine) {
        match self.rendered_line_clip(&line) {
            Some(Some(clip)) => {
                self.current_page
                    .push_line_clipped_in_band(band, line, clip);
            }
            Some(None) => {
                self.current_page.push_line_in_band(band, line);
            }
            None => {}
        }
    }

    pub(super) fn push_overflow_clip(&mut self, clip: OverflowClip) {
        // An empty used scrollport is still an active overflow clip: it
        // suppresses every descendant rather than becoming `overflow:
        // visible`. Retaining it also keeps push/pop scope depth balanced;
        // dropping it would let `pop_overflow_clip` remove an ancestor clip.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        self.overflow_clips.push(clip);
    }

    /// Push a CSS overflow clip for a box's padding box.
    ///
    /// CSS Overflow clips non-visible overflow to the overflow clip edge, whose
    /// default position is the padding box. This helper is shared by formatting
    /// contexts that know their used content height before painting children:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_padding_box_overflow_clip(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        scrollbar_reservation: Option<ScrollbarGutterReservation>,
        outer_x: f32,
        block_top: f32,
        border_widths: css::Edges,
        content_width: f32,
        content_height: f32,
    ) -> bool {
        let containment = used_property_containment(element, style);
        if !style_clips_overflow(style) && !containment.clips_descendant_paint() {
            return false;
        }
        let used_overflow = UsedOverflowAxes::from_style(style);
        let (clip_edge_x, clip_edge_y) = overflow_clip_edge_axes(style);
        let (clip_x, clip_y) = overflow_clipping_axes(style);
        let margin = if clip_edge_x || clip_edge_y {
            style.overflow_clip_margin.length
        } else {
            0.0
        };
        let clip_height = content_height + style.padding.top + style.padding.bottom;
        let padding_box = PageTopRect::new(
            outer_x + border_widths.left - margin,
            block_top - border_widths.top + margin,
            content_width + style.padding.left + style.padding.right + margin * 2.0,
            clip_height + margin * 2.0,
        )
        .paint_clip();
        let scrollport = scrollbar_reservation.map_or_else(
            || ScrollportGeometry::for_padding_box(padding_box, style, used_overflow, false, false),
            |reservation| {
                ScrollportGeometry::for_padding_box_with_reservation(padding_box, reservation)
            },
        );
        self.push_overflow_clip(OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            scrollport.scrollport.paint_rect(),
            clip_x || containment.clips_descendant_paint(),
            clip_y || containment.clips_descendant_paint(),
            used_overflow.non_scrollable_clip_x(),
            used_overflow.non_scrollable_clip_y(),
        ));
        true
    }

    pub(super) fn pop_overflow_clip(&mut self, active: bool) {
        if active {
            self.overflow_clips.pop();
        }
    }

    /// Clip a filled rectangle to active CSS overflow clipping rectangles.
    ///
    /// CSS Overflow clips visual overflow to the overflow clip edge. The first
    /// implemented clip primitive is an axis-aligned rectangle, which covers
    /// block padding-box clipping for `overflow: hidden`:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
    fn clip_rendered_rect(&self, mut rect: RenderedRect) -> Option<RenderedRect> {
        for clip in &self.overflow_clips {
            let clip_rect = clip.paint_rect();
            let left = if clip.clips_x {
                rect.x().max(clip_rect.origin.x)
            } else {
                rect.x()
            };
            let right = if clip.clips_x {
                (rect.x() + rect.width()).min(clip_rect.origin.x + clip_rect.size.width)
            } else {
                rect.x() + rect.width()
            };
            let bottom = if clip.clips_y {
                rect.y().max(clip_rect.origin.y)
            } else {
                rect.y()
            };
            let top = if clip.clips_y {
                (rect.y() + rect.height()).min(clip_rect.origin.y + clip_rect.size.height)
            } else {
                rect.y() + rect.height()
            };
            if right <= left || top <= bottom {
                return None;
            }
            rect.set_paint_rect(paint_space_rect(left, bottom, right - left, top - bottom));
        }
        Some(rect)
    }

    /// Return the effective PDF clip for a text line, if it remains visible.
    ///
    /// CSS Tables 3 clips cell content portions that belong to collapsed
    /// tracks. PDF text is emitted as whole shaped lines, so a partial
    /// intersection needs an in-band graphics-state clip rather than line
    /// filtering; otherwise the glyph ink outside the collapsed cell leaks:
    /// <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>.
    fn rendered_line_clip(&self, line: &RenderedLine) -> Option<Option<PaintClip>> {
        rendered_line_clip_for_overflow_clips(line, &self.overflow_clips)
    }
}

fn rendered_line_clip_for_overflow_clips(
    line: &RenderedLine,
    overflow_clips: &[OverflowClip],
) -> Option<Option<PaintClip>> {
    if overflow_clips.is_empty() {
        return Some(None);
    }

    let line_clip = line.glyph_ink_bounds.unwrap_or_else(|| {
        let width = rendered_line_width(line);
        PaintClip::new(
            line.x(),
            line.y() - line.font_size,
            width,
            line.font_size * 1.35,
        )
    });
    let mut clipped = line_clip;
    for clip in overflow_clips {
        let clip_rect = clip.paint_rect();
        let left = if clip.clips_x {
            clipped.x().max(clip_rect.origin.x)
        } else {
            clipped.x()
        };
        let right = if clip.clips_x {
            (clipped.x() + clipped.width()).min(clip_rect.origin.x + clip_rect.size.width)
        } else {
            clipped.x() + clipped.width()
        };
        let bottom = if clip.clips_y {
            clipped.y().max(clip_rect.origin.y)
        } else {
            clipped.y()
        };
        let top = if clip.clips_y {
            (clipped.y() + clipped.height()).min(clip_rect.origin.y + clip_rect.size.height)
        } else {
            clipped.y() + clipped.height()
        };
        if right <= left || top <= bottom {
            return None;
        }
        clipped = PaintClip::new(left, bottom, right - left, top - bottom);
    }

    Some((clipped != line_clip).then_some(clipped))
}

fn rendered_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = if run.text_matrix.is_identity() {
            run.glyphs
                .as_ref()
                .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5)
        } else {
            run.font_size
        };
        width.max(run.x_offset + run_width)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn expanded_outline_rect_preserves_nonzero_paint_origin() {
        let border_rect = paint_space_rect(13.0, 29.0, 20.0, 40.0);

        assert_eq!(
            expanded_outline_paint_rect(border_rect, 5.0),
            paint_space_rect(8.0, 24.0, 30.0, 50.0)
        );
    }

    #[test]
    fn fragmented_decoration_keeps_source_positioning_separate_from_destination_clip() {
        let source = paint_space_rect(10.0, 20.0, 100.0, 200.0);
        let destination = paint_space_rect(40.0, 80.0, 100.0, 60.0);
        let slice = FragmentedDecorationSlice::new(
            source,
            destination,
            PaintTranslation::new(30.0, 60.0),
            false,
            true,
        );

        assert_eq!(
            slice.positioning_border_rect(css::BoxDecorationBreak::Slice),
            paint_space_rect(40.0, 80.0, 100.0, 200.0)
        );
        assert_eq!(
            slice.positioning_border_rect(css::BoxDecorationBreak::Clone),
            destination
        );
        assert!(!slice.owns_block_start());
        assert!(slice.owns_block_end());
    }

    #[test]
    fn partially_intersecting_line_receives_pdf_clip_scope() {
        let line = RenderedLine::new(
            "shouldbeclipped".to_string(),
            10.0,
            20.0,
            10.0,
            None,
            CssColor::BLACK,
            vec![RenderedTextRun {
                text: Rc::from("shouldbeclipped"),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: 10.0,
                font_id: None,
                glyphs: None,
            }],
        );
        let clip = OverflowClip::from_paint_rect(paint_space_rect(10.0, 0.0, 30.0, 40.0));

        assert_eq!(
            rendered_line_clip_for_overflow_clips(&line, &[clip]),
            Some(Some(PaintClip::new(10.0, 10.0, 30.0, 13.5)))
        );
    }

    #[test]
    fn glyph_ink_wholly_inside_fragment_edge_does_not_receive_synthetic_clip() {
        let mut line = RenderedLine::new(
            "ink fits".to_string(),
            10.0,
            20.0,
            10.0,
            None,
            CssColor::BLACK,
            Vec::new(),
        );
        // The line's conservative em box reaches y=10, but the selected glyph
        // outlines remain above the fragment edge at y=12. A PDF clip here
        // would introduce an antialiased edge without clipping any CSS ink.
        line.glyph_ink_bounds = Some(PaintClip::new(10.0, 13.0, 30.0, 7.0));
        let clip = OverflowClip::from_paint_rect(paint_space_rect(10.0, 12.0, 30.0, 28.0));

        assert_eq!(
            rendered_line_clip_for_overflow_clips(&line, &[clip]),
            Some(None)
        );
    }
}
