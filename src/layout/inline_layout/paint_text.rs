use super::super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn paint_inline_item_line(
        &mut self,
        line: &[InlineFragment],
        line_metrics: InlineLineMetrics,
        context: InlinePaintContext<'_>,
    ) {
        let block_style = context.block_style;
        let line_height = line_metrics.height.max(block_style.line_height);
        if line.is_empty() {
            self.cursor_y -= line_height;
            return;
        }
        if self.cursor_y - block_style.font_size < self.page_bottom() {
            self.push_page();
        }
        let mut line = line.to_vec();
        trim_inline_fragment_edges(&mut line);
        if context.is_first_line {
            apply_first_line_pseudos_to_fragments(&mut line, block_style);
        }
        if line.is_empty() {
            self.cursor_y -= line_height;
            return;
        }
        let visible_line_width = inline_fragment_line_width(&line, &mut self.font_system);
        let line_available_width = (context.available_width - context.line_indent).max(1.0);
        let hanging_widths = hanging_punctuation_widths(
            &mut self.font_system,
            &line,
            block_style,
            context.is_first_line,
            context.is_last_line,
            visible_line_width > line_available_width,
        );
        let measured_line_width =
            (visible_line_width - hanging_widths.start - hanging_widths.end).max(0.0);
        let justify_by_character = matches!(block_style.text_justify, TextJustify::InterCharacter);
        if justify_by_character {
            line = split_fragments_into_inter_character_units(&line);
        }
        let uses_parley_alignment = line_metrics.aligned_by_parley;
        let hanging_changes_measure = hanging_widths.start > 0.0 || hanging_widths.end > 0.0;
        let line_align = context.text_align;
        let should_justify = !uses_parley_alignment
            && line_align.justifies()
            && !matches!(block_style.text_justify, TextJustify::None);
        let justify_inter_word = should_justify && !justify_by_character;
        let extra_space_width = if should_justify {
            let gaps = if justify_by_character {
                inter_character_fragment_gap_count(&line)
            } else {
                justifiable_fragment_space_count(&line)
            };
            if gaps > 0 && measured_line_width < line_available_width {
                (line_available_width - measured_line_width) / gaps as f32
            } else {
                0.0
            }
        } else {
            0.0
        };
        let line_left = inline_line_left_for_indent(
            self.content_left,
            context.padding_left,
            context.line_indent,
            block_style,
        );
        let mut x = if uses_parley_alignment && !hanging_changes_measure {
            line_left + line_metrics.offset
        } else {
            aligned_x_with_width(
                line_left,
                line_available_width,
                measured_line_width,
                if should_justify {
                    TextAlign::Left
                } else {
                    line_align
                },
            )
        };
        x += line_start_hanging_punctuation_paint_offset(block_style, hanging_widths.start)
            + line_end_hanging_punctuation_paint_offset(block_style, hanging_widths.end);
        let mut pending_fragments = Vec::new();
        let mut pending_x = x;
        for (index, fragment) in line.iter().enumerate() {
            let mut width = self
                .font_system
                .measure_text(&fragment.text, &fragment.style);
            if index + 1 == line.len() {
                width = (width
                    - trailing_letter_spacing_width_for_fragments(std::slice::from_ref(fragment)))
                .max(0.0);
            }
            let inter_word_space_count = usize::from(justify_inter_word)
                * usize::from(inline_fragment_is_inter_word_justification_space(fragment))
                * fragment.text.chars().count();
            self.paint_inline_fragment_background(
                fragment,
                x,
                self.cursor_y - fragment.style.line_height + fragment.baseline_shift,
                width + extra_space_width * inter_word_space_count as f32,
                fragment.style.line_height,
            );
            let can_append = (extra_space_width == 0.0 || justify_inter_word)
                && pending_fragments.last().is_some_and(|previous| {
                    can_queue_inline_fragments_for_shaping(previous, fragment)
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
                    self.paint_prepared_inline_text_group(&group);
                }
                pending_fragments.clear();
                pending_x = x;
            }
            if fragment.style.visibility == Visibility::Visible
                && inline_fragment_has_visible_text_paint(fragment)
            {
                pending_fragments.push(fragment.clone());
            }
            x += width;
            let add_inter_character_gap = justify_by_character && index + 1 < line.len();
            if extra_space_width > 0.0
                && (add_inter_character_gap
                    || inline_fragment_is_inter_word_justification_space(fragment))
            {
                if add_inter_character_gap {
                    if let Some(group) =
                        self.prepare_inline_text_group(&pending_fragments, pending_x)
                    {
                        self.paint_prepared_inline_text_group(&group);
                    }
                    pending_fragments.clear();
                }
                x += if add_inter_character_gap {
                    extra_space_width
                } else {
                    extra_space_width * fragment.text.chars().count() as f32
                };
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
            self.paint_prepared_inline_text_group(&group);
        }
        self.cursor_y -= line_height;
    }
}
