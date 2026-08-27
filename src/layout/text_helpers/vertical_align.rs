use crate::css::{
    self, AlignmentBaseline, BaselineMetric, BaselineShift, ComputedStyle, DominantBaseline,
    LayoutLength, TextLayoutPolicy, TextOrientation, layout_pt,
};
use crate::layout::{
    InlineAtom, LayoutBuilder, inline_atom_logical_block_size,
    inline_atom_logical_margin_box_baseline_offset,
};
use crate::units::{
    AlphabeticBaselineRelativeOffset, AuthorBaselineShift, BaselineTableAlignmentDelta,
    ContentBoxBaselineOffset, GlyphBaselineDisplacement, SemanticLengthExt,
    alphabetic_baseline_relative_pt, author_baseline_shift_pt, baseline_table_alignment_pt,
    content_box_baseline_pt, glyph_baseline_displacement_pt,
};

/// The two CSS operations that determine where an inline glyph baseline is
/// painted. Keeping the baseline-table result distinct from `baseline-shift`
/// prevents a content-box baseline coordinate from being passed to paint as a
/// glyph-origin displacement.
#[derive(Clone, Copy, Debug)]
pub(in crate::layout) struct InlineBaselinePlacement {
    inherited_shift: GlyphBaselineDisplacement,
    alignment: BaselineTableAlignmentDelta,
    author_shift: AuthorBaselineShift,
}

impl InlineBaselinePlacement {
    pub(in crate::layout) const fn from_inherited_glyph_displacement(
        inherited_shift: GlyphBaselineDisplacement,
    ) -> Self {
        Self {
            inherited_shift,
            alignment: baseline_table_alignment_pt(0.0),
            author_shift: author_baseline_shift_pt(0.0),
        }
    }

    pub(in crate::layout) fn with_added(self, added: Self) -> Self {
        Self {
            inherited_shift: glyph_baseline_displacement_pt(
                self.inherited_shift.points() + added.inherited_shift.points(),
            ),
            alignment: baseline_table_alignment_pt(
                self.alignment.points() + added.alignment.points(),
            ),
            author_shift: author_baseline_shift_pt(
                self.author_shift.points() + added.author_shift.points(),
            ),
        }
    }

    pub(in crate::layout) fn glyph_displacement(self) -> GlyphBaselineDisplacement {
        glyph_baseline_displacement_pt(
            self.inherited_shift.points() + self.alignment.points() + self.author_shift.points(),
        )
    }
}

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
    ) -> InlineBaselinePlacement {
        let metric = resolved_alignment_baseline_metric(style, parent_style);
        let own_metric = self
            .font_system
            .baseline_offset_for_style(style, metric)
            .into_content_box_baseline_offset();
        let own_alphabetic = self
            .font_system
            .baseline_offset_for_style(style, BaselineMetric::Alphabetic)
            .into_content_box_baseline_offset();
        self.vertical_align_baseline_shift_for_coordinates(
            style,
            parent_style,
            own_metric,
            own_alphabetic,
            metric,
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
    ) -> InlineBaselinePlacement {
        let own_block_size = inline_atom_logical_block_size(atom, parent_style);
        let own_baseline =
            inline_atom_logical_margin_box_baseline_offset(atom, parent_style).points();
        let style = atom.style();
        let metric = resolved_alignment_baseline_metric(style, parent_style);
        // Atomic inline boxes export their first baseline. CSS Inline's
        // content-derived baseline set supplies the other named metrics from
        // that same internal coordinate system. Only an atom without a
        // content-derived baseline set uses CSS Inline's margin-edge
        // synthesis rules.
        let own_metric =
            if let Some(source_metric) = atom.content_derived_baseline_metric(parent_style) {
                self.content_exported_atom_metric_offset(atom, metric, source_metric, own_baseline)
            } else {
                atomic_baseline_metric_offset(metric, own_block_size, own_baseline)
            };
        self.vertical_align_baseline_shift_for_coordinates(
            style,
            parent_style,
            content_box_baseline_pt(own_metric),
            content_box_baseline_pt(own_baseline),
            metric,
        )
    }

    fn vertical_align_baseline_shift_for_coordinates(
        &mut self,
        style: &ComputedStyle,
        parent_style: &ComputedStyle,
        own_metric: ContentBoxBaselineOffset,
        own_alphabetic: ContentBoxBaselineOffset,
        metric: BaselineMetric,
    ) -> InlineBaselinePlacement {
        // Align the child's selected alignment baseline with the *same*
        // parent baseline. This is the CSS Inline baseline-table operation;
        // it replaces the old per-keyword offsets relative to alphabetic.
        // <https://drafts.csswg.org/css-inline-3/#baseline-alignment>
        let parent_metric = self
            .font_system
            .baseline_offset_for_style(parent_style, metric)
            .into_content_box_baseline_offset();
        let parent_alphabetic = self
            .font_system
            .baseline_offset_for_style(parent_style, BaselineMetric::Alphabetic)
            .into_content_box_baseline_offset();
        // Inline formatting starts from alphabetic-baseline alignment. Move
        // by the difference between each box's selected metric and that
        // already-aligned alphabetic baseline, rather than treating the
        // selected coordinates as absolute line-box positions.
        let alignment_shift =
            baseline_alignment_shift(own_metric, own_alphabetic, parent_metric, parent_alphabetic);
        self.vertical_align_baseline_shift_after_alignment(style, alignment_shift)
    }

    fn vertical_align_baseline_shift_after_alignment(
        &mut self,
        style: &ComputedStyle,
        alignment_shift: BaselineTableAlignmentDelta,
    ) -> InlineBaselinePlacement {
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
        InlineBaselinePlacement {
            inherited_shift: glyph_baseline_displacement_pt(0.0),
            alignment: alignment_shift,
            author_shift: author_baseline_shift_pt(
                css::clamp_used_layout_coordinate(layout_pt(baseline_shift)).points(),
            ),
        }
    }

    /// Convert an exported atom first baseline into another member of the
    /// same text baseline set. The atom's first baseline is measured from its
    /// margin-box block start; rebasing through its recorded source metric
    /// preserves that origin for central baselines in vertical writing modes.
    fn content_exported_atom_metric_offset(
        &mut self,
        atom: &InlineAtom,
        metric: BaselineMetric,
        source_metric: BaselineMetric,
        exported_source_metric: f32,
    ) -> f32 {
        let style = atom.style();
        let source = self
            .font_system
            .baseline_offset_for_style(style, source_metric)
            .points();
        let requested = self
            .font_system
            .baseline_offset_for_style(style, metric)
            .points();
        content_exported_metric_offset(exported_source_metric, requested, source)
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
            DominantBaseline::Auto => match parent_style.text_layout_policy() {
                TextLayoutPolicy::Vertical(TextOrientation::Mixed | TextOrientation::Upright) => {
                    BaselineMetric::Central
                }
                TextLayoutPolicy::Horizontal
                | TextLayoutPolicy::Vertical(TextOrientation::Sideways)
                | TextLayoutPolicy::Sideways(_) => BaselineMetric::Alphabetic,
            },
        },
    }
}

fn atomic_baseline_metric_offset(
    metric: BaselineMetric,
    block_size: f32,
    alphabetic_baseline: f32,
) -> f32 {
    match metric {
        BaselineMetric::TextTop | BaselineMetric::Hanging => 0.0,
        BaselineMetric::TextBottom | BaselineMetric::Ideographic => block_size,
        BaselineMetric::Middle | BaselineMetric::Central | BaselineMetric::Mathematical => {
            block_size / 2.0
        }
        BaselineMetric::Alphabetic => alphabetic_baseline,
    }
}

/// Rebase a metric from an atom's text content on its exported alphabetic
/// baseline, which is already measured from the parent line's margin-box
/// origin.
fn content_exported_metric_offset(
    exported_alphabetic: f32,
    requested_metric: f32,
    alphabetic_metric: f32,
) -> f32 {
    exported_alphabetic + requested_metric - alphabetic_metric
}

trait IntoContentBoxBaselineOffset {
    fn into_content_box_baseline_offset(self) -> ContentBoxBaselineOffset;
}

impl IntoContentBoxBaselineOffset for LayoutLength {
    fn into_content_box_baseline_offset(self) -> ContentBoxBaselineOffset {
        content_box_baseline_pt(self.points())
    }
}

/// Convert a baseline-table alignment into a displacement from the inline
/// collector's alphabetic-aligned coordinate.
///
/// Each named baseline is measured from its content box's block-start edge.
/// Subtracting alphabetic makes the two values comparable on the coordinate
/// that inline formatting has already aligned, leaving only the requested
/// baseline-table adjustment.
fn baseline_alignment_shift(
    child_metric: ContentBoxBaselineOffset,
    child_alphabetic: ContentBoxBaselineOffset,
    parent_metric: ContentBoxBaselineOffset,
    parent_alphabetic: ContentBoxBaselineOffset,
) -> BaselineTableAlignmentDelta {
    let child_relative = alphabetic_baseline_relative_offset(child_metric, child_alphabetic);
    let parent_relative = alphabetic_baseline_relative_offset(parent_metric, parent_alphabetic);
    baseline_table_alignment_pt(child_relative.points() - parent_relative.points())
}

fn alphabetic_baseline_relative_offset(
    metric: ContentBoxBaselineOffset,
    alphabetic: ContentBoxBaselineOffset,
) -> AlphabeticBaselineRelativeOffset {
    alphabetic_baseline_relative_pt(metric.points() - alphabetic.points())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::WritingMode;

    #[test]
    fn dominant_baseline_auto_uses_central_for_vertical_mixed_and_upright() {
        let child = ComputedStyle::initial();
        for orientation in [TextOrientation::Mixed, TextOrientation::Upright] {
            let mut parent = ComputedStyle::initial();
            parent.writing_mode = WritingMode::VerticalRl;
            parent.text_orientation = orientation;
            assert_eq!(
                resolved_alignment_baseline_metric(&child, &parent),
                BaselineMetric::Central
            );
        }
    }

    #[test]
    fn dominant_baseline_auto_keeps_alphabetic_for_horizontal_and_sideways() {
        let child = ComputedStyle::initial();
        let horizontal = ComputedStyle::initial();
        assert_eq!(
            resolved_alignment_baseline_metric(&child, &horizontal),
            BaselineMetric::Alphabetic
        );

        let mut vertical_sideways = ComputedStyle::initial();
        vertical_sideways.writing_mode = WritingMode::VerticalRl;
        vertical_sideways.text_orientation = TextOrientation::Sideways;
        assert_eq!(
            resolved_alignment_baseline_metric(&child, &vertical_sideways),
            BaselineMetric::Alphabetic
        );
    }

    #[test]
    fn baseline_alignment_uses_each_nested_parent_baseline_table() {
        // The BASE diagnostic font at 240px, then successively halved. Each
        // assertion is the shift added at one nesting level; the cumulative
        // stream is +72, +66, +78, +78px.
        assert_eq!(
            baseline_alignment_shift(
                content_box_baseline_pt(18.0),
                content_box_baseline_pt(90.0),
                content_box_baseline_pt(36.0),
                content_box_baseline_pt(180.0),
            )
            .points(),
            72.0
        );
        assert_eq!(
            baseline_alignment_shift(
                content_box_baseline_pt(51.0),
                content_box_baseline_pt(45.0),
                content_box_baseline_pt(102.0),
                content_box_baseline_pt(90.0),
            )
            .points(),
            -6.0
        );
        assert_eq!(
            baseline_alignment_shift(
                content_box_baseline_pt(10.5),
                content_box_baseline_pt(22.5),
                content_box_baseline_pt(21.0),
                content_box_baseline_pt(45.0),
            )
            .points(),
            12.0
        );
        assert_eq!(
            baseline_alignment_shift(
                content_box_baseline_pt(11.25),
                content_box_baseline_pt(11.25),
                content_box_baseline_pt(22.5),
                content_box_baseline_pt(22.5),
            )
            .points(),
            0.0
        );
    }

    #[test]
    fn baseline_table_alignment_reaches_paint_only_through_a_glyph_displacement() {
        let placement = InlineBaselinePlacement {
            inherited_shift: glyph_baseline_displacement_pt(8.0),
            alignment: baseline_table_alignment_pt(72.0),
            author_shift: author_baseline_shift_pt(-3.0),
        };

        // A content-box baseline coordinate cannot be added to this record:
        // only a baseline-table delta and authored shift can be resolved into
        // the glyph-origin displacement consumed by inline painting.
        assert_eq!(placement.glyph_displacement().points(), 77.0);
    }

    #[test]
    fn exported_atom_baseline_set_keeps_the_exported_alphabetic_origin() {
        assert_eq!(content_exported_metric_offset(40.0, 12.0, 5.0), 47.0);
    }
}
