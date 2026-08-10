use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Resolve the inline-level `vertical-align` shift for text fragments.
    ///
    /// CSS 2.2 defines most `vertical-align` values in terms of the parent
    /// inline box's baseline, content area, or x-height. This helper returns a
    /// shift where positive values raise the child inline box and negative
    /// values lower it:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn vertical_align_baseline_shift_for_inline_style(
        &mut self,
        style: &ComputedStyle,
        parent_style: &ComputedStyle,
    ) -> f32 {
        let own_baseline = self
            .font_system
            .rendered_first_line_baseline_offset(style)
            .points();
        self.vertical_align_baseline_shift_for_box(
            style,
            parent_style,
            style.line_height,
            own_baseline,
        )
    }

    /// Resolve the inline-level `vertical-align` shift for an atomic inline box.
    ///
    /// Atomic inline boxes expose synthesized baselines and margin-box extents,
    /// but CSS 2.2 alignment values still use the containing inline box as the
    /// reference:
    /// <https://www.w3.org/TR/css-inline-3/#atomic-inline> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn vertical_align_baseline_shift_for_atom(
        &mut self,
        atom: &InlineAtom,
        parent_style: &ComputedStyle,
    ) -> f32 {
        let own_block_size = inline_atom_logical_block_size(atom, parent_style);
        let own_baseline = inline_atom_logical_margin_box_baseline_offset(atom, parent_style);
        self.vertical_align_baseline_shift_for_box(
            atom.style(),
            parent_style,
            own_block_size,
            own_baseline,
        )
    }

    pub(in crate::layout) fn vertical_align_baseline_shift_for_box(
        &mut self,
        style: &ComputedStyle,
        parent_style: &ComputedStyle,
        own_block_size: f32,
        own_baseline: f32,
    ) -> f32 {
        let alignment_shift = match resolved_alignment_baseline_metric(style, parent_style) {
            BaselineMetric::Alphabetic => 0.0,
            BaselineMetric::Middle => {
                let parent_x_height = self
                    .font_system
                    .used_x_height_for_style(parent_style)
                    .points();
                own_block_size / 2.0 - own_baseline + parent_x_height / 2.0
            }
            BaselineMetric::TextTop | BaselineMetric::Hanging => {
                own_baseline
                    - self
                        .font_system
                        .rendered_first_line_baseline_offset(parent_style)
                        .points()
            }
            BaselineMetric::TextBottom | BaselineMetric::Ideographic => {
                let parent_baseline = self
                    .font_system
                    .rendered_first_line_baseline_offset(parent_style)
                    .points();
                own_block_size - own_baseline - (parent_style.font_size - parent_baseline)
            }
            BaselineMetric::Central | BaselineMetric::Mathematical => {
                own_block_size / 2.0 - own_baseline + parent_style.font_size / 2.0
            }
        };
        let baseline_shift = match style.vertical_align.baseline_shift {
            BaselineShift::LengthPercentage(_) => style
                .vertical_align
                .clone()
                .length_percentage_shift(layout_pt(style.line_height))
                .points(),
            BaselineShift::Super => self
                .font_system
                .script_vertical_align_shift(style, BaselineShift::Super)
                .unwrap_or(style.font_size * 0.45),
            BaselineShift::Sub => self
                .font_system
                .script_vertical_align_shift(style, BaselineShift::Sub)
                .unwrap_or(-style.font_size * 0.4),
            BaselineShift::Top | BaselineShift::Center | BaselineShift::Bottom => 0.0,
        };
        css::clamp_used_layout_coordinate(layout_pt(alignment_shift + baseline_shift)).points()
    }
}

fn resolved_alignment_baseline_metric(
    style: &ComputedStyle,
    parent_style: &ComputedStyle,
) -> BaselineMetric {
    match style.vertical_align.alignment_baseline {
        AlignmentBaseline::Metric(metric) => metric,
        AlignmentBaseline::Baseline => match parent_style.vertical_align.dominant_baseline {
            DominantBaseline::Metric(metric) => metric,
            DominantBaseline::Auto => BaselineMetric::Alphabetic,
        },
    }
}
