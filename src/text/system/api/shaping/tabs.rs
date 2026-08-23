use super::super::*;
use crate::css::ComputedLengthPercentage;
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};

pub(in crate::text) struct RenderedRunTabContext<'a> {
    /// The computed style of the preserved tab itself. Its `tab-size` value
    /// selects the tab period.
    pub(in crate::text) style: &'a ComputedStyle,
    /// The nearest block container's computed style. Numeric `tab-size`
    /// values use this style's U+0020 advance, including text spacing.
    pub(in crate::text) metric_style: &'a ComputedStyle,
}

impl FontSystem {
    pub(in crate::text) fn apply_css_tab_stops(
        &mut self,
        runs: &mut [ShapedGlyphRun],
        contexts: &[RenderedRunTabContext<'_>],
        tab_origin: f32,
        letter_spacing: ShapingLetterSpacing,
    ) {
        if !runs.iter().any(|run| run.text.contains('\t')) {
            return;
        }

        // CSS tab stops are one line-level grid. Parley can split that line
        // into independently positioned style, fallback, or bidi runs, but
        // none of those boundaries resets the cursor used to select a stop.
        // Retain the backend offsets as a provisional placement only and
        // rebase each later run with the accumulated tab correction.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        let mut logical_line_cursor = 0.0;
        let mut accumulated_tab_correction = 0.0;
        for (run_index, run) in runs.iter_mut().enumerate() {
            // Parley reports a run beginning with a tab after its provisional
            // tab glyph advance. CSS instead resolves the tab from the cursor
            // before that glyph, at the block content edge for a leading tab.
            let starts_with_tab = run
                .text
                .chars()
                .find(|character| !character_is_bidi_format_control(*character))
                .filter(|character| *character == '\t')
                .is_some();
            let provisional_start = run.x_offset + accumulated_tab_correction;
            let run_start = if starts_with_tab {
                logical_line_cursor
            } else {
                provisional_start.max(logical_line_cursor)
            };
            run.x_offset = run_start;
            let mut run_cursor = run_start;

            for glyph in &mut run.glyphs {
                if glyph.unicode == "\t" {
                    let Some(context) = contexts.get(run_index) else {
                        run_cursor += glyph.x_advance;
                        continue;
                    };
                    let space_advance =
                        self.css_tab_stop_space_advance(context.metric_style, letter_spacing);
                    let tab_period = context
                        .style
                        .tab_size
                        .used_tab_stop_advance(space_advance.points())
                        .points();
                    // `ch` is a font metric, not a tracked or word-spaced
                    // glyph advance. Resolve it from the tab's own font
                    // inputs while retaining the tab's computed style as the
                    // owner of the minimum-width rule.
                    let mut unspaced_tab_style = context.style.clone();
                    unspaced_tab_style.letter_spacing = ComputedLengthPercentage::ZERO;
                    unspaced_tab_style.word_spacing = ComputedLengthPercentage::ZERO;
                    let minimum_advance = self.ch_advance(&unspaced_tab_style).points() * 0.5;
                    let old_advance = glyph.x_advance;
                    let used_advance =
                        tab_stop_advance(tab_period, tab_origin + run_cursor, minimum_advance)
                            .points();
                    glyph.x_advance = used_advance;
                    glyph.nominal_x_advance = space_advance.points();
                    glyph.x_offset = 0.0;
                    glyph.y_offset = 0.0;
                    accumulated_tab_correction += used_advance - old_advance;
                }
                run_cursor += glyph.x_advance;
            }

            logical_line_cursor = run_cursor;
        }
    }

    fn css_tab_stop_space_advance(
        &mut self,
        style: &ComputedStyle,
        letter_spacing: ShapingLetterSpacing,
    ) -> LayoutLength {
        // Shape a spacing-free U+0020 through the ordinary text pipeline.
        // This retains the font face that actually renders U+0020, including
        // generic-family aliases, @font-face selection, and fallback. Add the
        // nearest block container's associated spacing exactly once below.
        // <https://www.w3.org/TR/css-text-3/#tab-size-property>
        let mut unspaced_style = style.clone();
        unspaced_style.letter_spacing = ComputedLengthPercentage::ZERO;
        unspaced_style.word_spacing = ComputedLengthPercentage::ZERO;
        layout_pt(
            self.shape_untracked_inline_line(" ", &unspaced_style, unspaced_style.line_height)
                .and_then(|matched| {
                    matched
                        .runs
                        .iter()
                        .filter_map(|run| run.font_id)
                        .any(|font_id| self.document_fonts.font_has_character(font_id, ' '))
                        .then(|| matched.advance_width())
                })
                .or_else(|| {
                    self.character_font_match(style, ' ').and_then(|matched| {
                        let font_size = self
                            .font_size_adjusted_size_for_font_id(style, matched.font_id)
                            .unwrap_or(style.font_size);
                        let font = self.document_fonts.get(matched.font_id)?;
                        shape_text_with_document_font(font, " ", font_size, 0.0, 0.0)
                            .map(|glyphs| glyphs.into_iter().map(|glyph| glyph.x_advance).sum())
                    })
                })
                .filter(|advance| *advance > 0.0 && advance.is_finite())
                .unwrap_or(style.font_size * 0.25)
                + letter_spacing.requested_for(style)
                + style.used_word_spacing().points(),
        )
    }
}

pub(in crate::text) fn synthesized_tab_glyph(provisional_advance: f32) -> RenderedGlyph {
    RenderedGlyph {
        kind: RenderedGlyphKind::AdvanceOnly,
        x_advance: provisional_advance,
        nominal_x_advance: provisional_advance,
        x_offset: 0.0,
        y_offset: 0.0,
        unicode: "\t".to_string(),
    }
}

pub(in crate::text) fn tab_stop_advance(
    period: f32,
    current_x: f32,
    minimum_advance: f32,
) -> LayoutLength {
    if period <= 0.0 || !period.is_finite() || !current_x.is_finite() {
        return layout_pt(0.0);
    }
    let next_stop = (current_x / period).floor().mul_add(period, period);
    let advance = (next_stop - current_x).max(0.0);
    // Tabs that would be too narrow use exactly the subsequent stop. The
    // threshold is strictly less than 0.5ch, per CSS Text.
    // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
    if minimum_advance.is_finite() && advance < minimum_advance.max(0.0) {
        layout_pt(advance + period)
    } else {
        layout_pt(advance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_stop_uses_the_following_stop_only_below_half_ch() {
        assert_eq!(tab_stop_advance(8.0, 7.6, 0.5), layout_pt(8.4));
        assert_eq!(tab_stop_advance(8.0, 7.5, 0.5), layout_pt(0.5));
        assert_eq!(tab_stop_advance(0.0, 0.0, 0.5), layout_pt(0.0));
    }
}
