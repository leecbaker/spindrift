use super::super::*;
use super::graph::{InlineLineFragment, MeasuredInlineItem, measured_inline_items};

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn paint_mixed_inline_line(
        &mut self,
        line: &InlineLineFragment,
        context: InlinePaintContext<'_>,
    ) {
        let block_style = context.block_style;
        let line_metrics = line.metrics;
        let line_height = line_metrics.height.max(block_style.line_height);
        if line.items.is_empty() {
            self.cursor_y -= line_height;
            return;
        }
        if self.cursor_y - line_height < self.page_bottom() {
            self.push_page();
        }
        let band = self.current_float_band(self.cursor_y, line_height);
        let left_offset = (band.left - self.content_left - context.padding_left).max(0.0);
        let right_offset = (self.content_right - band.right).max(0.0);
        let context = if left_offset > 0.0 || right_offset > 0.0 {
            let available_width = (band.right - self.content_left - context.padding_left).max(1.0);
            InlinePaintContext {
                available_width: context.available_width.min(available_width),
                line_indent: context.line_indent.max(left_offset),
                ..context
            }
        } else {
            context
        };
        if let Some(prepared) = self.prepare_mixed_inline_line(line, context) {
            self.paint_prepared_mixed_inline_line(&prepared);
        }
        self.cursor_y -= line_height;
    }

    /// Prepare one mixed inline line for painting.
    ///
    /// CSS Inline first resolves the line box and positions inline-level
    /// fragments within it; CSS Text shaping then produces glyph runs for
    /// eligible adjacent text fragments. This function records those used
    /// positions and shaped groups before any page paint operation is emitted:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
    pub(in crate::layout) fn prepare_mixed_inline_line(
        &mut self,
        line_fragment: &InlineLineFragment,
        context: InlinePaintContext<'_>,
    ) -> Option<PreparedMixedInlineLine> {
        let block_style = context.block_style;
        let line_metrics = line_fragment.metrics;
        let justify_by_character = matches!(block_style.text_justify, TextJustify::InterCharacter);
        let apply_typographic_pseudos = context.is_first_line
            && (block_style.first_line_style.is_some() || block_style.first_letter_style.is_some());
        let line = if apply_typographic_pseudos {
            let mut source_items = measured_inline_items(&line_fragment.items);
            apply_first_line_pseudos_to_line_items(&mut source_items, block_style);
            let source_items = if justify_by_character {
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
                        InlineLineItem::Atom(atom) => atom.width,
                    };
                    MeasuredInlineItem {
                        item,
                        width,
                        shaped: None,
                    }
                })
                .collect::<Vec<_>>()
        } else if justify_by_character {
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
                    InlineLineItem::Atom(atom) => atom.width,
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
        let hanging_widths = line_fragment.hanging_widths;
        let line_available_width = (context.available_width - context.line_indent).max(1.0);
        let line_align = context.text_align;
        let line_baseline_offset = line_metrics.baseline_offset;
        let should_justify =
            line_align.justifies() && !matches!(block_style.text_justify, TextJustify::None);
        let justify_inter_word = should_justify && !justify_by_character;
        let extra_space_width = if should_justify {
            let gaps = if justify_by_character {
                inter_character_mixed_gap_count(&line_items)
            } else {
                justifiable_mixed_space_count(&line_items)
            };
            if gaps > 0 && line_metrics.width < line_available_width {
                (line_available_width - line_metrics.width) / gaps as f32
            } else {
                0.0
            }
        } else {
            0.0
        };
        let mut x = aligned_x_with_width(
            inline_line_left_for_indent(
                self.content_left,
                context.padding_left,
                context.line_indent,
                block_style,
            ),
            line_available_width,
            line_metrics.width,
            if should_justify {
                TextAlign::Left
            } else {
                line_align
            },
        );
        x += line_start_hanging_punctuation_paint_offset(block_style, hanging_widths.start)
            + line_end_hanging_punctuation_paint_offset(block_style, hanging_widths.end);
        let mut pending_fragments = Vec::new();
        let mut pending_x = x;
        let mut paint_items = Vec::new();
        for (item_index, measured_item) in line.iter().enumerate() {
            match &measured_item.item {
                InlineLineItem::Fragment(fragment) => {
                    let mut fragment = fragment.clone();
                    let had_soft_hyphen = fragment.text.contains('\u{00ad}');
                    if had_soft_hyphen {
                        fragment.text = fragment.text.replace('\u{00ad}', "");
                    }
                    if inline_fragment_is_join_control_only(&fragment) {
                        if pending_fragments.is_empty() {
                            pending_x = x;
                        }
                        pending_fragments.push(fragment);
                        continue;
                    }
                    let fragment_baseline_offset =
                        self.inline_paint_fragment_baseline_offset(&fragment);
                    let fragment_background_y = self.cursor_y - line_baseline_offset
                        + fragment_baseline_offset
                        - fragment.style.line_height;
                    if !matches!(fragment.style.vertical_align, VerticalAlign::Top) {
                        let natural_baseline_offset =
                            fragment_baseline_offset + fragment.baseline_shift;
                        fragment.baseline_shift += natural_baseline_offset - line_baseline_offset;
                    }
                    let mut width = measured_item
                        .shaped
                        .as_ref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or(measured_item.width);
                    if had_soft_hyphen {
                        width = self
                            .font_system
                            .measure_text(&fragment.text, &fragment.style);
                    }
                    if item_index + 1 == line.len() {
                        width = (width
                            - trailing_letter_spacing_width_for_line_items(std::slice::from_ref(
                                &measured_item.item,
                            )))
                        .max(0.0);
                    }
                    let fragment_is_justification_space =
                        inline_fragment_is_inter_word_justification_space(&fragment);
                    let inter_word_space_count = usize::from(justify_inter_word)
                        * usize::from(fragment_is_justification_space)
                        * fragment.text.chars().count();
                    paint_items.push(PreparedInlinePaintItem::FragmentBackground(
                        PreparedInlineFragment {
                            fragment: fragment.clone(),
                            x,
                            background_y: fragment_background_y,
                            width: width + extra_space_width * inter_word_space_count as f32,
                            height: fragment.style.line_height,
                        },
                    ));
                    let fragment_char_count = fragment.text.chars().count();
                    let can_append = (extra_space_width == 0.0 || justify_inter_word)
                        && pending_fragments.last().is_some_and(|previous| {
                            can_queue_inline_fragments_for_shaping(previous, &fragment)
                        });
                    if pending_fragments.is_empty() {
                        pending_x = x;
                    } else if !can_append {
                        if let Some(group) = if justify_inter_word {
                            self.prepare_justified_inline_text_group(
                                &pending_fragments,
                                pending_x,
                                extra_space_width,
                            )
                        } else {
                            self.prepare_inline_text_group(&pending_fragments, pending_x)
                        } {
                            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                        }
                        pending_fragments.clear();
                        pending_x = x;
                    }
                    if fragment.style.visibility == Visibility::Visible
                        && inline_fragment_has_visible_text_paint(&fragment)
                    {
                        pending_fragments.push(fragment);
                    }
                    x += width;
                    let add_inter_character_gap = justify_by_character
                        && inter_character_gap_after_mixed_item(&line_items, item_index);
                    if extra_space_width > 0.0
                        && (add_inter_character_gap || fragment_is_justification_space)
                    {
                        if add_inter_character_gap {
                            if let Some(group) =
                                self.prepare_inline_text_group(&pending_fragments, pending_x)
                            {
                                paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                            }
                            pending_fragments.clear();
                        }
                        x += if add_inter_character_gap {
                            extra_space_width
                        } else {
                            extra_space_width * fragment_char_count as f32
                        };
                    }
                }
                InlineLineItem::Atom(atom)
                    if matches!(atom.content, InlineAtomContent::Leader(_)) =>
                {
                    if let Some(group) = if justify_inter_word {
                        self.prepare_justified_inline_text_group(
                            &pending_fragments,
                            pending_x,
                            extra_space_width,
                        )
                    } else {
                        self.prepare_inline_text_group(&pending_fragments, pending_x)
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    let InlineAtomContent::Leader(pattern) = &atom.content else {
                        unreachable!();
                    };
                    let pattern_width = self.font_system.measure_text(pattern, &atom.style);
                    if pattern_width > 0.0 {
                        let remaining = (line_available_width - line_metrics.width).max(0.0);
                        let repeat_count = (remaining / pattern_width).floor() as usize;
                        if repeat_count > 0 {
                            let text = pattern.repeat(repeat_count);
                            let width = self.font_system.measure_text(&text, &atom.style);
                            let fragment = InlineFragment {
                                text,
                                style: atom.style.clone(),
                                baseline_shift: atom.baseline_shift,
                                link_target: atom.link_target.clone(),
                                mergeable: false,
                                hanging_edges: InlineHangingEdges::default(),
                            };
                            if let Some(group) =
                                self.prepare_inline_text_group(std::slice::from_ref(&fragment), x)
                            {
                                paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                            }
                            x += width;
                        }
                    }
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(atom.content, InlineAtomContent::Leader(_)) {
                        continue;
                    }
                    if let Some(group) = if justify_inter_word {
                        self.prepare_justified_inline_text_group(
                            &pending_fragments,
                            pending_x,
                            extra_space_width,
                        )
                    } else {
                        self.prepare_inline_text_group(&pending_fragments, pending_x)
                    } {
                        paint_items.push(PreparedInlinePaintItem::TextGroup(group));
                    }
                    pending_fragments.clear();
                    let content_x = x + atom.style.margin.left;
                    let content_width =
                        (atom.width - atom.style.margin.left - atom.style.margin.right).max(0.0);
                    let margin_box_inner_height =
                        (atom.height - atom.style.margin.top - atom.style.margin.bottom).max(0.0);
                    // CSS 2.2 inline formatting treats inline-block/replaced
                    // boxes as atomic inline-level margin boxes; the border
                    // box is painted inside the atom's vertical margins.
                    let content_height = margin_box_inner_height;
                    let y = self.cursor_y - line_baseline_offset + atom.baseline_offset
                        - margin_box_inner_height
                        + atom.baseline_shift;
                    paint_items.push(PreparedInlinePaintItem::Atom(PreparedInlineAtom {
                        atom: atom.clone(),
                        content_x,
                        y,
                        content_width,
                        content_height,
                    }));
                    x += atom.width;
                    if extra_space_width > 0.0
                        && justify_by_character
                        && inter_character_gap_after_mixed_item(&line_items, item_index)
                    {
                        x += extra_space_width;
                    }
                }
            }
        }
        if let Some(group) = if justify_inter_word {
            self.prepare_justified_inline_text_group(
                &pending_fragments,
                pending_x,
                extra_space_width,
            )
        } else {
            self.prepare_inline_text_group(&pending_fragments, pending_x)
        } {
            paint_items.push(PreparedInlinePaintItem::TextGroup(group));
        }
        Some(PreparedMixedInlineLine {
            metrics: line_metrics,
            paint_items,
        })
    }

    /// Paint a prepared mixed inline line without reshaping text.
    ///
    /// PDF text and CSS decoration emission consume the shaped glyph runs
    /// stored during line preparation, keeping fallback fonts and glyph
    /// advances stable after line fitting:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.4 "Text".
    fn paint_prepared_mixed_inline_line(&mut self, line: &PreparedMixedInlineLine) {
        debug_assert!(line.metrics.height.is_finite());
        for item in &line.paint_items {
            match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => {
                    self.paint_inline_fragment_background(
                        &fragment.fragment,
                        fragment.x,
                        fragment.background_y,
                        fragment.width,
                        fragment.height,
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
            InlineAtomContent::InlineEdge | InlineAtomContent::Leader(_)
        ) {
            return;
        }
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_prepared_inline_atom_contents(prepared);
        self.scope_current_page_atomic_paint_since(
            &checkpoint,
            PaintBand::Inline,
            PaintClip {
                x: prepared.content_x,
                y: prepared.y,
                width: prepared.content_width,
                height: prepared.content_height,
            },
            &atom.style,
            Vec::new(),
        );
    }

    fn paint_prepared_inline_atom_contents(&mut self, prepared: &PreparedInlineAtom) {
        let atom = &prepared.atom;
        let content_x = prepared.content_x;
        let y = prepared.y;
        let content_width = prepared.content_width;
        let content_height = prepared.content_height;
        match &atom.content {
            InlineAtomContent::InlineEdge | InlineAtomContent::Leader(_) => {}
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
                    RenderedImage {
                        background: false,
                        x: image_x,
                        y: image_y,
                        width: image_width,
                        height: image_height,
                        pixel_width: decoded.pixel_width,
                        pixel_height: decoded.pixel_height,
                        source_rect: None,
                        interpolate: false,
                        rgb: decoded.rgb.clone(),
                        alpha: decoded.alpha.clone(),
                        alt_text: atom.alt_text.clone(),
                    },
                );
            }
            InlineAtomContent::Svg { fill } => {
                self.push_rect_in_band(
                    PaintBand::Inline,
                    RenderedRect {
                        x: content_x,
                        y,
                        width: content_width,
                        height: content_height,
                        fill: Some(*fill),
                        stroke: None,
                        stroke_width: 0.0,
                    },
                );
            }
            InlineAtomContent::InlineBox { lines } => {
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
                let mut line_top = y + content_height - borders.top - atom.style.padding.top;
                let text_x = content_x + borders.left + atom.style.padding.left;
                let text_available_width = (content_width
                    - borders.left
                    - borders.right
                    - atom.style.padding.left
                    - atom.style.padding.right)
                    .max(1.0);
                for line in lines {
                    let x = aligned_x_with_width(
                        text_x,
                        text_available_width,
                        line.width,
                        atom.style.text_align.physical(atom.style.direction),
                    );
                    let rendered_line = if let Some(shaped) = &line.shaped {
                        self.paint_shaped_inline_line(
                            shaped,
                            x,
                            line_top - atom.style.font_size,
                            &atom.style,
                        )
                    } else {
                        self.paint_text_runs(
                            &line.text,
                            x,
                            line_top - atom.style.font_size,
                            &atom.style,
                        )
                    };
                    if let Some(rendered_line) = rendered_line {
                        self.paint_text_decoration_lines(
                            x,
                            rendered_line.y,
                            line.width,
                            &atom.style,
                            &rendered_line.runs,
                        );
                    }
                    line_top -= line.line_height;
                }
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
                    .append_paint_fragment(fragment, content_x, y);
            }
        }
        for primitive in
            self.box_outline_primitives(content_x, y, content_width, content_height, &atom.style)
        {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
        if let Some(target) = &atom.link_target {
            self.current_page.push_link(RenderedLink {
                x: content_x,
                y,
                width: content_width,
                height: content_height,
                target: target.clone(),
            });
        }
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
