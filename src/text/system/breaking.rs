use super::*;

pub(super) const LINE_WIDTH_EPSILON: f32 = 0.5;

#[derive(Clone)]
pub(super) struct MeasuredLineCandidate {
    range: Range<usize>,
    text: String,
    width: f32,
    shaped: Option<ShapedInlineLine>,
}

#[derive(Clone)]
struct BreakSpacesCandidate {
    end: usize,
    candidate: MeasuredLineCandidate,
}

impl FontSystem {
    pub(crate) fn break_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Vec<TextLine> {
        let available_width = available_width.max(1.0);
        if style.white_space == crate::css::WhiteSpace::BreakSpaces {
            return self.break_spaces_text(text, style, available_width);
        }
        let line_height = self.used_line_height(style);
        let mut output = Vec::new();
        for (paragraph_index, paragraph) in text.split('\n').enumerate() {
            let paragraph = text_with_hyphenation_controls(paragraph, style);
            let paragraph = paragraph.as_ref();
            if paragraph.is_empty() && style.white_space.preserves_newlines() {
                let line = TextLine::new(String::new(), 0.0, line_height);
                output.push(if paragraph_index > 0 {
                    line.starting_after_forced_break()
                } else {
                    line
                });
                continue;
            }
            if text_is_css_collapsible_whitespace(paragraph)
                && !style.white_space.preserves_space_edges()
            {
                continue;
            }
            if contains_bidi_text(paragraph)
                || bidi_control_scope_for_style(style).is_some()
                || style.direction == Direction::Rtl
            {
                let mut lines = self.break_bidi_text_with_parley(paragraph, style, available_width);
                if paragraph_index > 0
                    && let Some(first) = lines.first_mut()
                {
                    first.starts_after_forced_break = true;
                }
                output.extend(lines);
                continue;
            }
            for (line_offset, candidate) in self
                .measured_line_ranges(paragraph, style, available_width)
                .into_iter()
                .enumerate()
            {
                if !candidate.text.is_empty() {
                    let line = TextLine::new(candidate.text, candidate.width, line_height)
                        .with_shaped(candidate.shaped);
                    output.push(if paragraph_index > 0 && line_offset == 0 {
                        line.starting_after_forced_break()
                    } else {
                        line
                    });
                }
            }
        }
        if output.is_empty() && !text_is_css_collapsible_whitespace(text) {
            let text = text_with_hyphenation_controls(text, style);
            let trimmed = if style.white_space.preserves_space_edges() {
                text.as_ref()
            } else {
                trim_css_collapsible_whitespace(text.as_ref())
            };
            let text = trimmed.to_string();
            let shaped = self.shape_measured_line(&text, style, line_height);
            let width = shaped
                .as_ref()
                .map(|line| line.width)
                .unwrap_or_else(|| self.measure_line_text(&text, style));
            output.push(TextLine::new(text, width, line_height).with_shaped(shaped));
        }
        output
    }

    pub(super) fn break_spaces_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Vec<TextLine> {
        let line_height = self.used_line_height(style);
        let mut output = Vec::new();
        for (paragraph_index, paragraph) in text.split('\n').enumerate() {
            let paragraph = text_with_hyphenation_controls(paragraph, style);
            let paragraph = paragraph.as_ref();
            if paragraph.is_empty() {
                let line = TextLine::new(String::new(), 0.0, line_height);
                output.push(if paragraph_index > 0 {
                    line.starting_after_forced_break()
                } else {
                    line
                });
                continue;
            }
            for (line_offset, candidate) in self
                .break_spaces_candidates(paragraph, style, available_width)
                .into_iter()
                .enumerate()
            {
                let line = TextLine::new(candidate.text, candidate.width, line_height)
                    .with_shaped(candidate.shaped);
                output.push(if paragraph_index > 0 && line_offset == 0 {
                    line.starting_after_forced_break()
                } else {
                    line
                });
            }
        }
        output
    }

    fn break_spaces_candidate(
        &mut self,
        text: &str,
        range: Range<usize>,
        style: &ComputedStyle,
        line_height: f32,
    ) -> MeasuredLineCandidate {
        let line_text = normalize_soft_hyphens(text[range.clone()].to_string(), false);
        let shaped = self.shape_unwrapped_line(&line_text, style, line_height);
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        MeasuredLineCandidate {
            range,
            text: line_text,
            width,
            shaped,
        }
    }

    fn break_spaces_candidates(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Vec<MeasuredLineCandidate> {
        if text.is_empty() {
            return Vec::new();
        }

        let line_height = self.used_line_height(style);
        let grapheme_breaks = grapheme_cluster_inner_boundaries(text);
        let uax14_breaks = measured_break_opportunities(text, style);
        let characters = text
            .char_indices()
            .map(|(start, character)| {
                let end = start + character.len_utf8();
                (
                    start,
                    end,
                    character_is_break_spaces_preserved_space(character),
                    uax14_breaks.binary_search(&end).is_ok()
                        || break_spaces_other_space_separator_allows_break_after(character),
                    break_spaces_character_suppresses_break_before(character),
                )
            })
            .collect::<Vec<_>>();
        let mut output = Vec::new();
        let mut line_start = 0usize;
        let mut index = 0usize;
        let mut last_space_break: Option<BreakSpacesCandidate> = None;
        let mut previous_space_break: Option<BreakSpacesCandidate> = None;
        let mut last_anywhere_break: Option<BreakSpacesCandidate> = None;
        let available_width = available_width.max(1.0);
        let line_break_anywhere = matches!(style.line_break, crate::css::LineBreak::Anywhere);
        let word_break_break_all = matches!(style.word_break, crate::css::WordBreak::BreakAll);
        let anywhere_breaks_allowed = line_break_anywhere || word_break_break_all;

        while index < characters.len() {
            let (
                char_start,
                char_end,
                is_preserved_space,
                is_uax14_break_after,
                suppresses_break_before,
            ) = characters[index];
            let is_soft_wrap_after = is_preserved_space || is_uax14_break_after;
            if anywhere_breaks_allowed
                && char_start > line_start
                && grapheme_breaks.binary_search(&char_start).is_ok()
                && break_spaces_anywhere_break_allowed(
                    line_break_anywhere,
                    &characters,
                    index,
                    line_start,
                )
            {
                last_anywhere_break = Some(BreakSpacesCandidate {
                    end: char_start,
                    candidate: self.break_spaces_candidate(
                        text,
                        line_start..char_start,
                        style,
                        line_height,
                    ),
                });
            }

            let candidate =
                self.break_spaces_candidate(text, line_start..char_end, style, line_height);
            if candidate.width > available_width && line_start < char_end {
                let space_break = if suppresses_break_before
                    && last_space_break.as_ref().map(|candidate| candidate.end) == Some(char_start)
                {
                    previous_space_break.clone().or(last_space_break.clone())
                } else {
                    last_space_break.clone()
                };
                let break_candidate = if anywhere_breaks_allowed {
                    last_anywhere_break
                        .clone()
                        .filter(|candidate| {
                            candidate.end > line_start && candidate.end <= char_start
                        })
                        .or_else(|| {
                            space_break.clone().filter(|candidate| {
                                candidate.end > line_start && candidate.end < char_end
                            })
                        })
                } else {
                    space_break
                        .clone()
                        .filter(|candidate| candidate.end > line_start && candidate.end < char_end)
                }
                .or_else(|| {
                    is_soft_wrap_after
                        .then(|| BreakSpacesCandidate {
                            end: char_end,
                            candidate: candidate.clone(),
                        })
                        .filter(|candidate| candidate.end > line_start)
                });
                let Some(break_candidate) = break_candidate else {
                    index += 1;
                    continue;
                };
                output.push(break_candidate.candidate);
                line_start = break_candidate.end;
                index = characters
                    .iter()
                    .position(|(_, end, _, _, _)| *end > line_start)
                    .unwrap_or(characters.len());
                last_space_break = None;
                previous_space_break = None;
                last_anywhere_break = None;
                continue;
            }
            if is_soft_wrap_after {
                if is_preserved_space {
                    previous_space_break = last_space_break.clone();
                }
                last_space_break = Some(BreakSpacesCandidate {
                    end: char_end,
                    candidate,
                });
            }
            index += 1;
        }
        if line_start < text.len() {
            output.push(self.break_spaces_candidate(
                text,
                line_start..text.len(),
                style,
                line_height,
            ));
        }
        output
    }

    pub(super) fn measured_line_ranges(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Vec<MeasuredLineCandidate> {
        if text.is_empty() {
            return Vec::new();
        }
        if !style.white_space.allows_soft_wrap() {
            let range = 0..text.len();
            return self
                .visible_line_candidate(text, range, style, false, false)
                .into_iter()
                .collect();
        }

        let breaks = measured_break_opportunities(text, style);
        let emergency_breaks_allowed = measured_emergency_breaks_allowed(style);
        let mut output = Vec::new();
        let mut line_start = 0usize;
        let mut index = 0usize;
        while line_start < text.len() {
            while index < breaks.len() && breaks[index] <= line_start {
                index += 1;
            }
            let mut chosen: Option<MeasuredLineCandidate> = None;
            let mut emitted = false;
            let mut break_index = index;
            while break_index < breaks.len() {
                let line_end = breaks[break_index];
                let line_is_soft_wrap = line_end < text.len();
                let visible_trailing_soft_hyphen =
                    line_end < text.len() && text[..line_end].ends_with(SOFT_HYPHEN);
                let candidate = self.visible_line_candidate(
                    text,
                    line_start..line_end,
                    style,
                    visible_trailing_soft_hyphen,
                    line_is_soft_wrap,
                );
                let Some(candidate) = candidate else {
                    break_index += 1;
                    continue;
                };
                let fit_width = self.hanging_punctuation_fit_width(
                    text,
                    line_start..line_end,
                    style,
                    candidate.width,
                );
                if fit_width <= available_width + LINE_WIDTH_EPSILON || chosen.is_none() {
                    chosen = Some(candidate.clone());
                }
                if fit_width > available_width + LINE_WIDTH_EPSILON {
                    if let Some(candidate) = chosen
                        && candidate.range.end > line_start
                        && candidate.range.end < line_end
                    {
                        let range_end =
                            pre_wrap_soft_break_range_end(text, candidate.range.end, style);
                        output.push(MeasuredLineCandidate {
                            range: line_start..range_end,
                            ..candidate
                        });
                        line_start = range_end;
                        index = break_index;
                        emitted = true;
                        break;
                    }
                    if !emergency_breaks_allowed {
                        let range_end = pre_wrap_soft_break_range_end(text, line_end, style);
                        output.push(MeasuredLineCandidate {
                            range: line_start..range_end,
                            ..candidate
                        });
                        line_start = range_end;
                        index = break_index + 1;
                        emitted = true;
                        break;
                    }
                    let candidate =
                        self.emergency_line_candidate(text, line_start, style, available_width);
                    let end = candidate
                        .as_ref()
                        .map(|candidate| candidate.range.end)
                        .unwrap_or_else(|| {
                            text[line_start..]
                                .chars()
                                .next()
                                .map(|character| line_start + character.len_utf8())
                                .unwrap_or(text.len())
                        });
                    let range_end = pre_wrap_soft_break_range_end(text, end, style);
                    if let Some(candidate) = candidate {
                        output.push(MeasuredLineCandidate {
                            range: line_start..range_end,
                            ..candidate
                        });
                    }
                    line_start = range_end;
                    index = break_index;
                    emitted = true;
                    break;
                }
                if line_end == text.len() {
                    output.push(candidate);
                    line_start = line_end;
                    emitted = true;
                    break;
                }
                break_index += 1;
            }
            if !emitted {
                break;
            }
        }
        output
    }

    pub(super) fn emergency_line_candidate(
        &mut self,
        text: &str,
        start: usize,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Option<MeasuredLineCandidate> {
        let mut last_fit = None;
        for (offset, character) in text[start..].char_indices() {
            let end = start + offset + character.len_utf8();
            let candidate = self.visible_line_candidate(text, start..end, style, false, false)?;
            let fit_width =
                self.hanging_punctuation_fit_width(text, start..end, style, candidate.width);
            if fit_width > available_width {
                break;
            }
            last_fit = Some(candidate);
        }
        last_fit.or_else(|| {
            text[start..].chars().next().and_then(|character| {
                let end = start + character.len_utf8();
                self.visible_line_candidate(text, start..end, style, false, end < text.len())
            })
        })
    }

    pub(super) fn visible_line_candidate(
        &mut self,
        text: &str,
        range: Range<usize>,
        style: &ComputedStyle,
        visible_trailing_soft_hyphen: bool,
        line_is_soft_wrap: bool,
    ) -> Option<MeasuredLineCandidate> {
        let line_height = self.used_line_height(style);
        let line_text = self.visible_line_text(
            text,
            range.clone(),
            style,
            visible_trailing_soft_hyphen,
            line_is_soft_wrap,
        );
        let shaped = self.shape_measured_line(&line_text, style, line_height);
        let width = shaped
            .as_ref()
            .map(|line| line.width)
            .unwrap_or_else(|| self.measure_line_text(&line_text, style));
        Some(MeasuredLineCandidate {
            range,
            text: line_text,
            width,
            shaped,
        })
    }

    /// Return the line-fit width after CSS Text hanging punctuation policy.
    ///
    /// `force-end` and `allow-end` affect whether a stop/comma at the
    /// candidate line edge is measured for fitting. `allow-end` is conditional:
    /// the punctuation only hangs when the line would otherwise overflow:
    /// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
    pub(crate) fn hanging_punctuation_fit_width(
        &mut self,
        text: &str,
        range: Range<usize>,
        style: &ComputedStyle,
        width: f32,
    ) -> f32 {
        if range.is_empty()
            || !(style.hanging_punctuation.last
                || style.hanging_punctuation.force_end
                || style.hanging_punctuation.allow_end)
        {
            return width;
        }
        let is_last_line = range.end == text.len();
        let line_text = self.visible_line_text(text, range, style, false, false);
        let Some(character) = trim_end_css_collapsible_whitespace(&line_text)
            .chars()
            .next_back()
        else {
            return width;
        };
        let hangs_by_last = style.hanging_punctuation.last
            && is_last_line
            && character_is_last_hangable_punctuation(character);
        let hangs_by_force_end =
            style.hanging_punctuation.force_end && character_is_hangable_stop_or_comma(character);
        let hangs_by_allow_end =
            style.hanging_punctuation.allow_end && character_is_hangable_stop_or_comma(character);
        if !(hangs_by_last || hangs_by_force_end || hangs_by_allow_end) {
            return width;
        }
        let hanging_width = self.measure_text(&character.to_string(), style);
        (width - hanging_width).max(0.0)
    }

    pub(super) fn visible_line_text(
        &self,
        text: &str,
        range: Range<usize>,
        style: &ComputedStyle,
        visible_trailing_soft_hyphen: bool,
        line_is_soft_wrap: bool,
    ) -> String {
        let text = if style.white_space.preserves_space_edges() {
            let text = &text[range];
            if line_is_soft_wrap && style.white_space == crate::css::WhiteSpace::PreWrap {
                text.trim_end_matches(pre_wrap_soft_break_consumed_space)
                    .to_string()
            } else {
                text.to_string()
            }
        } else {
            trim_css_collapsible_whitespace(&text[range]).to_string()
        };
        normalize_soft_hyphens(text, visible_trailing_soft_hyphen)
    }

    pub(super) fn break_bidi_text_with_parley(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> Vec<TextLine> {
        let bidi_text = text_with_css_bidi_controls(text, style);
        let text = bidi_text.as_str();
        let mut builder = self.parley_layout_context.ranged_builder(
            &mut self.parley_font_context,
            text,
            1.0,
            false,
        );
        push_parley_default_style(&mut builder, style);
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(available_width));
        layout
            .lines()
            .filter_map(|line| {
                let visual_ranges = visual_ranges_for_line(line);
                let line_text = visual_text_from_ranges(text, &visual_ranges);
                let line_text = if style.white_space.preserves_space_edges() {
                    line_text
                } else {
                    trim_css_collapsible_whitespace(&line_text).to_string()
                };
                let line_text = text_without_bidi_format_controls(&line_text).into_owned();
                let line_range = line.text_range();
                let soft_hyphen_break =
                    line_ends_with_visible_soft_hyphen(text, &line_range, line.break_reason());
                let line_text = normalize_soft_hyphens(line_text, soft_hyphen_break);
                if line_text.is_empty() {
                    return None;
                }
                let line_height = self.used_line_height(style);
                let logical_line_text = text.get(line_range.clone()).unwrap_or_default();
                let logical_line_text = if style.white_space.preserves_space_edges() {
                    logical_line_text.to_string()
                } else {
                    trim_css_collapsible_whitespace(logical_line_text).to_string()
                };
                let logical_line_text =
                    text_without_bidi_format_controls(&logical_line_text).into_owned();
                let logical_line_text =
                    normalize_soft_hyphens(logical_line_text, soft_hyphen_break);
                let shaped = if line_text == logical_line_text {
                    self.shaped_measured_line_from_parley_line(
                        text,
                        &line_text,
                        line,
                        style,
                        line_height,
                    )
                } else {
                    self.shape_measured_line(&line_text, style, line_height)
                };
                let width = shaped
                    .as_ref()
                    .map(|line| line.width)
                    .unwrap_or_else(|| self.measure_line_text(&line_text, style));
                Some(TextLine::new(line_text, width, line_height).with_shaped(shaped))
            })
            .collect()
    }
}

/// Return the consumed source range end for a `pre-wrap` soft line break.
///
/// CSS Text phase II says preserved spaces at the end of a `pre-wrap` line
/// without a forced break must hang, so they do not contribute to line
/// measurement or alignment, but the soft break still consumes the space run
/// that provided the break opportunity:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
fn pre_wrap_soft_break_range_end(text: &str, line_end: usize, style: &ComputedStyle) -> usize {
    if style.white_space != crate::css::WhiteSpace::PreWrap || line_end >= text.len() {
        return line_end;
    }
    text[line_end..]
        .char_indices()
        .find_map(|(offset, character)| {
            (!pre_wrap_soft_break_consumed_space(character)).then_some(line_end + offset)
        })
        .unwrap_or(text.len())
}

fn pre_wrap_soft_break_consumed_space(character: char) -> bool {
    matches!(character, ' ' | '\t')
}
