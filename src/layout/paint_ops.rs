use super::*;
use crate::layout::assets::BorderPaint;

/// Whether every background layer is either empty or a decoded raster URL
/// whose source can use the independent decoration-phase painter.
///
/// This intentionally excludes SVG and generated CSS images: those paths are
/// still coupled to legacy aggregate paint-time geometry and must move to the
/// phase model with their own source-specific tests.
fn uses_only_raster_url_background_layers(style: &ComputedStyle) -> bool {
    if !style.background_layers.is_empty() {
        return style
            .background_layers
            .iter()
            .all(|layer| match layer.image.as_image() {
                None => layer.image.is_none(),
                Some(BackgroundImage::Url { src, .. }) => raster_url_suffix(src),
                _ => false,
            });
    }
    match style.background_image.as_image() {
        None => style.background_image.is_none(),
        Some(BackgroundImage::Url { src, .. }) => raster_url_suffix(src),
        _ => false,
    }
}

fn raster_url_suffix(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim()
        .to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".bmp"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
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
    outline_style.background_color = css::BackgroundColor::TRANSPARENT;
    outline_style.background_image = css::ComputedImage::None;
    outline_style.background_layers.clear();
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
        policy.effects.overflow_clip = None;
        policy.effects.rounded_overflow_clip = None;
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
        let background_images = self.background_image_primitives_with_paint_areas(
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
        background_images: Vec<PaintPrimitive>,
    ) -> Vec<PaintPrimitive> {
        if uses_only_raster_url_background_layers(style)
            && style.box_shadow.is_empty()
            && !style.border_image.source.is_image()
        {
            return self.raster_background_box_primitives_in_css_paint_order(
                border_rect,
                style,
                background_images,
            );
        }

        let border_paint = self.border_image_paint(border_rect, style);
        let mut normal_border_style = style.clone();
        if matches!(&border_paint, BorderPaint::UseNormalBorder) {
            normal_border_style.border_image.source = css::ComputedImage::None;
        }
        let mut primitives = Vec::new();
        let (rects, rounded_rects, paths, strokes) =
            block_paint_ops(border_rect, &normal_border_style);
        primitives.extend(
            rects
                .into_iter()
                .filter_map(|rect| self.clip_rendered_rect(rect))
                .map(PaintPrimitive::Rect),
        );
        primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
        primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
        primitives.extend(background_images);
        if let BorderPaint::ReplaceNormalBorder {
            primitives: border_primitives,
        } = border_paint
        {
            primitives.extend(border_primitives);
        }
        primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
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
    ) -> Option<(Vec<PaintPrimitive>, Vec<PaintPrimitive>)> {
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
        let mut backgrounds = Vec::new();
        let background_phase = block_paint_ops_with_phases(
            border_rect,
            &normal_border_style,
            insets,
            true,
            true,
            true,
            false,
        );
        self.append_box_paint_phase(&mut backgrounds, background_phase);
        backgrounds.extend(self.background_image_primitives(border_rect, style));
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
        Some((backgrounds, borders))
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

    /// Emit simple raster URL backgrounds in CSS Backgrounds' decoration
    /// order. This keeps a translucent border above the image it overlays.
    ///
    /// Generated images and SVGs keep the aggregate path until they can use
    /// the same fully phase-typed representation; unlike decoded raster URLs,
    /// they still have legacy paint-time dependencies on that aggregate path.
    /// <https://www.w3.org/TR/css-backgrounds-3/#layering>.
    fn raster_background_box_primitives_in_css_paint_order(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
        background_images: Vec<PaintPrimitive>,
    ) -> Vec<PaintPrimitive> {
        let border_insets = used_border_widths(style);
        let mut primitives = Vec::new();
        self.append_box_paint_phase(
            &mut primitives,
            block_paint_ops_with_phases(
                border_rect,
                style,
                border_insets,
                true,
                true,
                false,
                false,
            ),
        );
        primitives.extend(background_images);
        self.append_box_paint_phase(
            &mut primitives,
            block_paint_ops_with_phases(
                border_rect,
                style,
                border_insets,
                false,
                false,
                false,
                true,
            ),
        );
        primitives
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
        if clip.width() > 0.0 && clip.height() > 0.0 {
            self.overflow_clips.push(clip);
        }
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
        let scrollport = ScrollportGeometry::for_padding_box(
            padding_box,
            style,
            UsedOverflowAxes::from_style(style),
            false,
            false,
        );
        self.push_overflow_clip(
            OverflowClip::from_paint_rect(scrollport.scrollport.paint_rect()).with_axes(
                clip_x || containment.clips_descendant_paint(),
                clip_y || containment.clips_descendant_paint(),
            ),
        );
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
