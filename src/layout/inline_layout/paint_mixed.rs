use super::super::*;
use super::InlineLineSequence;
use super::graph::{InlineLineFragment, MeasuredInlineItem, measured_inline_items};

impl<'a> LayoutBuilder<'a> {
    /// Prepare one inline line fragment for painting.
    ///
    /// CSS Inline first resolves the line box and positions inline-level
    /// fragments within it; CSS Text shaping then produces glyph runs for
    /// eligible adjacent text fragments. This function records those used
    /// positions and shaped groups before any page paint operation is emitted:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
    pub(in crate::layout) fn prepare_inline_line_fragment(
        &mut self,
        line_fragment: &InlineLineFragment,
        context: InlinePaintContext<'_>,
    ) -> Option<PreparedInlineLine> {
        let block_style = context.block_style;
        let line_metrics = line_fragment.metrics;
        let should_justify_line = context.text_align.justifies()
            && !matches!(block_style.text_justify, TextJustify::None);
        let split_for_inter_character =
            should_justify_line && matches!(block_style.text_justify, TextJustify::InterCharacter);
        let apply_typographic_pseudos = context.is_first_line
            && (block_style.first_line_style.is_some() || block_style.first_letter_style.is_some());
        let line = if apply_typographic_pseudos {
            let mut source_items = measured_inline_items(&line_fragment.items);
            apply_first_line_pseudos_to_line_items(&mut source_items, block_style);
            let source_items = if split_for_inter_character {
                split_mixed_line_into_inter_character_units(&source_items)
            } else {
                source_items
            };
            source_items
                .into_iter()
                .map(|item| {
                    let width = match &item {
                        InlineLineItem::Fragment(fragment) => self
                            .font_system
                            .shape_unwrapped_line(
                                &fragment.text,
                                &fragment.style,
                                fragment.style.line_height,
                            )
                            .map(|line| line.advance_width())
                            .unwrap_or(0.0),
                        InlineLineItem::Atom(atom) => {
                            inline_atom_logical_inline_size(atom, block_style)
                        }
                        InlineLineItem::Float(_) => 0.0,
                    };
                    MeasuredInlineItem {
                        item,
                        width,
                        shaped: None,
                    }
                })
                .collect::<Vec<_>>()
        } else if split_for_inter_character {
            split_mixed_line_into_inter_character_units(&measured_inline_items(
                &line_fragment.items,
            ))
            .into_iter()
            .map(|item| {
                let width = match &item {
                    InlineLineItem::Fragment(fragment) => self
                        .font_system
                        .shape_unwrapped_line(
                            &fragment.text,
                            &fragment.style,
                            fragment.style.line_height,
                        )
                        .map(|line| line.advance_width())
                        .unwrap_or(0.0),
                    InlineLineItem::Atom(atom) => {
                        inline_atom_logical_inline_size(atom, block_style)
                    }
                    InlineLineItem::Float(_) => 0.0,
                };
                MeasuredInlineItem {
                    item,
                    width,
                    shaped: None,
                }
            })
            .collect::<Vec<_>>()
        } else {
            line_fragment.items.clone()
        };
        let line_items = measured_inline_items(&line);
        let line_geometry = InlineLineGeometry::new(self.content_left, self.cursor_y, context);
        let line_alignment_width = (line_metrics.width
            - visual_leading_inline_end_box_edge_width(&line, line_geometry))
        .max(0.0);
        let hanging_widths = line_fragment.hanging_widths;
        let line_available_width = line_geometry.inline_size;
        let line_align = context.text_align;
        let line_baseline_offset = line_metrics.baseline_offset;
        let justification_plan = InlineJustificationPlan::for_line(
            &line_items,
            block_style.text_justify,
            should_justify_line,
        );
        let extra_space_width =
            justification_plan.extra_space_width(line_metrics.width, line_available_width);
        let mut line_logical_inline_start = line_geometry.alignment_offset(
            line_alignment_width,
            if should_justify_line {
                TextAlign::Left
            } else {
                line_align
            },
        );
        line_logical_inline_start += line_geometry.hanging_punctuation_offset(hanging_widths);
        let line_physical_origin =
            line_geometry.visual_line_origin(line_logical_inline_start, line_alignment_width);
        let mut inline_position = 0.0;
        let mut pending_fragments = Vec::new();
        let mut pending_inline_position = inline_position;
        let mut pending_preserve_leading_summary_space = false;
        let mut previous_item_was_opaque_atom = false;
        let mut paint_items = Vec::new();
        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let mut fragment = fragment.clone();
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_inline_position = inline_position;
                            pending_preserve_leading_summary_space = previous_item_was_opaque_atom;
                        }
                        pending_fragments.push(fragment);
                        previous_item_was_opaque_atom = false;
                        continue;
                    }
                    let fragment_baseline_offset =
                        self.inline_paint_fragment_baseline_offset(&fragment);
                    let fragment_background_y = self.cursor_y - line_baseline_offset
                        + fragment_baseline_offset
                        - fragment.style.line_height;
                    if !fragment.style.vertical_align.aligns_to_line_box_edge() {
                        let natural_baseline_offset =
                            fragment_baseline_offset + fragment.baseline_shift;
                        fragment.baseline_shift += natural_baseline_offset - line_baseline_offset;
                    }
                    let mut width = measured_item
                        .shaped
                        .as_ref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or(measured_item.width);
                    if item_index + 1 == line.len() {
                        width = (width
                            - trailing_letter_spacing_width_for_line_items(std::slice::from_ref(
                                &measured_item.item,
                            )))
                        .max(0.0);
                    }
                    let fragment_expansion_count =
                        justification_plan.expansion_count_after_item(item_index);
                    let fragment_rect = line_geometry.visual_line_item_rect(
                        line_logical_inline_start,
                        line_physical_origin,
                        inline_position,
                        width + extra_space_width * fragment_expansion_count as f32,
                        fragment_background_y,
                        fragment.style.line_height,
                    );
                    paint_items.push(PreparedInlinePaintItem::FragmentBackground(
                        PreparedInlineFragment {
                            fragment: fragment.clone(),
                            rect: fragment_rect,
                        },
                    ));
                    let can_append = (extra_space_width == 0.0
                        || justification_plan.justifies_inter_word())
                        && pending_fragments.last().is_some_and(|previous| {
                            can_queue_inline_fragments_for_shaping(previous, &fragment)
                        });
                    if pending_fragments.is_empty() {
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = previous_item_was_opaque_atom;
                    } else if !can_append {
                        if let Some(group) = if justification_plan.justifies_inter_word() {
                            self.prepare_justified_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                extra_space_width,
                                pending_preserve_leading_summary_space,
                            )
                        } else {
                            self.prepare_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_preserve_leading_summary_space,
                            )
                        } {
                            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                        }
                        pending_fragments.clear();
                        pending_inline_position = inline_position;
                        pending_preserve_leading_summary_space = false;
                    }
                    if fragment.style.visibility == Visibility::Visible
                        && inline_fragment_has_visible_text_paint(&fragment)
                    {
                        pending_fragments.push(fragment);
                    }
                    inline_position += width;
                    let add_inter_character_gap = justification_plan.justifies_inter_character()
                        && fragment_expansion_count > 0;
                    if extra_space_width > 0.0
                        && (add_inter_character_gap || fragment_expansion_count > 0)
                    {
                        if add_inter_character_gap {
                            if let Some(group) = self.prepare_inline_text_group_at_inline_position(
                                &pending_fragments,
                                line_geometry,
                                line_logical_inline_start,
                                line_physical_origin,
                                pending_inline_position,
                                pending_preserve_leading_summary_space,
                            ) {
                                paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                            }
                            pending_fragments.clear();
                            pending_preserve_leading_summary_space = false;
                        }
                        inline_position += if add_inter_character_gap {
                            extra_space_width
                        } else {
                            extra_space_width * fragment_expansion_count as f32
                        };
                    }
                    previous_item_was_opaque_atom = false;
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content, InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    if let Some(group) = if justification_plan.justifies_inter_word() {
                        self.prepare_justified_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            extra_space_width,
                            pending_preserve_leading_summary_space,
                        )
                    } else {
                        self.prepare_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                        )
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    pending_preserve_leading_summary_space = false;
                    let logical_inline_start_margin =
                        inline_atom_logical_inline_start_margin(atom, block_style);
                    let content_inline_size =
                        inline_atom_logical_border_inline_size(atom, block_style);
                    let content_block_size =
                        inline_atom_logical_border_block_size(atom, block_style);
                    // CSS 2.2 inline formatting treats inline-block/replaced
                    // boxes as atomic inline-level margin boxes; the border
                    // box is painted inside the atom's logical margins.
                    let y = inline_atom_horizontal_content_y(
                        atom,
                        block_style,
                        self.cursor_y,
                        line_metrics.height,
                        line_baseline_offset,
                        content_block_size,
                    );
                    let content_rect = line_geometry.visual_line_item_rect(
                        line_logical_inline_start,
                        line_physical_origin,
                        inline_position + logical_inline_start_margin,
                        content_inline_size,
                        y,
                        content_block_size,
                    );
                    paint_items.push(PreparedInlinePaintItem::Atom(PreparedInlineAtom {
                        atom: atom.clone(),
                        content_rect,
                    }));
                    inline_position += inline_atom_logical_inline_size(atom, block_style);
                    previous_item_was_opaque_atom =
                        inline_atom_content_preserves_adjacent_space_summary(&atom.content);
                }
                InlineLineItem::Float(_) => {
                    if let Some(group) = if justification_plan.justifies_inter_word() {
                        self.prepare_justified_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            extra_space_width,
                            pending_preserve_leading_summary_space,
                        )
                    } else {
                        self.prepare_inline_text_group_at_inline_position(
                            &pending_fragments,
                            line_geometry,
                            line_logical_inline_start,
                            line_physical_origin,
                            pending_inline_position,
                            pending_preserve_leading_summary_space,
                        )
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    pending_preserve_leading_summary_space = false;
                    inline_position += measured_item.width;
                    previous_item_was_opaque_atom = false;
                }
            }
        }
        if let Some(group) = if justification_plan.justifies_inter_word() {
            self.prepare_justified_inline_text_group_at_inline_position(
                &pending_fragments,
                line_geometry,
                line_logical_inline_start,
                line_physical_origin,
                pending_inline_position,
                extra_space_width,
                pending_preserve_leading_summary_space,
            )
        } else {
            self.prepare_inline_text_group_at_inline_position(
                &pending_fragments,
                line_geometry,
                line_logical_inline_start,
                line_physical_origin,
                pending_inline_position,
                pending_preserve_leading_summary_space,
            )
        } {
            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
        }
        Some(PreparedInlineLine {
            metrics: line_metrics,
            paint_items,
        })
    }

    fn prepare_inline_text_group_at_inline_position(
        &mut self,
        fragments: &[InlineFragment],
        line_geometry: InlineLineGeometry,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            preserve_leading_summary_space,
        )?;
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
        Some(group)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_justified_inline_text_group_at_inline_position(
        &mut self,
        fragments: &[InlineFragment],
        line_geometry: InlineLineGeometry,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        extra_per_separator: f32,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_justified_inline_text_group_with_summary_policy(
            fragments,
            0.0,
            extra_per_separator,
            preserve_leading_summary_space,
        )?;
        line_geometry.position_visual_text_group(
            &mut group,
            line_logical_inline_start,
            line_physical_origin,
            visual_inline_start,
        );
        Some(group)
    }

    /// Paint a prepared inline line without reshaping text.
    ///
    /// PDF text and CSS decoration emission consume the shaped glyph runs
    /// stored during line preparation, keeping fallback fonts and glyph
    /// advances stable after line fitting:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(in crate::layout) fn paint_prepared_inline_line(&mut self, line: &PreparedInlineLine) {
        debug_assert!(line.metrics.height.is_finite());
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.rect.x(),
                        fragment.rect.y(),
                        fragment.rect.width(),
                        fragment.rect.height(),
                    );
                }
                PreparedInlinePaintItem::TextGroup(group) => {
                    self.paint_prepared_inline_text_group(group);
                }
                PreparedInlinePaintItem::Atom(atom) => {
                    self.paint_prepared_inline_atom(atom);
                }
            }
        }
    }

    /// Paint one prepared atomic inline box.
    ///
    /// CSS Inline treats replaced and inline-block descendants as atomic
    /// inline-level boxes. The prepared atom stores the resolved content box so
    /// painting does not recompute line positioning:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
    fn paint_prepared_inline_atom(&mut self, prepared: &PreparedInlineAtom) {
        let atom = &prepared.atom;
        if atom.style.visibility != Visibility::Visible {
            return;
        }
        if matches!(
            atom.content,
            InlineAtomContent::InlineEdge(_) | InlineAtomContent::Leader(_)
        ) {
            return;
        }
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_prepared_inline_atom_contents(prepared);
        self.scope_current_page_atomic_paint_since(
            &checkpoint,
            PaintBand::Inline,
            prepared.content_rect.paint_clip(),
            &atom.style,
            Vec::new(),
        );
        if let Some(layers) = &atom.escaped_positioned_layers {
            for layer in layers.iter() {
                let mut layer = layer.clone().translated(PaintVector::new(
                    prepared.content_rect.x(),
                    prepared.content_rect.y(),
                ));
                layer.page_index = self.pages.len();
                self.positioned_layers.push(layer);
            }
        }
    }

    fn paint_prepared_inline_atom_contents(&mut self, prepared: &PreparedInlineAtom) {
        let atom = &prepared.atom;
        let content_x = prepared.content_rect.x();
        let y = prepared.content_rect.y();
        let content_width = prepared.content_rect.width();
        let content_height = prepared.content_rect.height();
        match &atom.content {
            InlineAtomContent::InlineEdge(_) | InlineAtomContent::Leader(_) => {}
            InlineAtomContent::Canvas => {
                if atom.style.background_color.is_some() || used_border_width(&atom.style) > 0.0 {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, &atom.style);
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::Inline, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::Inline, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::Inline, path);
                    }
                    for stroke in strokes {
                        self.push_stroke_in_band(PaintBand::Inline, stroke);
                    }
                }
            }
            InlineAtomContent::Image(decoded) => {
                if atom.style.background_color.is_some() || used_border_width(&atom.style) > 0.0 {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, &atom.style);
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::Inline, rect);
                    }
                    for rounded_rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::Inline, rounded_rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::Inline, path);
                    }
                    self.extend_strokes_in_band(PaintBand::Inline, strokes);
                }
                let borders = used_border_widths(&atom.style);
                let image_x = content_x + borders.left + atom.style.padding.left;
                let image_y = y + borders.bottom + atom.style.padding.bottom;
                let image_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style.padding.left
                    - atom.style.padding.right)
                    .max(0.0);
                let image_height = (content_height
                    - borders.top
                    - borders.bottom
                    - atom.style.padding.top
                    - atom.style.padding.bottom)
                    .max(0.0);
                self.push_image_in_band(
                    PaintBand::Inline,
                    RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, image_width, image_height),
                        false,
                        decoded.pixel_width,
                        decoded.pixel_height,
                        None,
                        false,
                        decoded.rgb.clone(),
                        decoded.alpha.clone(),
                        atom.alt_text.clone(),
                    ),
                );
            }
            InlineAtomContent::Svg { fill } => {
                self.push_rect_in_band(
                    PaintBand::Inline,
                    RenderedRect::from_paint_rect(
                        paint_space_rect(content_x, y, content_width, content_height),
                        Some(*fill),
                    ),
                );
            }
            InlineAtomContent::InlineBox { sequence } => {
                if atom.style.background_color.is_some() || used_border_width(&atom.style) > 0.0 {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, &atom.style);
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::Inline, rect);
                    }
                    for rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::Inline, rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::Inline, path);
                    }
                    self.extend_strokes_in_band(PaintBand::Inline, strokes);
                }
                let borders = used_border_widths(&atom.style);
                let text_top = y + content_height - borders.top - atom.style.padding.top;
                let text_x = content_x + borders.left + atom.style.padding.left;
                let text_available_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style.padding.left
                    - atom.style.padding.right)
                    .max(1.0);
                self.paint_inline_box_sequence(
                    sequence,
                    &atom.style,
                    text_x,
                    text_available_width,
                    text_top,
                );
            }
            InlineAtomContent::InlineFragment(fragment) => {
                if atom.style.background_color.is_some() || used_border_width(&atom.style) > 0.0 {
                    let (rects, rounded_rects, paths, strokes) =
                        block_paint_ops(content_x, y, content_width, content_height, &atom.style);
                    for rect in rects {
                        self.push_rect_in_band(PaintBand::Inline, rect);
                    }
                    for rect in rounded_rects {
                        self.push_rounded_rect_in_band(PaintBand::Inline, rect);
                    }
                    for path in paths {
                        self.push_path_in_band(PaintBand::Inline, path);
                    }
                    self.extend_strokes_in_band(PaintBand::Inline, strokes);
                }
                self.current_page
                    .append_paint_fragment(fragment, PaintVector::new(content_x, y));
            }
        }
        for primitive in
            self.box_outline_primitives(content_x, y, content_width, content_height, &atom.style)
        {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
        if let Some(target) = &atom.link_target {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                paint_space_rect(content_x, y, content_width, content_height),
                target.clone(),
            ));
        }
    }

    fn paint_inline_box_sequence(
        &mut self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
    ) {
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        self.content_left = content_left;
        self.content_right = content_left + available_width;
        self.cursor_y = block_top;
        self.paint_inline_line_sequence_slice(
            sequence,
            style,
            block_top,
            block_top,
            f32::NEG_INFINITY,
        );
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
    }

    /// Return a text fragment baseline offset from the mixed line top.
    ///
    /// CSS Inline Layout aligns all inline-level participants in a line box to
    /// the line baseline. Text fragments derive their baseline from selected
    /// font metrics; mixed-line painting shifts each fragment from its natural
    /// font baseline to the shared line baseline:
    /// <https://www.w3.org/TR/css-inline-3/#line-box>.
    fn inline_paint_fragment_baseline_offset(&mut self, fragment: &InlineFragment) -> f32 {
        let font_id = self.font_system.resolve_style(&fragment.style);
        let line_height = self
            .font_system
            .line_height_for_font(font_id, &fragment.style);
        let adjustment =
            self.font_system
                .font_ascent_baseline_adjustment(font_id, &fragment.style, line_height);
        fragment.style.font_size - adjustment - fragment.baseline_shift
    }
}

/// Return the physical bottom y for a horizontal atomic inline content box.
///
/// CSS 2.2 defines `vertical-align: top`/`bottom` on inline-level boxes as
/// margin-box alignment to the line box edge; baseline-like values use the
/// atom's synthesized baseline:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
fn inline_atom_horizontal_content_y(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
    line_top: f32,
    line_height: f32,
    line_baseline_offset: f32,
    content_block_size: f32,
) -> f32 {
    match atom.style.vertical_align.baseline_shift {
        BaselineShift::Top => {
            line_top
                - inline_atom_logical_block_start_margin(atom, containing_style)
                - content_block_size
                + atom.baseline_shift
        }
        BaselineShift::Bottom => {
            line_top - line_height
                + inline_atom_logical_block_end_margin(atom, containing_style)
                + atom.baseline_shift
        }
        BaselineShift::LengthPercentage(_)
        | BaselineShift::Sub
        | BaselineShift::Super
        | BaselineShift::Center => {
            line_top - line_baseline_offset + atom.baseline_offset - content_block_size
                + atom.baseline_shift
        }
    }
}

fn inline_atom_content_preserves_adjacent_space_summary(content: &InlineAtomContent) -> bool {
    matches!(
        content,
        InlineAtomContent::Canvas
            | InlineAtomContent::Image(_)
            | InlineAtomContent::Svg { .. }
            | InlineAtomContent::InlineBox { .. }
            | InlineAtomContent::InlineFragment(_)
            | InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge)
    )
}

fn visual_leading_inline_end_box_edge_width(
    line: &[MeasuredInlineItem],
    geometry: InlineLineGeometry,
) -> f32 {
    if !matches!(geometry.writing_mode, WritingMode::HorizontalTb)
        || !matches!(geometry.direction, Direction::Rtl)
    {
        return 0.0;
    }
    line.iter()
        .take_while(|item| {
            matches!(
                &item.item,
                InlineLineItem::Atom(atom)
                    if matches!(atom.content, InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge))
            )
        })
        .map(|item| item.width)
        .sum()
}
