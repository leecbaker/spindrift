use super::super::*;
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
    ) {
        if !runs.iter().any(|run| run.text.contains('\t')) {
            return;
        }

        let mut logical_line_cursor = 0.0;
        for run_index in 0..runs.len() {
            let Some(context) = contexts.get(run_index) else {
                continue;
            };
            let space_advance = self.css_tab_stop_space_advance(context.metric_style);
            let tab_period = context
                .style
                .tab_size
                .used_tab_stop_advance(space_advance.points())
                .points();
            let run_x_offset = runs[run_index].x_offset;
            // A visual shaping pass can split immediately before a preserved
            // tab and give that later run an origin of zero. Carry the
            // selected line's cursor across that split; otherwise the tab is
            // incorrectly treated as leading and advances a whole period.
            // <https://www.w3.org/TR/css-text-3/#tab-size-property>
            let tab_run_x_offset = if run_x_offset <= 0.01 && logical_line_cursor > 0.01 {
                logical_line_cursor
            } else {
                run_x_offset
            };
            // Parley reports a run beginning with a tab after its provisional
            // tab glyph advance. CSS instead resolves the tab from the cursor
            // before that glyph, at the block content edge for a leading tab.
            // Subsequent tabs in the same run retain the advances accumulated
            // from their preceding glyphs.
            let starts_with_tab = runs[run_index]
                .text
                .chars()
                .find(|character| !character_is_bidi_format_control(*character))
                .filter(|character| *character == '\t')
                .is_some();
            let mut pen_x = if starts_with_tab && logical_line_cursor <= 0.01 {
                -tab_run_x_offset
            } else {
                0.0
            };
            let mut following_run_shift = 0.0;

            for glyph in &mut runs[run_index].glyphs {
                if glyph.unicode == "\t" {
                    let old_advance = glyph.x_advance;
                    let used_advance =
                        tab_stop_advance(tab_period, tab_origin + tab_run_x_offset + pen_x)
                            .points();
                    glyph.x_advance = used_advance;
                    glyph.nominal_x_advance = space_advance.points();
                    glyph.x_offset = 0.0;
                    glyph.y_offset = 0.0;
                    following_run_shift += used_advance - old_advance;
                }
                pen_x += glyph.x_advance;
            }

            if following_run_shift.abs() > 0.01 {
                for following_run in runs.iter_mut().skip(run_index + 1) {
                    following_run.x_offset += following_run_shift;
                }
            }
            logical_line_cursor = logical_line_cursor.max(
                tab_run_x_offset
                    + runs[run_index]
                        .glyphs
                        .iter()
                        .map(|glyph| glyph.x_advance)
                        .sum::<f32>(),
            );
        }
    }

    fn css_tab_stop_space_advance(&mut self, style: &ComputedStyle) -> LayoutLength {
        let direct_advance = self.character_font_match(style, ' ').and_then(|matched| {
            let (data, face_index, units_per_em) = self
                .document_fonts
                .get(matched.font_id)
                .map(|font| (font.data.clone(), font.face_index, font.units_per_em))?;
            let face = ttf_parser::Face::parse(&data, face_index).ok()?;
            let advance = face.glyph_hor_advance(matched.glyph_id.raw())? as f32;
            let used_font_size = self
                .font_size_adjusted_size_for_font_id(style, matched.font_id)
                .unwrap_or(style.font_size);
            Some(advance * used_font_size / units_per_em.max(1) as f32)
        });
        layout_pt(
            direct_advance
                .or_else(|| {
                    self.shape_unwrapped_line(" ", style, style.line_height)
                        .map(|line| line.advance_width())
                })
                .filter(|advance| *advance > 0.0 && advance.is_finite())
                .unwrap_or(style.font_size * 0.25)
                + style.used_letter_spacing().points()
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

pub(in crate::text) fn tab_stop_advance(period: f32, current_x: f32) -> LayoutLength {
    if period <= 0.0 || !period.is_finite() || !current_x.is_finite() {
        return layout_pt(0.0);
    }
    let next_stop = (current_x / period).floor().mul_add(period, period);
    layout_pt((next_stop - current_x).max(0.0))
}
