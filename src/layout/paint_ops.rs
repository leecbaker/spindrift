use super::*;

impl<'a> LayoutBuilder<'a> {
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
        let Some(fragment) = self.current_page.paint_tree_fragment_since(checkpoint) else {
            return false;
        };
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

    /// Scope paint emitted since `checkpoint` as an atomic/effect box.
    ///
    /// CSS Transforms, CSS Color opacity, CSS Overflow clipping, replaced
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
        self.scope_current_page_paint_since(
            checkpoint,
            policy.parent_band,
            bounds,
            child_contexts,
            policy.effects,
        )
    }

    pub(super) fn push_rect(&mut self, rect: RenderedRect) {
        self.push_rect_in_band(PaintBand::InFlowBlock, rect);
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

    pub(super) fn box_background_primitives(
        &self,
        outer_x: f32,
        block_bottom: f32,
        outer_width: f32,
        block_height: f32,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        let (rects, rounded_rects, paths, strokes) =
            block_paint_ops(outer_x, block_bottom, outer_width, block_height, style);
        let mut primitives = Vec::new();
        primitives.extend(
            rects
                .into_iter()
                .filter_map(|rect| self.clip_rendered_rect(rect))
                .map(PaintPrimitive::Rect),
        );
        primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
        primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
        primitives.extend(
            self.background_images(outer_x, block_bottom, outer_width, block_height, style)
                .into_iter()
                .map(PaintPrimitive::Image),
        );
        primitives.extend(
            self.border_image_slices(outer_x, block_bottom, outer_width, block_height, style)
                .into_iter()
                .map(PaintPrimitive::Image),
        );
        primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
        primitives
    }

    /// Builds CSS outline paint primitives for a box border area.
    ///
    /// CSS UI paints outlines outside the border edge and excludes them from box
    /// sizing. The renderer models this as a synthetic border expanded by the
    /// used outline offset and outline width, then emits it in CSS 2.2 Appendix E's
    /// final outline paint band:
    /// <https://www.w3.org/TR/css-ui-3/#outline-props> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(super) fn box_outline_primitives(
        &self,
        outer_x: f32,
        block_bottom: f32,
        outer_width: f32,
        block_height: f32,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        if style.outline_width <= 0.0 || style.outline_style.suppresses_used_width() {
            return Vec::new();
        }
        if outer_width <= 0.0 || block_height <= 0.0 {
            return Vec::new();
        }

        let mut outline_style = style.clone();
        outline_style.background_color = None;
        outline_style.background_image = None;
        outline_style.background_layers.clear();
        outline_style.border_image = css::BorderImage::initial();
        outline_style.border_width = style.outline_width;
        outline_style.border_widths = css::Edges {
            top: style.outline_width,
            right: style.outline_width,
            bottom: style.outline_width,
            left: style.outline_width,
        };
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

        let outset = style.outline_offset + style.outline_width;
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(
            outer_x - outset,
            block_bottom - outset,
            outer_width + outset * 2.0,
            block_height + outset * 2.0,
            &outline_style,
        );
        let mut primitives = Vec::new();
        primitives.extend(rects.into_iter().map(PaintPrimitive::Rect));
        primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
        primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
        primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
        primitives
    }

    pub(super) fn push_primitive_in_band(&mut self, band: PaintBand, primitive: PaintPrimitive) {
        match primitive {
            PaintPrimitive::Rect(rect) => self.push_rect_in_band(band, rect),
            PaintPrimitive::RoundedRect(rect) => self.push_rounded_rect_in_band(band, rect),
            PaintPrimitive::Path(path) => self.push_path_in_band(band, path),
            PaintPrimitive::Stroke(stroke) => self.push_stroke_in_band(band, stroke),
            PaintPrimitive::Image(image) => self.push_image_in_band(band, image),
            PaintPrimitive::Line(line) => self.push_line_in_band(band, line),
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

    pub(super) fn push_line_in_band(&mut self, band: PaintBand, line: RenderedLine) {
        if self.rendered_line_intersects_active_clips(&line) {
            self.current_page.push_line_in_band(band, line);
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
    pub(super) fn push_padding_box_overflow_clip(
        &mut self,
        style: &ComputedStyle,
        outer_x: f32,
        block_top: f32,
        border_widths: css::Edges,
        content_width: f32,
        content_height: f32,
    ) -> bool {
        if !style.overflow.clips_overflow() {
            return false;
        }
        let clip_height = content_height + style.padding.top + style.padding.bottom;
        self.push_overflow_clip(OverflowClip::from_page_top_rect(PageTopRect::new(
            outer_x + border_widths.left,
            block_top - border_widths.top,
            content_width + style.padding.left + style.padding.right,
            clip_height,
        )));
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
            let left = rect.x().max(clip_rect.origin.x);
            let right = (rect.x() + rect.width()).min(clip_rect.origin.x + clip_rect.size.width);
            let bottom = rect.y().max(clip_rect.origin.y);
            let top = (rect.y() + rect.height()).min(clip_rect.origin.y + clip_rect.size.height);
            if right <= left || top <= bottom {
                return None;
            }
            rect.set_paint_rect(paint_space_rect(left, bottom, right - left, top - bottom));
        }
        Some(rect)
    }

    /// Return whether a text line intersects all active rectangular clips.
    ///
    /// CSS Tables 3 clips cell content portions that belong to collapsed row
    /// tracks. PDF text is emitted as whole shaped lines, so this clips the
    /// common table-rowspan case by suppressing text lines wholly outside the
    /// active clip while leaving partially intersecting glyph runs intact:
    /// <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>.
    fn rendered_line_intersects_active_clips(&self, line: &RenderedLine) -> bool {
        if self.overflow_clips.is_empty() {
            return true;
        }

        let width = rendered_line_width(line);
        let left = line.x();
        let right = line.x() + width;
        let bottom = line.y() - line.font_size;
        let top = line.y() + line.font_size * 0.35;

        self.overflow_clips.iter().all(|clip| {
            let clip_rect = clip.paint_rect();
            right > clip_rect.origin.x
                && left < clip_rect.origin.x + clip_rect.size.width
                && top > clip_rect.origin.y
                && bottom < clip_rect.origin.y + clip_rect.size.height
        })
    }
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
