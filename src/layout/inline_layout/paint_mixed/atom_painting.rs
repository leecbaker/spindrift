use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Paint one prepared atomic inline box.
    ///
    /// CSS Inline treats replaced and inline-block descendants as atomic
    /// inline-level boxes. The prepared atom stores the resolved content box so
    /// painting does not recompute line positioning:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
    pub(in crate::layout) fn paint_prepared_inline_atom(&mut self, prepared: &PreparedInlineAtom) {
        let atom = &prepared.atom;
        if atom.style().visibility != Visibility::Visible {
            return;
        }
        if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() {
            self.paint_prepared_inline_box_edge(prepared, *edge);
            self.replay_escaped_inline_atom_positioned_layers(prepared);
            return;
        }
        if matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) {
            self.replay_escaped_inline_atom_positioned_layers(prepared);
            return;
        }
        let checkpoint = self.current_page.paint_checkpoint();
        if let Some(marker) = atom.outside_marker() {
            let borders = used_border_widths(atom.style());
            let text_top = prepared.border_box.y() + prepared.border_box.height()
                - borders.top
                - atom.style().padding.top;
            let formatted_line_block_start = PageTopBlockPosition::new(text_top);
            let fallback_baseline_offset =
                self.inline_box_text_line_layout_baseline_offset(atom.style());
            self.paint_outside_marker(
                marker,
                atom.style(),
                OutsideMarkerAnchor {
                    principal_line_inline_span: PageInlineSpan::from_edges(
                        prepared.border_box.x() + borders.left,
                        prepared.border_box.x() + prepared.border_box.width() - borders.right,
                    ),
                    formatted_line_block_start,
                    alphabetic_baseline: formatted_line_block_start
                        .toward_block_end(layout_pt(fallback_baseline_offset)),
                },
            );
        }
        self.paint_prepared_inline_atom_contents(prepared);
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if !fragment.is_empty() {
            let bounds = prepared.border_box.paint_clip();
            let mut policy = match atom.content() {
                // SVG 2 requires an embedded outermost SVG element to be an
                // isolated, atomic stacking context.  Its box decorations
                // and rendered SVG scene have already been recorded in this
                // fragment in source paint order; the policy gives that
                // semantic group a single compositing boundary.
                // <https://www.w3.org/TR/SVG2/render.html#EstablishingANewStackingContext>
                InlineAtomContent::Svg { .. } => StackingContextPolicy::for_inline_svg_root(
                    atom.style(),
                    PaintBand::Inline,
                    bounds,
                ),
                // A non-atomic inline's `InlineBox` atom is only a retained
                // line-layout sequence. Do not give it the atomic replay
                // policy: that would retain a negative positioned descendant
                // inside the inline's own background fragment.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                InlineAtomContent::InlineBox { .. }
                    if !property_containment_applies_to_style(atom.style()) =>
                {
                    StackingContextPolicy::for_non_positioned_style_effect(atom.style(), bounds)
                }
                _ => StackingContextPolicy::for_atomic(atom.style(), PaintBand::Inline, bounds),
            };
            // A replaced atom's CSS content clip is attached directly to its
            // image/SVG primitive.  The atomic context also contains the
            // principal decoration, so a generic padding-box overflow effect
            // here would both clip that decoration and introduce a duplicate
            // rectangular raster edge before the exact contour. Captured
            // formatting contexts, including inline tables, do not have that
            // primitive-owned contour and must retain their own CSS overflow
            // clip around the replayed descendant fragment.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            if matches!(
                atom.content(),
                InlineAtomContent::Canvas
                    | InlineAtomContent::Iframe(_)
                    | InlineAtomContent::Image(_)
                    | InlineAtomContent::Gradient { .. }
                    | InlineAtomContent::Svg { .. }
                    | InlineAtomContent::InlineFragment {
                        contents_overflow_clip_applied: true,
                        ..
                    }
            ) {
                policy.effects.clear_overflow_clip_effects();
            }
            // Preserve the atom's original stack level through this atomic
            // replay boundary.  `escaped_positioned_layers` later extracts
            // such contexts and uses their level to insert them into the
            // nearest real parent stacking context; rebuilding it as `auto`
            // would incorrectly move a negative descendant above the
            // parent's in-flow inline paint.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                Vec::new(),
            )
            .with_source_order(self.next_paint_source_order())
            .with_effects(policy.effects)
            .with_bounds(bounds);
            let fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
        self.replay_escaped_inline_atom_positioned_layers(prepared);
    }

    /// Replays positioned descendants attached to an inline source atom.
    ///
    /// A positioned inline's start edge may be the only selected fragment
    /// that owns its descendants.  Inline-edge atoms do not otherwise paint
    /// content, but must still replay those layers.
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
    fn replay_escaped_inline_atom_positioned_layers(&mut self, prepared: &PreparedInlineAtom) {
        if let Some(layers) = prepared.atom.escaped_positioned_layers() {
            for layer in layers.iter() {
                let atom_offset = layer
                    .escaped_atom_translation
                    .atom_offset(prepared.border_box.x(), prepared.border_box.y());
                let mut layer = layer.clone().translated(atom_offset);
                layer.page_index = layer
                    .escaped_atom_translation
                    .replay_page_index(self.pages.len(), layer.page_index);
                // The atomic inline was measured on a scratch page, so a
                // descendant's original source order predates the atom's
                // final normal-flow paint.  Its containing block is a
                // `z-index:auto` positioned inline-block, which means the
                // descendant belongs in the enclosing stacking context's
                // auto/zero phase after that normal-flow paint.  Reserve its
                // order at replay, rather than retaining the scratch cursor.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                layer.context.source_order = self.next_paint_source_order();
                self.positioned_layers.push(layer);
            }
        }
    }

    fn paint_inline_atom_box_background(&mut self, border_rect: PaintRect, style: &ComputedStyle) {
        for primitive in self.box_background_primitives(border_rect, style) {
            // The atomic inline context itself is inserted in its parent's
            // inline phase. Within that context, its own decoration remains
            // before its in-flow block descendants.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }
    }

    pub(in crate::layout) fn paint_prepared_inline_atom_contents(
        &mut self,
        prepared: &PreparedInlineAtom,
    ) {
        let atom = &prepared.atom;
        let content_x = prepared.border_box.x();
        let y = prepared.border_box.y();
        let content_width = prepared.border_box.width();
        let content_height = prepared.border_box.height();
        if !matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(_)
                | InlineAtomContent::Leader(_)
                | InlineAtomContent::StaticPositionPlaceholder
        ) && (atom
            .style()
            .background
            .background_color
            .is_potentially_visible()
            || atom.style().background.background_image.is_image()
            || used_border_width(atom.style()) > layout_pt(0.0))
        {
            self.paint_inline_atom_box_background(
                paint_space_rect(content_x, y, content_width, content_height),
                atom.style(),
            );
        }
        match atom.content() {
            InlineAtomContent::InlineEdge(_)
            | InlineAtomContent::Leader(_)
            | InlineAtomContent::StaticPositionPlaceholder => {}
            InlineAtomContent::Canvas => {}
            InlineAtomContent::Iframe(element_id) => {
                let Some(document) = self.iframe_documents.get(element_id) else {
                    return;
                };
                let Some(page) = document.pages.first() else {
                    return;
                };
                let borders = used_border_widths(atom.style());
                let iframe_x = content_x + borders.left + atom.style().padding.left;
                let iframe_y = y + borders.bottom + atom.style().padding.bottom;
                let iframe_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(0.0);
                let iframe_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style().padding.top
                    - atom.style().padding.bottom)
                    .max(0.0);
                let clip = PaintClip::from_paint_rect(paint_space_rect(
                    iframe_x,
                    iframe_y,
                    iframe_width,
                    iframe_height,
                ));
                let mut fragment = page.paint_fragment().translated(PaintTranslation::new(
                    iframe_x,
                    iframe_y + iframe_height - page.height(),
                ));
                fragment.promote_page_background_to_in_flow_block();
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
                // An embedded page background is still child browsing-context
                // paint. Keep it in the iframe viewport along with the
                // translated child scroll contents.
                // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
                fragment = fragment.with_effect_scoped_to_rect_all_bands(clip);
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
            InlineAtomContent::Image(decoded) => {
                let borders = used_border_widths(atom.style());
                let overflow = ReplacedObjectOverflow::from_style(atom.style());
                let content_contour = replaced_content_contour(
                    paint_space_rect(content_x, y, content_width, content_height),
                    atom.style(),
                    borders,
                );
                let image_x = content_x + borders.left + atom.style().padding.left;
                let image_y = y + borders.bottom + atom.style().padding.bottom;
                let image_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(0.0);
                let image_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style().padding.top
                    - atom.style().padding.bottom)
                    .max(0.0);
                let mut image = RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, image_width, image_height),
                    false,
                    decoded.pixel_size.width,
                    decoded.pixel_size.height,
                    decoded.source_rect,
                    raster_image_sampling(atom.style()),
                    decoded.rgb.shared(),
                    decoded.alpha.clone(),
                    atom.alt_text().map(Rc::from),
                )
                .with_raster_color_space(decoded.color_space.clone())
                .with_image_id(decoded.image_id);
                if let Some(clip) = content_contour
                    .as_ref()
                    .and_then(ResolvedBoxContentClip::path_clip)
                {
                    image = image.with_clip(clip);
                }
                if apply_object_fit(
                    &mut image,
                    decoded.natural_layout_size(),
                    atom.style().object_fit,
                    atom.style().object_position.clone(),
                    atom.style().object_view_box.clone(),
                    overflow,
                    atom.style().effective_zoom,
                ) {
                    self.push_image_in_band(PaintBand::Inline, image);
                }
            }
            InlineAtomContent::Gradient { image, fallback } => {
                let borders = used_border_widths(atom.style());
                let overflow = ReplacedObjectOverflow::from_style(atom.style());
                let image_x = content_x + borders.left + atom.style().padding.left;
                let image_y = y + borders.bottom + atom.style().padding.bottom;
                let image_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style().padding.left
                    - atom.style().padding.right)
                    .max(0.0);
                let image_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style().padding.top
                    - atom.style().padding.bottom)
                    .max(0.0);
                let paint_rect = paint_space_rect(image_x, image_y, image_width, image_height);
                if atom.style().object_fit == css::ObjectFit::Fill
                    && matches!(atom.style().object_view_box, css::ObjectViewBox::None)
                    && let Some(primitive) = native_generated_gradient_primitive(
                        image,
                        paint_rect,
                        atom.style().color,
                        None,
                    )
                {
                    self.push_primitive_in_band(PaintBand::Inline, primitive);
                } else {
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_rect,
                        false,
                        fallback.pixel_size.width,
                        fallback.pixel_size.height,
                        fallback.source_rect,
                        raster_image_sampling(atom.style()),
                        fallback.rgb.shared(),
                        fallback.alpha.clone(),
                        atom.alt_text().map(Rc::from),
                    )
                    .with_raster_color_space(fallback.color_space.clone())
                    .with_image_id(fallback.image_id);
                    if apply_object_fit(
                        &mut rendered,
                        fallback.natural_layout_size(),
                        atom.style().object_fit,
                        atom.style().object_position.clone(),
                        atom.style().object_view_box.clone(),
                        overflow,
                        atom.style().effective_zoom,
                    ) {
                        self.push_image_in_band(PaintBand::Inline, rendered);
                    }
                }
            }
            InlineAtomContent::Svg { asset } => {
                if let Some(asset) = asset {
                    let borders = used_border_widths(atom.style());
                    let border_rect = paint_space_rect(content_x, y, content_width, content_height);
                    let overflow_edge = resolve_overflow_clip_edge(
                        border_rect,
                        atom.style(),
                        borders,
                        UsedOverflowAxes::from_svg_viewport_style(atom.style()),
                        atom.style().contain.paint,
                        None,
                    );
                    let svg_x = content_x + borders.left + atom.style().padding.left;
                    let svg_y = y + borders.bottom + atom.style().padding.bottom;
                    let svg_width = (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0);
                    let svg_height = (content_height
                        - borders.top
                        - borders.bottom
                        - atom.style().padding.top
                        - atom.style().padding.bottom)
                        .max(0.0);
                    // Inline SVG follows the same concrete-object and source
                    // selection path as block replaced SVG. Unlike an image
                    // asset, however, an embedded SVG's root viewport obeys
                    // the element's computed CSS overflow.
                    if svg_width > 0.0 && svg_height > 0.0 {
                        let group = svg_replaced_group_with_overflow_clip(
                            asset,
                            paint_space_rect(svg_x, svg_y, svg_width, svg_height),
                            atom.style().object_fit,
                            atom.style().object_position.clone(),
                            atom.style().object_view_box.clone(),
                            overflow_edge.as_ref(),
                        );
                        self.push_svg_group_in_band(PaintBand::Inline, group);
                    }
                }
            }
            InlineAtomContent::InlineBox { sequence } => {
                let borders = used_border_widths(atom.style());
                let text_top = y + content_height - borders.top - atom.style().padding.top;
                let text_x = content_x
                    + borders.left
                    + atom.style().padding.left
                    + atom.content_inline_offset();
                let text_available_width = atom.content_inline_paint_width().unwrap_or_else(|| {
                    (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0)
                });
                self.paint_inline_box_sequence(
                    sequence,
                    atom.style(),
                    text_x,
                    text_available_width,
                    text_top,
                );
            }
            InlineAtomContent::Ruby {
                base,
                annotations,
                annotation_sides,
                base_block_size,
                annotation_block_sizes,
                ..
            } => {
                let borders = used_border_widths(atom.style());
                let text_x = content_x
                    + borders.left
                    + atom.style().padding.left
                    + atom.content_inline_offset();
                let text_available_width = atom.content_inline_paint_width().unwrap_or_else(|| {
                    (content_width
                        - borders.left
                        - borders.right
                        - atom.style().padding.left
                        - atom.style().padding.right)
                        .max(0.0)
                });
                let ruby_origin = ruby::RubyPaintOrigin::new(text_x, y);
                let resolved_placement = atom.ruby_placement();
                // Paint the base at the ruby atom's logical under side. Each
                // annotation level is stacked toward its logical over side;
                // the horizontal coordinates are a backend boundary, while
                // the nested line sequences retain their own writing mode.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-position>
                // The initial value of `ruby-align` is `space-around`. For a
                // single base/annotation run, its used result is centering the
                // shorter run in the paired column. Multi-base distribution
                // is represented by the normalized column spans and is
                // completed before this paint boundary.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
                let base_x = resolved_placement.map_or_else(
                    || {
                        ruby_origin.inline()
                            + (text_available_width - base.paint_inline_size.points()).max(0.0)
                                / 2.0
                    },
                    |placement| ruby_origin.inline() + placement.base_inline_offset.points(),
                );
                let all_annotations_are_under = annotation_sides
                    .iter()
                    .all(|side| *side == css::RubyAnnotationSide::Under);
                self.paint_inline_box_sequence(
                    &base.sequence,
                    &base.style,
                    base_x,
                    text_available_width,
                    ruby_origin.block_offset(ruby::RubyBlockExtent::new(
                        if all_annotations_are_under {
                            annotation_block_sizes.iter().sum::<f32>() + *base_block_size
                        } else {
                            *base_block_size
                        },
                    )),
                );
                // The inline-sequence paint boundary is expressed in the
                // page's bottom-origin block coordinate. The first (closest)
                // annotation level therefore starts at the base level's
                // block-start edge; each subsequent level advances toward the
                // logical over side by the preceding annotation extent.
                // Keep this entirely within the ruby atom: annotations are
                // not ordinary parent-line children.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-position>
                let base_paint_top =
                    ruby_origin.block_offset(ruby::RubyBlockExtent::new(*base_block_size));
                let mut over_annotation_baseline = base_paint_top;
                let mut under_annotation_baseline =
                    ruby_origin.block_offset(ruby::RubyBlockExtent::default());
                for (annotation_index, ((annotation, annotation_block_size), side)) in annotations
                    .iter()
                    .zip(annotation_block_sizes)
                    .zip(annotation_sides)
                    .enumerate()
                {
                    let annotation_available_width =
                        if annotation.starts_span && annotation.column_span > 1 {
                            annotation.containing_inline_size.points()
                        } else {
                            text_available_width
                        };
                    let annotation_x = resolved_placement.map_or_else(
                        || {
                            ruby_origin.inline()
                                + (annotation_available_width
                                    - annotation.paint_inline_size.points())
                                .max(0.0)
                                    / 2.0
                        },
                        |placement| {
                            ruby_origin.inline()
                                + placement
                                    .annotation_inline_offsets
                                    .get(annotation_index)
                                    .copied()
                                    .unwrap_or(ruby::RubyInlineDisplacement::ZERO)
                                    .points()
                        },
                    );
                    // The captured sequence's first visible glyph may have a
                    // different ascent from the annotation line box (for
                    // example Ahem). Reconcile that glyph baseline with the
                    // line box before placing it at the ruby-level boundary.
                    let annotation_line_box_baseline = self
                        .font_system
                        .rendered_first_line_baseline_offset(&annotation.style)
                        .points();
                    let annotation_baseline = match side {
                        css::RubyAnnotationSide::Over => {
                            let baseline = over_annotation_baseline;
                            over_annotation_baseline += *annotation_block_size;
                            baseline
                        }
                        css::RubyAnnotationSide::Under => {
                            let baseline = under_annotation_baseline;
                            under_annotation_baseline += *annotation_block_size;
                            baseline
                        }
                    };
                    let annotation_block_top = annotation_baseline + annotation_line_box_baseline;
                    // A ruby-text container owns a generated principal box;
                    // its child sequence retains only descendant fragments.
                    // Paint its decoration even when no direct text fragment
                    // exists, such as an `rt` containing a positioned span.
                    // <https://drafts.csswg.org/css-ruby-1/#ruby-text-container>
                    let annotation_height = annotation_block_size
                        .max(annotation.style.line_height)
                        .max(0.0);
                    let annotation_width = annotation.paint_inline_size.points().max(0.0);
                    if annotation.style.visibility == Visibility::Visible
                        && annotation_width > 0.0
                        && annotation_height > 0.0
                    {
                        for primitive in self.box_background_primitives(
                            paint_space_rect(
                                annotation_x,
                                annotation_block_top - annotation_height,
                                annotation_width,
                                annotation_height,
                            ),
                            &annotation.style,
                        ) {
                            self.push_primitive_in_band(PaintBand::Inline, primitive);
                        }
                    }
                    self.paint_inline_box_sequence(
                        &annotation.sequence,
                        &annotation.style,
                        annotation_x,
                        annotation_available_width,
                        annotation_block_top,
                    );
                }
            }
            InlineAtomContent::TextCombineUpright {
                sequence,
                horizontal_style,
                inline_scale,
            } => {
                self.paint_text_combine_upright(
                    sequence,
                    horizontal_style,
                    *inline_scale,
                    prepared.border_box,
                );
            }
            InlineAtomContent::InlineFragment {
                fragment,
                replay_coordinates,
                table_cell_context,
                ..
            } => {
                if let Some(context) = table_cell_context {
                    // The fragment is already normalized to its atomic
                    // border box, but preserve the originating table-cell
                    // coordinate context through replay. This keeps a later
                    // writing-mode-aware fragment projection from guessing
                    // at the enclosing inline line's flow.
                    debug_assert!(
                        context.origin.x().is_finite() && context.origin.top_y().is_finite()
                    );
                    debug_assert!(matches!(
                        context.writing_mode,
                        WritingMode::HorizontalTb
                            | WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr
                    ));
                    debug_assert!(matches!(context.direction, Direction::Ltr | Direction::Rtl));
                }
                self.current_page.append_paint_fragment(
                    fragment,
                    replay_coordinates.replay_translation(prepared.border_box),
                );
            }
        }
        for primitive in self.box_outline_primitives(
            paint_space_rect(content_x, y, content_width, content_height),
            atom.style(),
        ) {
            self.push_primitive_in_band(PaintBand::InFlowOutline, primitive);
        }
        if let Some(target) = atom.link_target() {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                paint_space_rect(content_x, y, content_width, content_height),
                target.to_string(),
            ));
        }
    }

    /// Paint a horizontal tate-chu-yoko sequence inside its one-em atomic
    /// vertical box.  Capturing the nested sequence before adding the scale
    /// keeps glyphs, shadows, decorations, and links in one normal paint
    /// subtree rather than applying disconnected per-glyph offsets. The
    /// measured square limits layout only: CSS permits glyph ink to extend
    /// outside it, so this replay deliberately establishes no overflow clip.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    fn paint_text_combine_upright(
        &mut self,
        sequence: &InlineLineSequence,
        horizontal_style: &ComputedStyle,
        inline_scale: f32,
        content_rect: PhysicalInlineRect,
    ) {
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_inline_box_sequence(
            sequence,
            horizontal_style,
            content_rect.x(),
            sequence.available_width,
            content_rect.y() + content_rect.height(),
        );
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        if fragment.is_empty() {
            return;
        }
        let scaled_width = sequence.available_width * inline_scale;
        let centered_x = content_rect.x() + (content_rect.width() - scaled_width) / 2.0;
        let transform = PaintTransform::translate(PaintTranslation::new(centered_x, 0.0))
            .multiply(PaintTransform::scale(inline_scale, 1.0))
            .multiply(PaintTransform::translate(PaintTranslation::new(
                -content_rect.x(),
                0.0,
            )));
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                transform: Some(transform),
                ..PaintEffects::default()
            });
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(PaintBand::Inline, context),
            PaintTranslation::identity(),
        );
    }

    pub(in crate::layout) fn paint_inline_box_sequence(
        &mut self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
    ) {
        self.paint_inline_box_sequence_with_float_policy(
            sequence,
            style,
            content_left,
            available_width,
            block_top,
            NestedInlinePaintFloatPolicy::ReapplyActiveFloatBands,
        );
    }

    pub(in crate::layout) fn paint_inline_box_sequence_with_float_policy(
        &mut self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
        float_policy: NestedInlinePaintFloatPolicy,
    ) {
        // This is a nested paint replay. Its selected lines may establish an
        // internal baseline (for example a ruby base or annotation), but
        // that baseline is not a line of the enclosing formatting context
        // and therefore must not escape through inline-block baseline export.
        // <https://drafts.csswg.org/css-inline-3/#baseline-layout>
        // <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_last_in_flow_line_baseline_y = self.last_in_flow_line_baseline_y;
        self.content_left = content_left;
        self.content_right = content_left + available_width;
        self.cursor_y = block_top;
        self.paint_inline_line_sequence_slice_with_text_source(
            sequence,
            style,
            InlineLineSequenceSlice {
                block_top,
                top: block_top,
                bottom: f32::NEG_INFINITY,
            },
            RenderedLineSource::InlineAtom,
            float_policy,
        );
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
        self.last_in_flow_line_baseline_y = saved_last_in_flow_line_baseline_y;
    }

    /// Paint the owned decoration of a split inline box edge.
    ///
    /// CSS margins affect layout advance, but backgrounds, borders, and padding
    /// paint over the border/padding area. Keeping this separate for box-edge
    /// atoms preserves negative-margin behavior without clipping the border:
    /// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>.
    pub(in crate::layout) fn paint_prepared_inline_box_edge(
        &mut self,
        prepared: &PreparedInlineAtom,
        edge: InlineBoxEdgeFragment,
    ) {
        if edge.paint_extent <= 0.0 || prepared.border_box.height() <= 0.0 {
            return;
        }
        let mut style = prepared.atom.style().clone();
        if let Some(color) = prepared.atom.current_color_override() {
            style.color = color;
        }
        apply_inline_box_edge_paint_style(&mut style, edge);
        if style.background.background_color.is_transparent()
            && style.background.background_image.is_none()
            && used_border_width(&style) == layout_pt(0.0)
        {
            return;
        }
        for primitive in self.box_background_primitives(
            paint_space_rect(
                prepared.border_box.x(),
                prepared.border_box.y(),
                prepared.border_box.width(),
                prepared.border_box.height(),
            ),
            &style,
        ) {
            self.push_primitive_in_band(PaintBand::Inline, primitive);
        }
    }
}
