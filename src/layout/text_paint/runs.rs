use super::positioning::positioned_rendered_runs_for_writing_mode;
use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn paint_text_runs(
        &mut self,
        text: &str,
        origin: PaintPoint,
        style: &ComputedStyle,
    ) -> Option<RenderedLine> {
        let line_height = self.font_system.used_line_height(style).points();
        let shaped = self
            .font_system
            .shape_unwrapped_line(text, style, line_height)?;
        self.paint_shaped_inline_line(&shaped, origin, style)
    }

    /// Paint a previously shaped inline line without reshaping.
    ///
    /// CSS Text and CSS Fonts require the glyph run selected during shaping to
    /// remain the glyph run emitted by the renderer. Reusing
    /// `ShapedInlineLine` here keeps fallback font ids, glyph ids, advances,
    /// and ToUnicode cluster summaries stable through PDF output:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.10.3 "ToUnicode CMaps".
    pub(in crate::layout) fn paint_shaped_inline_line(
        &mut self,
        shaped: &ShapedInlineLine,
        origin: PaintPoint,
        style: &ComputedStyle,
    ) -> Option<RenderedLine> {
        let mut rendered_runs = positioned_rendered_runs_for_writing_mode(shaped, style);
        if rendered_runs.is_empty() {
            return None;
        }
        debug_assert!(shaped.advance_width().is_finite());
        let first_font_id = shaped.first_font_id();
        let origin = PaintPoint::new(origin.x, origin.y + shaped.baseline_adjustment);
        let raster_glyph_images = self
            .font_system
            .take_raster_glyph_images(origin, &mut rendered_runs);
        let palettes = vec![style.font_palette.clone(); rendered_runs.len()];
        let color_glyph_paths =
            self.font_system
                .take_color_glyph_paths(origin, &mut rendered_runs, &palettes, style);
        let rendered_line = RenderedLine::from_paint_origin(
            shaped.text.to_string(),
            origin,
            rendered_line_font_size(&rendered_runs, style.font_size),
            first_font_id,
            style.text_fill_color.unwrap_or(style.color),
            rendered_runs,
        );
        self.paint_text_shadows(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x(),
            rendered_line.y(),
            shaped.advance_width(),
            style,
            &rendered_line.runs,
            TextDecorationPaintPhase::BeforeText,
        );
        for path in color_glyph_paths {
            self.push_path_in_band(PaintBand::Inline, path);
        }
        for image in raster_glyph_images {
            self.push_image_in_band(PaintBand::Inline, image);
        }
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_prepared_text_emphasis_marks_for_line(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x(),
            rendered_line.y(),
            shaped.advance_width(),
            style,
            &rendered_line.runs,
            TextDecorationPaintPhase::AfterText,
        );
        Some(rendered_line)
    }
    pub(in crate::layout) fn paint_prepared_inline_text_group(
        &mut self,
        group: &PreparedInlineTextGroup,
    ) {
        let source = match group.source {
            InlineTextSource::Normal | InlineTextSource::Generated => RenderedLineSource::Normal,
            InlineTextSource::RunIn => RenderedLineSource::RunIn,
            InlineTextSource::Marker => RenderedLineSource::Marker,
        };
        self.paint_prepared_inline_text_group_with_source(group, source);
    }

    pub(in crate::layout) fn paint_prepared_inline_text_group_with_source(
        &mut self,
        group: &PreparedInlineTextGroup,
        source: RenderedLineSource,
    ) {
        let mut rendered_runs =
            positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
        if rendered_runs.is_empty() {
            return;
        }
        let first_font_id = group.shaped.first_font_id();
        let text_origin = group.bounds.text_origin();
        let raster_glyph_images = self
            .font_system
            .take_raster_glyph_images(text_origin, &mut rendered_runs);
        let color_glyph_paths = self.font_system.take_color_glyph_paths(
            text_origin,
            &mut rendered_runs,
            &group
                .shaped
                .runs
                .iter()
                .map(|run| run.font_palette.clone())
                .collect::<Vec<_>>(),
            &group.style,
        );
        let rendered_line = RenderedLine::from_paint_origin_with_source(
            group.shaped.text.to_string(),
            text_origin,
            rendered_line_font_size(&rendered_runs, group.style.font_size),
            first_font_id,
            group.style.text_fill_color.unwrap_or(group.style.color),
            rendered_runs,
            source,
        );
        let decoration_runs = rendered_line.runs.clone();
        let mut decoration_style = group.style.clone();
        let (decoration_x, decoration_baseline_y, decoration_width, decoration_style) =
            if let Some(rect) = group.decoration_paint_rect {
                match group.style.writing_mode {
                    WritingMode::HorizontalTb => {
                        decoration_style.font_size = rect.height().max(1.0);
                        (
                            rect.origin.x,
                            rect.origin.y,
                            rect.width(),
                            &decoration_style,
                        )
                    }
                    WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr => {
                        decoration_style.font_size = rect.width().max(1.0);
                        (
                            rect.origin.x,
                            rect.origin.y,
                            rect.height(),
                            &decoration_style,
                        )
                    }
                }
            } else {
                (group.x(), group.y(), group.width(), &group.style)
            };
        self.paint_text_shadows(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            decoration_x,
            decoration_baseline_y,
            decoration_width,
            decoration_style,
            &decoration_runs,
            TextDecorationPaintPhase::BeforeText,
        );
        for path in color_glyph_paths {
            self.push_path_in_band(PaintBand::Inline, path);
        }
        for image in raster_glyph_images {
            self.push_image_in_band(PaintBand::Inline, image);
        }
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_prepared_text_emphasis_marks_for_line(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            decoration_x,
            decoration_baseline_y,
            decoration_width,
            decoration_style,
            &decoration_runs,
            TextDecorationPaintPhase::AfterText,
        );

        if let Some(target) = &group.link_target {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                group.link_paint_rect(),
                target.clone(),
            ));
        }
    }

    /// Paint one inline fragment's background and border for a line box.
    ///
    /// CSS Backgrounds and Borders applies backgrounds and borders to inline
    /// boxes on each generated line box fragment. CSS 2.2 defines the inline
    /// box content area independently from line-height; vertical padding and
    /// borders start at the content-area edges rather than shrinking into
    /// glyph content. CSS Text hanging separators remain part of the fragment
    /// for painting even when excluded from line measurement:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>,
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>,
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-color> and
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(in crate::layout) fn paint_inline_fragment_background(
        &mut self,
        fragment: &InlineFragment,
        rect: PaintRect,
    ) {
        if fragment.style().visibility != Visibility::Visible
            || (!fragment.style().display.is_inline_level()
                && !fragment.force_inline_background_paint())
            || fragment.style().display.is_atomic_inline()
            || rect.width() <= 0.0
            || rect.height() <= 0.0
            || (fragment.style().background_color.is_none()
                && fragment.style().background_image.is_none()
                && used_border_width(fragment.style()) == layout_pt(0.0))
        {
            return;
        }
        let mut x = rect.min_x();
        let mut y = rect.min_y();
        let mut width = rect.width();
        let mut height = rect.height();
        let mut style = fragment.style().clone();
        apply_inline_fragment_edge_painting(
            &mut style,
            fragment.hanging_edges(),
            &mut x,
            &mut y,
            &mut width,
            &mut height,
        );
        for primitive in
            self.box_background_primitives(paint_space_rect(x, y, width, height), &style)
        {
            self.push_primitive_in_band(PaintBand::Inline, primitive);
        }
    }
}

pub(in crate::layout) fn rendered_line_font_size(
    rendered_runs: &[RenderedTextRun],
    fallback: f32,
) -> f32 {
    rendered_runs
        .iter()
        .find(|run| !run.text.is_empty())
        .map(|run| run.font_size)
        .unwrap_or(fallback)
}
