use super::*;
use crate::layout::text_paint::{
    TextDecorationInlineAxis, TextDecorationLogicalInlineRange, TextDecorationPositionedInkBox,
};

impl<'a> LayoutBuilder<'a> {
    /// Paint a prepared inline line without reshaping text.
    ///
    /// PDF text and CSS decoration emission consume the shaped glyph runs
    /// stored during line preparation, keeping fallback fonts and glyph
    /// advances stable after line fitting:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(in crate::layout) fn paint_prepared_inline_line(&mut self, line: &PreparedInlineLine) {
        self.paint_prepared_inline_line_with_text_source(line, None);
    }

    pub(in crate::layout) fn paint_prepared_inline_line_with_text_source(
        &mut self,
        line: &PreparedInlineLine,
        text_source: Option<RenderedLineSource>,
    ) {
        debug_assert!(line.metrics.height.is_finite());
        let decoration_geometries = self.prepared_line_text_decoration_geometries(line);
        let mut phaseable_text_groups = Vec::new();
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.rect.paint_rect(),
                    );
                }
                PreparedInlinePaintItem::TextGroup(group) => {
                    let has_paint_effect = if group.paint_scope_ancestry.is_empty() {
                        group.style.opacity.value() < 1.0
                    } else {
                        group.paint_opacity < 1.0
                    } || group.positioned_paint_style.is_some();
                    if has_paint_effect {
                        if !phaseable_text_groups.is_empty() {
                            self.paint_prepared_inline_text_groups_in_phases(
                                &phaseable_text_groups,
                                text_source,
                                &decoration_geometries,
                            );
                            phaseable_text_groups.clear();
                        }
                        if let Some(source) = text_source {
                            self.paint_prepared_inline_text_group_with_source_and_decoration_geometries(
                                group,
                                source,
                                &decoration_geometries,
                            );
                        } else {
                            self.paint_prepared_inline_text_group_with_decoration_geometries(
                                group,
                                &decoration_geometries,
                            );
                        }
                    } else {
                        phaseable_text_groups.push(group);
                    }
                }
                PreparedInlinePaintItem::Atom(atom) => {
                    if !phaseable_text_groups.is_empty() {
                        self.paint_prepared_inline_text_groups_in_phases(
                            &phaseable_text_groups,
                            text_source,
                            &decoration_geometries,
                        );
                        phaseable_text_groups.clear();
                    }
                    self.paint_prepared_inline_atom(atom);
                }
            }
        }
        if !phaseable_text_groups.is_empty() {
            self.paint_prepared_inline_text_groups_in_phases(
                &phaseable_text_groups,
                text_source,
                &decoration_geometries,
            );
        }
    }

    /// Select one considered-text geometry for every decoration origin on a
    /// prepared line.
    ///
    /// Decorations propagate through eligible in-flow descendants, but CSS
    /// Text Decoration requires a decorating box to use one uniform position
    /// and thickness for all of its selected text on a line.  The prepared
    /// line is the first point where all shaped descendants and their physical
    /// baselines coexist, so collection belongs here rather than in an
    /// individual text-group painter.
    ///
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    /// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-line-uniformity>
    fn prepared_line_text_decoration_geometries(
        &mut self,
        line: &PreparedInlineLine,
    ) -> Vec<TextDecorationOriginLineGeometry> {
        let mut geometries: Vec<TextDecorationOriginLineGeometry> = Vec::new();
        for item in &line.paint_items {
            let PreparedInlinePaintItem::TextGroup(group) = item else {
                continue;
            };
            // A group can retain its shaped glyph advance while its fitted
            // paint bounds collapse at an inline-fragment boundary. Line
            // decorations still cover that text, so use the shaped advance
            // as the non-empty geometry criterion and coverage span.
            if group.width().max(group.shaped.advance_width()) <= 0.0 {
                continue;
            }
            if group.decoration_provenance.is_empty() {
                continue;
            }
            let mut reference = group
                .decoration_paint_rect
                .map(|rect| rect.origin)
                .unwrap_or_else(|| PaintPoint::new(group.x(), group.y()));
            if let (Some(rect), Some(inline_axis)) = (
                group.decoration_paint_rect,
                VerticalInlineAxis::for_style(&group.style),
            ) {
                reference.y = inline_axis.logical_start_for_paint_rect(rect).y();
            }
            let axis = if group.style.writing_mode == WritingMode::HorizontalTb {
                TextDecorationStrokeAxis::Horizontal
            } else {
                TextDecorationStrokeAxis::Vertical
            };
            let mut glyph_runs =
                positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
            self.align_sideways_runs_to_vertical_line_box(
                &mut glyph_runs,
                &group.shaped,
                &group.style,
                group.line_block_size,
            );
            let positioned_ink_boxes = self
                .font_system
                .glyph_ink_boxes_for_runs(&glyph_runs, reference.y)
                .into_iter()
                .map(|ink| TextDecorationPositionedInkBox {
                    x_min: reference.x + ink.x_min,
                    x_max: reference.x + ink.x_max,
                    y_min: ink.y_min,
                    y_max: ink.y_max,
                })
                .collect::<Vec<_>>();
            for provenance in &group.decoration_provenance {
                for receiver in &provenance.receivers {
                    if receiver.style.visibility != Visibility::Visible {
                        continue;
                    }
                    let coverage = TextDecorationLineGlyphCoverage {
                        span: match receiver.style.writing_mode {
                            WritingMode::HorizontalTb => TextInlineSpan::new(
                                reference.x + receiver.inline_span.start,
                                reference.x + receiver.inline_span.end,
                            ),
                            WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr => {
                                VerticalInlineAxis::for_style(&receiver.style)
                                    .expect("vertical writing modes have a vertical inline axis")
                                    .project_span_from_start(
                                        layout_pt(reference.y),
                                        receiver.inline_span,
                                    )
                            }
                        },
                    };
                    let positioned_glyphs = text_decoration_positioned_glyphs(
                        axis,
                        reference.x,
                        reference.y,
                        coverage.span.start,
                        coverage.span.length(),
                        &glyph_runs,
                    );
                    let font_id = self.font_system.resolve_style(&receiver.style);
                    let metrics = self
                        .font_system
                        .text_decoration_metrics(font_id, &receiver.style);
                    for decoration in &provenance.layers {
                        let participates = (decoration.decoration.underline
                            && !text_decoration_skip_self_suppresses(
                                &receiver.style,
                                TextDecorationLineKind::Underline,
                            ))
                            || (decoration.decoration.overline
                                && !text_decoration_skip_self_suppresses(
                                    &receiver.style,
                                    TextDecorationLineKind::Overline,
                                ))
                            || (decoration.decoration.line_through
                                && !text_decoration_skip_self_suppresses(
                                    &receiver.style,
                                    TextDecorationLineKind::LineThrough,
                                ));
                        if !participates {
                            continue;
                        }
                        let origin_fragment = line
                            .decoration_origin_fragments
                            .iter()
                            .find(|fragment| {
                                Rc::ptr_eq(&fragment.origin_style, &decoration.origin_style)
                            })
                            .cloned()
                            // Direct record preparation is also used by a
                            // few isolated layout tests. Those records do
                            // not have an enclosing line sequence from
                            // which to derive fragment geometry.
                            .unwrap_or_else(|| TextDecorationOriginFragmentGeometry {
                                origin_style: Rc::clone(&decoration.origin_style),
                                complete_inline_range: TextDecorationLogicalInlineRange::from_edges(
                                    layout_pt(0.0),
                                    layout_pt(coverage.span.length()),
                                ),
                                fragment_inline_range: TextDecorationLogicalInlineRange::from_edges(
                                    layout_pt(0.0),
                                    layout_pt(coverage.span.length()),
                                ),
                                is_first_fragment: true,
                                is_last_fragment: true,
                            });
                        if let Some(existing) = geometries.iter_mut().find(|existing| {
                            Rc::ptr_eq(&existing.layer.origin_style, &decoration.origin_style)
                        }) {
                            // The selected text with the largest em box is the
                            // conservative shared metric source: it keeps automatic
                            // decorations clear of every eligible descendant rather
                            // than letting a later, smaller receiver pull the common
                            // line through it.  The physical outside reference is
                            // likewise the furthest text-under edge of the selected
                            // line in each writing-axis projection.
                            if receiver.style.font_size > existing.geometry.considered_font_size {
                                existing.geometry.considered_font_size = receiver.style.font_size;
                                existing.geometry.considered_metrics = metrics;
                            }
                            existing.selected_inline_span = Some(
                                existing
                                    .selected_inline_span
                                    .map(|span| {
                                        TextInlineSpan::new(
                                            span.start.min(coverage.span.start),
                                            span.end.max(coverage.span.end),
                                        )
                                    })
                                    .unwrap_or(coverage.span),
                            );
                            existing.receiver_spans.push(coverage.span);
                            existing
                                .glyph_sequence
                                .glyphs
                                .extend(positioned_glyphs.iter().cloned());
                            existing
                                .positioned_ink_boxes
                                .extend(positioned_ink_boxes.iter().copied());
                            match receiver.style.writing_mode {
                                WritingMode::HorizontalTb => {
                                    existing.line_reference.y =
                                        existing.line_reference.y.min(reference.y);
                                }
                                WritingMode::VerticalRl
                                | WritingMode::VerticalLr
                                | WritingMode::SidewaysRl
                                | WritingMode::SidewaysLr => {
                                    existing.line_reference.x =
                                        existing.line_reference.x.min(reference.x);
                                }
                            }
                            continue;
                        }
                        geometries.push(TextDecorationOriginLineGeometry {
                            layer: decoration.clone(),
                            geometry: TextDecorationLineGeometry::from_origin_and_considered_text(
                                decoration.origin_style.as_ref(),
                                &receiver.style,
                                metrics,
                            ),
                            origin_inline_axis: TextDecorationInlineAxis::for_style(
                                decoration.origin_style.as_ref(),
                            ),
                            selected_inline_span: Some(coverage.span),
                            receiver_spans: vec![coverage.span],
                            glyph_sequence: TextDecorationLineGlyphSequence {
                                glyphs: positioned_glyphs.clone(),
                            },
                            positioned_ink_boxes: positioned_ink_boxes.clone(),
                            line_reference: reference,
                            origin_fragment,
                        });
                    }
                }
            }
        }
        for geometry in &mut geometries {
            geometry.glyph_sequence.glyphs.sort_by(|left, right| {
                left.inline_start
                    .partial_cmp(&right.inline_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        geometries
    }
}
