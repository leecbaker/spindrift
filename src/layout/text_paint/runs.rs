use super::positioning::positioned_rendered_runs_for_writing_mode;
use super::*;
use std::rc::Rc;

/// The decoration paint inputs for one shaped text group.
///
/// Text-decoration placement can differ from the glyph baseline when
/// `text-box-trim` selects a separate paint rectangle.  Keep that used
/// geometry together, in text-decoration's logical inline space, rather than
/// carrying an untyped `(x, y, width, style)` tuple through the line painter.
#[derive(Debug, Clone)]
struct PreparedTextDecorationPaint {
    baseline: PaintPoint,
    receivers: Vec<PreparedTextDecorationReceiverPaint>,
}

/// One lexical receiver range materialized into page-local decoration paint
/// coordinates.  Its layer chain is preserved separately from the text
/// group's first style so a shared glyph run cannot extend a propagated line
/// into a following sibling.
#[derive(Debug, Clone)]
struct PreparedTextDecorationReceiverPaint {
    inline_span: TextInlineSpan,
    style: ComputedStyle,
    layers: Vec<crate::css::TextDecorationLayer>,
}

/// Paint resources prepared once for one inline text group.
///
/// Color and bitmap glyph extraction mutates the rendered run to remove the
/// glyphs emitted by those backends.  Materializing every resource before the
/// line scheduler starts keeps that extraction single-shot while allowing CSS
/// Text Decoration paint phases to span multiple styled groups.
#[derive(Debug)]
struct PreparedInlineTextPaint {
    rendered_line: RenderedLine,
    glyph_paths: Vec<RenderedPath>,
    glyph_images: Vec<RenderedImage>,
    opaque_text_coverage_paths: Vec<RenderedPath>,
    decorations: PreparedTextDecorationPaint,
    text_style: ComputedStyle,
    link: Option<RenderedLink>,
}

/// The globally ordered paint phases for compatible text groups on one line.
///
/// CSS Text Decoration Level 3 §5.1 orders text effects independently of the
/// inline fragments that supplied the text: shadows, underlines, overlines,
/// text, emphasis marks, and line-throughs.
/// <https://www.w3.org/TR/css-text-decor-3/#painting-order>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineTextPaintPhase {
    Shadows,
    BeforeTextDecorations,
    Glyphs,
    Emphasis,
    AfterTextDecorations,
    Links,
}

impl InlineTextPaintPhase {
    const ORDER: [Self; 6] = [
        Self::Shadows,
        Self::BeforeTextDecorations,
        Self::Glyphs,
        Self::Emphasis,
        Self::AfterTextDecorations,
        Self::Links,
    ];
}

fn rendered_line_source_for_inline_text_group(
    group: &PreparedInlineTextGroup,
) -> RenderedLineSource {
    match group.source {
        InlineTextSource::Normal
        | InlineTextSource::Generated
        | InlineTextSource::GeneratedWbr
        | InlineTextSource::WordSpaceTransform(_)
        | InlineTextSource::BlockEllipsis
        | InlineTextSource::FootnoteCall(_)
        | InlineTextSource::BidiControl => RenderedLineSource::Normal,
        InlineTextSource::RunIn => RenderedLineSource::RunIn,
        InlineTextSource::Marker => RenderedLineSource::Marker,
    }
}

fn prepared_inline_text_group_paint_opacity(group: &PreparedInlineTextGroup) -> f32 {
    if group.paint_scope_ancestry.is_empty() {
        group.style.opacity.value()
    } else {
        group.paint_opacity
    }
}

/// Whether a text receiver carries a decoration from `origin_style`.
///
/// Decoration propagation is modeled by the layer list rather than the
/// receiver's non-inherited `text-decoration-line` longhand.  The origin's
/// `Rc` is its stable identity, so equal-valued declarations cannot collapse.
fn text_decoration_layers_receive_origin(
    layers: &[crate::css::TextDecorationLayer],
    origin_style: &Rc<ComputedStyle>,
) -> bool {
    layers
        .iter()
        .any(|layer| Rc::ptr_eq(&layer.origin_style, origin_style))
}

impl<'a> LayoutBuilder<'a> {
    /// Translate a CSS layout baseline into the selected font program's paint
    /// origin. Every shaped-text paint route must use this exact conversion.
    fn shaped_text_paint_origin(
        &self,
        layout_baseline: PaintPoint,
        shaped: &ShapedInlineLine,
    ) -> PaintPoint {
        layout_baseline + PaintDisplacement::new(0.0, shaped.baseline_adjustment)
    }

    /// Align sideways glyph ink to the vertical line box's logical block span.
    ///
    /// A sideways run retains the font's horizontal alphabetic baseline and
    /// is then rotated at the PDF boundary.  Its horizontal ascent or
    /// descender would otherwise project past the cell's logical block-start
    /// edge.  Move only rotated runs by the appropriate font-metric span;
    /// upright runs already carry their OpenType vertical-origin correction
    /// from shaping and must retain their own cross-axis position.
    ///
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    /// <https://drafts.csswg.org/css-inline-3/#line-box>
    pub(in crate::layout) fn align_sideways_runs_to_vertical_line_box(
        &mut self,
        runs: &mut [RenderedTextRun],
        shaped: &ShapedInlineLine,
        style: &ComputedStyle,
    ) {
        if !style.writing_mode.has_vertical_lines() {
            return;
        }
        for run in runs {
            let descent = self
                .font_system
                .text_decoration_metrics(run.font_id.or_else(|| shaped.first_font_id()), style)
                .descender_depth;
            if run.text_matrix == RenderedTextMatrix::ROTATE_CW {
                run.x_offset += descent;
            } else if run.text_matrix == RenderedTextMatrix::ROTATE_CCW {
                run.x_offset += (style.line_height - descent).max(0.0);
            }
        }
    }

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
        self.align_sideways_runs_to_vertical_line_box(&mut rendered_runs, shaped, style);
        if rendered_runs.is_empty() {
            return None;
        }
        debug_assert!(shaped.advance_width().is_finite());
        let first_font_id = shaped.first_font_id();
        let origin = self.shaped_text_paint_origin(origin, shaped);
        let raster_glyph_images = self
            .font_system
            .take_raster_glyph_images(origin, &mut rendered_runs);
        let color_glyph_paths =
            self.font_system
                .take_color_glyph_paths(origin, &mut rendered_runs, style);
        let mut rendered_line = RenderedLine::from_paint_origin(
            shaped.text.to_string(),
            origin,
            rendered_line_font_size(&rendered_runs, style.font_size),
            first_font_id,
            style.text_fill_color.unwrap_or(style.color),
            rendered_runs,
        )
        .with_glyph_origin_adjustment(PaintDisplacement::new(0.0, shaped.baseline_adjustment));
        let glyph_ink_bounds =
            glyph_ink_bounds_for_rendered_line(&self.font_system, &rendered_line);
        rendered_line = rendered_line.with_glyph_ink_bounds(glyph_ink_bounds);
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
        let full_em_rect_coverage_paths = self.font_system.full_em_rect_glyph_coverage_paths(
            origin,
            &rendered_line.runs,
            style.text_fill_color.unwrap_or(style.color),
        );
        if full_em_rect_coverage_paths.is_empty() {
            self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        } else {
            self.current_page.push_opaque_text_coverage_in_band(
                PaintBand::Inline,
                rendered_line.clone(),
                full_em_rect_coverage_paths,
            );
        }
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
    pub(in crate::layout) fn paint_prepared_inline_text_group_with_decoration_geometries(
        &mut self,
        group: &PreparedInlineTextGroup,
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        let source = rendered_line_source_for_inline_text_group(group);
        self.paint_prepared_inline_text_group_with_source_and_decoration_geometries(
            group,
            source,
            decoration_geometries,
        );
    }

    pub(in crate::layout) fn paint_prepared_inline_text_group_with_source_and_decoration_geometries(
        &mut self,
        group: &PreparedInlineTextGroup,
        source: RenderedLineSource,
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        // A direct text fragment has no lexical owner beyond its own used
        // style. Once materialized pseudo/inline ancestry is present, retain
        // the prepared product so each copied source slice uses the same
        // lexical effect chain.
        let paint_opacity = prepared_inline_text_group_paint_opacity(group);
        if paint_opacity >= 1.0 {
            self.paint_prepared_inline_text_group_unscoped(group, source, decoration_geometries);
            return;
        }

        // `opacity` applies after the inline-like pseudo box and all of its
        // text paint have composed. Capture the complete prepared text group
        // rather than attenuating glyph colors, so shadows, decorations,
        // color glyph paths, raster glyphs, and links stay in the same PDF
        // transparency group.
        // <https://www.w3.org/TR/css-color-4/#transparency> and
        // <https://www.w3.org/TR/css-pseudo-4/#first-letter-styling>
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_prepared_inline_text_group_unscoped(group, source, decoration_geometries);
        let mut fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if fragment.is_empty() {
            return;
        }
        // Link annotations participate in the same transformed geometry, but
        // they are interactive page annotations rather than graphical paint.
        // Keep them outside the PDF transparency form so `opacity: 0` hides
        // the owned glyph/decorations without disabling hit testing.
        // <https://drafts.csswg.org/css-color-4/#transparency>
        let links = std::mem::take(&mut fragment.links);
        let bounds = PaintClip::from_paint_rect(group.link_paint_rect());
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                opacity: paint_opacity,
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(PaintBand::Inline, context),
            PaintTranslation::identity(),
        );
        if !links.is_empty() {
            self.current_page.append_paint_fragment_owned(
                PaintFragment::from_primitives(Vec::new(), links),
                PaintTranslation::identity(),
            );
        }
    }

    /// Paint compatible non-effect text groups by CSS-wide phase.
    ///
    /// Opacity is a compositing boundary, so callers must flush this scheduler
    /// before an effect-bearing group and use that group's atomic painter.
    pub(in crate::layout) fn paint_prepared_inline_text_groups_in_phases(
        &mut self,
        groups: &[&PreparedInlineTextGroup],
        text_source: Option<RenderedLineSource>,
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        if groups.is_empty() {
            return;
        }
        debug_assert!(
            groups
                .iter()
                .all(|group| { prepared_inline_text_group_paint_opacity(group) >= 1.0 })
        );
        let mut prepared_groups = groups
            .iter()
            .filter_map(|group| {
                self.prepare_inline_text_paint(
                    group,
                    text_source
                        .unwrap_or_else(|| rendered_line_source_for_inline_text_group(group)),
                )
            })
            .collect::<Vec<_>>();
        self.paint_prepared_inline_text_paint_records_in_phases(
            &mut prepared_groups,
            decoration_geometries,
        );
    }

    /// Paint materialized text records in CSS-wide line phases.
    ///
    /// Underlines must all precede overlines, independently of the styled
    /// text group that supplies each segment. Shadows remain group-local
    /// because inherited `currentcolor` can resolve differently on each
    /// receiving descendant.
    fn paint_prepared_inline_text_paint_records_in_phases(
        &mut self,
        prepared_groups: &mut [PreparedInlineTextPaint],
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        for phase in InlineTextPaintPhase::ORDER {
            match phase {
                InlineTextPaintPhase::BeforeTextDecorations => {
                    self.paint_prepared_inline_text_decoration_phase(
                        prepared_groups,
                        TextDecorationPaintPhase::Underlines,
                        decoration_geometries,
                    );
                    self.paint_prepared_inline_text_decoration_phase(
                        prepared_groups,
                        TextDecorationPaintPhase::Overlines,
                        decoration_geometries,
                    );
                }
                InlineTextPaintPhase::AfterTextDecorations => {
                    self.paint_prepared_inline_text_decoration_phase(
                        prepared_groups,
                        TextDecorationPaintPhase::AfterText,
                        decoration_geometries,
                    );
                }
                _ => {
                    for prepared in &mut *prepared_groups {
                        self.paint_prepared_inline_text_paint_phase(
                            prepared,
                            phase,
                            decoration_geometries,
                        );
                    }
                }
            }
        }
    }

    /// Paint a global CSS decoration phase from origin-owned jobs.
    ///
    /// The outer loop owns a line declaration, while the inner loop merely
    /// emits the receiver spans that carry that declaration.  This is
    /// deliberately identity-based: a receiver's computed
    /// `text-decoration-line` is initial even when it carries a propagated
    /// line, and two identical declarations may belong to different origins.
    ///
    /// CSS Text Decoration Level 4 § 2.5 and § 4.1.1.
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    /// <https://drafts.csswg.org/css-text-decor-4/#painting-order>
    fn paint_prepared_inline_text_decoration_phase(
        &mut self,
        prepared_groups: &[PreparedInlineTextPaint],
        phase: TextDecorationPaintPhase,
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        for geometry in decoration_geometries {
            for prepared in prepared_groups {
                for receiver in &prepared.decorations.receivers {
                    if !text_decoration_layers_receive_origin(
                        &receiver.layers,
                        &geometry.layer.origin_style,
                    ) {
                        continue;
                    }
                    let (x, baseline_y) = match receiver.style.writing_mode {
                        WritingMode::HorizontalTb => {
                            (receiver.inline_span.start, prepared.decorations.baseline.y)
                        }
                        WritingMode::VerticalRl
                        | WritingMode::VerticalLr
                        | WritingMode::SidewaysRl
                        | WritingMode::SidewaysLr => {
                            (prepared.decorations.baseline.x, receiver.inline_span.start)
                        }
                    };
                    self.paint_text_decoration_layer(
                        x,
                        baseline_y,
                        receiver.inline_span.length(),
                        &receiver.style,
                        &prepared.rendered_line.runs,
                        &geometry.layer,
                        phase,
                        None,
                        Some(geometry),
                    );
                }
            }
        }

        // A text group can be materialized outside the line-geometry pass
        // (for example after a fragmentation or anonymous-inline boundary).
        // Its decoration origins still belong to that receiver and must not
        // disappear merely because no aggregate line geometry was recorded.
        // Paint those unmatched origins with the group's own geometry; origins
        // represented above remain exclusively in the line-wide path.
        for prepared in prepared_groups {
            for receiver in &prepared.decorations.receivers {
                for decoration in &receiver.layers {
                    if decoration_geometries.iter().any(|geometry| {
                        std::rc::Rc::ptr_eq(&geometry.layer.origin_style, &decoration.origin_style)
                    }) {
                        continue;
                    }
                    let (x, baseline_y) = match receiver.style.writing_mode {
                        WritingMode::HorizontalTb => {
                            (receiver.inline_span.start, prepared.decorations.baseline.y)
                        }
                        WritingMode::VerticalRl
                        | WritingMode::VerticalLr
                        | WritingMode::SidewaysRl
                        | WritingMode::SidewaysLr => {
                            (prepared.decorations.baseline.x, receiver.inline_span.start)
                        }
                    };
                    self.paint_text_decoration_layer(
                        x,
                        baseline_y,
                        receiver.inline_span.length(),
                        &receiver.style,
                        &prepared.rendered_line.runs,
                        decoration,
                        phase,
                        None,
                        None,
                    );
                }
            }
        }
    }

    fn paint_prepared_inline_text_group_unscoped(
        &mut self,
        group: &PreparedInlineTextGroup,
        source: RenderedLineSource,
        decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        let Some(mut prepared) = self.prepare_inline_text_paint(group, source) else {
            return;
        };
        self.paint_prepared_inline_text_paint_records_in_phases(
            std::slice::from_mut(&mut prepared),
            decoration_geometries,
        );
    }

    /// Materialize the single-use glyph backend resources for one text group.
    ///
    /// This does not append page paint.  The prepared result can therefore be
    /// scheduled with sibling groups in the CSS text-decoration paint order.
    fn prepare_inline_text_paint(
        &mut self,
        group: &PreparedInlineTextGroup,
        source: RenderedLineSource,
    ) -> Option<PreparedInlineTextPaint> {
        let mut rendered_runs =
            positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
        self.align_sideways_runs_to_vertical_line_box(
            &mut rendered_runs,
            &group.shaped,
            &group.style,
        );
        if rendered_runs.is_empty() {
            return None;
        }
        let first_font_id = group.shaped.first_font_id();
        let text_origin = self.shaped_text_paint_origin(group.bounds.text_origin(), &group.shaped);
        let raster_glyph_images = self
            .font_system
            .take_raster_glyph_images(text_origin, &mut rendered_runs);
        let color_glyph_paths =
            self.font_system
                .take_color_glyph_paths(text_origin, &mut rendered_runs, &group.style);
        let mut rendered_line = RenderedLine::from_paint_origin_with_source(
            group.shaped.text.to_string(),
            text_origin,
            rendered_line_font_size(&rendered_runs, group.style.font_size),
            first_font_id,
            group.style.text_fill_color.unwrap_or(group.style.color),
            rendered_runs,
            source,
        )
        .with_glyph_origin_adjustment(PaintDisplacement::new(
            0.0,
            group.shaped.baseline_adjustment,
        ))
        .with_source_run(Rc::clone(&group.source_run));
        apply_word_space_transform_actual_text(&mut rendered_line, group.source);
        let glyph_ink_bounds =
            glyph_ink_bounds_for_rendered_line(&self.font_system, &rendered_line);
        rendered_line = rendered_line.with_glyph_ink_bounds(glyph_ink_bounds);
        let mut decoration_style = group.style.clone();
        let (decoration_baseline, decoration_width) =
            if let Some(rect) = group.decoration_paint_rect {
                match group.style.writing_mode {
                    WritingMode::HorizontalTb => {
                        decoration_style.font_size = rect.height().max(1.0);
                        (rect.origin, rect.width())
                    }
                    WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr => {
                        decoration_style.font_size = rect.width().max(1.0);
                        (rect.origin, rect.height())
                    }
                }
            } else {
                (PaintPoint::new(group.x(), group.y()), group.width())
            };
        // A selected line edge is a glyph-sequence edge, not the fitted line
        // measure.  In particular, `break-spaces` retains trailing advances
        // for painting after layout has excluded them from the fitting width.
        // Use the exact shaped run span so skip-spaces can remove only the
        // leading/trailing spacers instead of clipping a final text glyph.
        // <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>
        let decoration_width = decoration_width.max(rendered_text_line_width(&rendered_line));
        let source_inline_length = group.width().max(group.shaped.advance_width());
        let receiver_scale = (decoration_width / source_inline_length).max(0.0);
        let decoration_inline_start = match group.style.writing_mode {
            WritingMode::HorizontalTb => decoration_baseline.x,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => decoration_baseline.y,
        };
        let decoration_receivers = group
            .decoration_provenance
            .iter()
            .flat_map(|segment| {
                segment.receivers.iter().map(move |receiver| {
                    let mut style = receiver.style.clone();
                    if group.decoration_paint_rect.is_some() {
                        style.font_size = decoration_style.font_size;
                    }
                    PreparedTextDecorationReceiverPaint {
                        inline_span: TextInlineSpan::new(
                            decoration_inline_start + receiver.inline_span.start * receiver_scale,
                            decoration_inline_start + receiver.inline_span.end * receiver_scale,
                        ),
                        style,
                        layers: segment.layers.clone(),
                    }
                })
            })
            .collect();
        let opaque_text_coverage_paths = self.font_system.full_em_rect_glyph_coverage_paths(
            text_origin,
            &rendered_line.runs,
            group.style.text_fill_color.unwrap_or(group.style.color),
        );
        let link = group
            .link_target
            .as_ref()
            .map(|target| RenderedLink::from_paint_rect(group.link_paint_rect(), target.clone()));
        Some(PreparedInlineTextPaint {
            rendered_line,
            glyph_paths: color_glyph_paths,
            glyph_images: raster_glyph_images,
            opaque_text_coverage_paths,
            decorations: PreparedTextDecorationPaint {
                baseline: decoration_baseline,
                receivers: decoration_receivers,
            },
            text_style: group.style.clone(),
            link,
        })
    }

    /// Emit one CSS paint phase from a previously materialized text group.
    fn paint_prepared_inline_text_paint_phase(
        &mut self,
        prepared: &mut PreparedInlineTextPaint,
        phase: InlineTextPaintPhase,
        _decoration_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        match phase {
            InlineTextPaintPhase::Shadows => {
                self.paint_text_shadows(&prepared.rendered_line, &prepared.text_style);
            }
            InlineTextPaintPhase::BeforeTextDecorations => {
                unreachable!("line decoration phases are scheduled by origin")
            }
            InlineTextPaintPhase::Glyphs => {
                for path in std::mem::take(&mut prepared.glyph_paths) {
                    self.push_path_in_band(PaintBand::Inline, path);
                }
                for image in std::mem::take(&mut prepared.glyph_images) {
                    self.push_image_in_band(PaintBand::Inline, image);
                }
                let coverage_paths = std::mem::take(&mut prepared.opaque_text_coverage_paths);
                if coverage_paths.is_empty() {
                    self.push_line_in_band(PaintBand::Inline, prepared.rendered_line.clone());
                } else {
                    self.current_page.push_opaque_text_coverage_in_band(
                        PaintBand::Inline,
                        prepared.rendered_line.clone(),
                        coverage_paths,
                    );
                }
            }
            InlineTextPaintPhase::Emphasis => {
                self.paint_prepared_text_emphasis_marks_for_line(
                    &prepared.rendered_line,
                    &prepared.text_style,
                );
            }
            InlineTextPaintPhase::AfterTextDecorations => {
                unreachable!("line decoration phases are scheduled by origin")
            }
            InlineTextPaintPhase::Links => {
                if let Some(link) = prepared.link.take() {
                    self.current_page.push_link(link);
                }
            }
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
            || (fragment
                .style()
                .background
                .background_color
                .is_transparent()
                && fragment.style().background.background_image.is_none()
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

/// Restore the source stream for a layout-only `word-space-transform` word.
///
/// Its shaped glyphs deliberately remain the replacement space so CSS layout,
/// paint, and decoration use its advance. PDF `/ActualText` instead exposes
/// the source U+200B or no text for HTML `<wbr>`.
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>
fn apply_word_space_transform_actual_text(line: &mut RenderedLine, source: InlineTextSource) {
    let InlineTextSource::WordSpaceTransform(separator) = source else {
        return;
    };
    let actual_text = Rc::<str>::from(separator.extraction_text().unwrap_or(""));
    for run in &mut line.runs {
        run.actual_text = Some(Rc::from(""));
    }
    if let Some(first) = line.runs.first_mut() {
        first.actual_text = Some(actual_text);
    }
}

/// Return the page-space union of the selected glyph outlines for one line.
///
/// CSS fragmentation keeps the whole line box together, but PDF overflow
/// clipping must act on painted ink.  Retaining this outline union lets a
/// fragment edge avoid becoming a synthetic clip edge for glyphs that are
/// already wholly within the fragmentainer.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
fn glyph_ink_bounds_for_rendered_line(
    font_system: &crate::text::FontSystem,
    line: &RenderedLine,
) -> Option<PaintClip> {
    let boxes = font_system.glyph_ink_boxes_for_runs(&line.runs, line.y());
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::INFINITY;
    let mut top = f32::NEG_INFINITY;
    for ink in boxes {
        left = left.min(line.x() + ink.x_min);
        right = right.max(line.x() + ink.x_max);
        bottom = bottom.min(ink.y_min);
        top = top.max(ink.y_max);
    }
    (left.is_finite() && right > left && bottom.is_finite() && top > bottom)
        .then(|| PaintClip::new(left, bottom, right - left, top - bottom))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::TextDecorationLayer;

    fn line() -> RenderedLine {
        RenderedLine::new(
            " ".into(),
            0.0,
            0.0,
            12.0,
            None,
            CssColor::BLACK,
            vec![RenderedTextRun {
                text: Rc::from(" "),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: None,
                font_palette: crate::css::FontPalette::Normal,
                glyphs: None,
                glyph_source_ranges: None,
            }],
        )
    }

    #[test]
    fn word_space_transform_restores_authored_zero_width_space_for_extraction() {
        let mut line = line();
        apply_word_space_transform_actual_text(
            &mut line,
            InlineTextSource::WordSpaceTransform(
                ExplicitWordSeparatorSource::AuthoredZeroWidthSpace,
            ),
        );
        assert_eq!(line.text, " ");
        assert_eq!(line.runs[0].actual_text.as_deref(), Some("\u{200b}"));
    }

    #[test]
    fn word_space_transform_omits_generated_wbr_from_extraction() {
        let mut line = line();
        apply_word_space_transform_actual_text(
            &mut line,
            InlineTextSource::WordSpaceTransform(ExplicitWordSeparatorSource::HtmlWbr),
        );
        assert_eq!(line.text, " ");
        assert_eq!(line.runs[0].actual_text.as_deref(), Some(""));
    }

    #[test]
    fn global_text_paint_phases_order_decorations_around_glyphs() {
        assert_eq!(
            InlineTextPaintPhase::ORDER,
            [
                InlineTextPaintPhase::Shadows,
                InlineTextPaintPhase::BeforeTextDecorations,
                InlineTextPaintPhase::Glyphs,
                InlineTextPaintPhase::Emphasis,
                InlineTextPaintPhase::AfterTextDecorations,
                InlineTextPaintPhase::Links,
            ]
        );
    }

    #[test]
    fn propagated_decoration_origin_is_received_when_local_longhand_is_initial() {
        let mut origin_style = ComputedStyle::initial();
        origin_style.text_decoration.underline = true;
        let origin_style = Rc::new(origin_style);
        let mut receiver_style = ComputedStyle::initial();

        assert!(!receiver_style.text_decoration.has_visible_line());
        receiver_style
            .text_decoration_layers
            .push(TextDecorationLayer {
                decoration: origin_style.text_decoration.clone(),
                origin_style: Rc::clone(&origin_style),
            });

        assert!(text_decoration_layers_receive_origin(
            &receiver_style.text_decoration_layers,
            &origin_style
        ));
    }

    #[test]
    fn equal_looking_decoration_origins_remain_distinct_by_identity() {
        let mut declaration = ComputedStyle::initial();
        declaration.text_decoration.underline = true;
        let outer_origin = Rc::new(declaration.clone());
        let inner_origin = Rc::new(declaration);
        let mut receiver_style = ComputedStyle::initial();
        receiver_style.text_decoration_layers = vec![
            TextDecorationLayer {
                decoration: outer_origin.text_decoration.clone(),
                origin_style: Rc::clone(&outer_origin),
            },
            TextDecorationLayer {
                decoration: inner_origin.text_decoration.clone(),
                origin_style: Rc::clone(&inner_origin),
            },
        ];

        assert!(!Rc::ptr_eq(&outer_origin, &inner_origin));
        assert!(text_decoration_layers_receive_origin(
            &receiver_style.text_decoration_layers,
            &outer_origin
        ));
        assert!(text_decoration_layers_receive_origin(
            &receiver_style.text_decoration_layers,
            &inner_origin
        ));
    }
}
