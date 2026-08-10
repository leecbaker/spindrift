use super::*;
use crate::layout::inline_layout::mixed::InlineTextBoxMetrics;

/// Place a horizontal text group from the bottom of its CSS content area.
///
/// `PreparedInlineTextGroup` stores the physical text baseline origin, while
/// CSS 2.2 defines `vertical-align: top`/`bottom` in terms of box edges. This
/// maps the already resolved content-area bottom back to the group's baseline.
/// `InlineTextBoxMetrics::content_baseline_offset` is measured from the
/// content area's top edge to the baseline, so the bottom-to-baseline distance
/// is `content_block_size - content_baseline_offset`. Keeping that direction
/// explicit makes glyph painting and inline backgrounds share the same
/// edge-aligned box:
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
pub(in crate::layout) fn position_horizontal_text_group_at_content_bottom(
    group: &mut PreparedInlineTextGroup,
    content_bottom_y: f32,
    metrics: InlineTextBoxMetrics,
) {
    group.set_y(content_bottom_y + metrics.content_block_size - metrics.content_baseline_offset);
}

fn centered_content_bottom_y(
    line_top: f32,
    line_height: f32,
    content_block_size: f32,
    block_start_margin: f32,
    block_end_margin: f32,
    baseline_shift: f32,
) -> f32 {
    line_top - line_height / 2.0 - content_block_size / 2.0
        + (block_end_margin - block_start_margin) / 2.0
        + baseline_shift
}

/// Return a horizontal text fragment's line-relative content-area bottom y.
///
/// CSS Inline Layout aligns `baseline-shift: top | center | bottom` to the
/// line box. CSS 2.2 aligns `text-top`/`text-bottom` to the parent inline
/// box's content-area edges.
/// Inline non-replaced boxes paint their content area from `font-size` even
/// when only `line-height` contributes to line box sizing, so negative leading
/// can overflow without increasing the line box:
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
/// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
pub(in crate::layout) fn inline_fragment_horizontal_content_bottom_y(
    fragment: &(impl InlineFragmentAccess + ?Sized),
    line_relative_alignment: Option<InlineScopeLineRelativeAlignment>,
    line_top: f32,
    line_height: f32,
    line_baseline_offset: f32,
    metrics: InlineTextBoxMetrics,
    parent_metrics: InlineTextBoxMetrics,
) -> Option<f32> {
    match line_relative_alignment {
        Some(InlineScopeLineRelativeAlignment::Top) => Some(
            line_top - metrics.block_start_leading - metrics.content_block_size
                + fragment.baseline_shift(),
        ),
        Some(InlineScopeLineRelativeAlignment::Bottom) => {
            Some(line_top - line_height + metrics.block_end_leading + fragment.baseline_shift())
        }
        None => match fragment.style().vertical_align.baseline_shift {
            BaselineShift::Center => Some(centered_content_bottom_y(
                line_top,
                line_height,
                metrics.content_block_size,
                0.0,
                0.0,
                fragment.baseline_shift(),
            )),
            BaselineShift::LengthPercentage(_)
            | BaselineShift::Sub
            | BaselineShift::Super
            | BaselineShift::Top
            | BaselineShift::Bottom => None,
        },
    }
    .or_else(|| {
        let parent_content_top =
            line_top - line_baseline_offset + parent_metrics.content_baseline_offset;
        match fragment.style().vertical_align.alignment_baseline {
            AlignmentBaseline::Metric(BaselineMetric::TextTop) => {
                Some(parent_content_top - metrics.content_block_size)
            }
            AlignmentBaseline::Metric(BaselineMetric::TextBottom) => {
                Some(parent_content_top - parent_metrics.content_block_size)
            }
            AlignmentBaseline::Baseline
            | AlignmentBaseline::Metric(
                BaselineMetric::Alphabetic
                | BaselineMetric::Ideographic
                | BaselineMetric::Middle
                | BaselineMetric::Central
                | BaselineMetric::Mathematical
                | BaselineMetric::Hanging,
            ) => None,
        }
    })
}

pub(in crate::layout) fn inline_edge_horizontal_content_y(
    atom: &InlineAtom,
    line_top: f32,
    line_height: f32,
    line_baseline_offset: f32,
    metrics: InlineTextBoxMetrics,
    parent_metrics: InlineTextBoxMetrics,
) -> f32 {
    match atom.style().vertical_align.baseline_shift {
        BaselineShift::Top => {
            line_top - metrics.block_start_leading - metrics.content_block_size
                + atom.baseline_shift
        }
        BaselineShift::Bottom => {
            line_top - line_height + metrics.block_end_leading + atom.baseline_shift
        }
        BaselineShift::Center => centered_content_bottom_y(
            line_top,
            line_height,
            metrics.content_block_size,
            0.0,
            0.0,
            atom.baseline_shift,
        ),
        BaselineShift::LengthPercentage(_) | BaselineShift::Sub | BaselineShift::Super => {
            match atom.style().vertical_align.alignment_baseline {
                AlignmentBaseline::Metric(BaselineMetric::TextTop) => {
                    let parent_content_top =
                        line_top - line_baseline_offset + parent_metrics.content_baseline_offset;
                    parent_content_top - metrics.content_block_size
                }
                AlignmentBaseline::Metric(BaselineMetric::TextBottom) => {
                    let parent_content_top =
                        line_top - line_baseline_offset + parent_metrics.content_baseline_offset;
                    parent_content_top - parent_metrics.content_block_size
                }
                AlignmentBaseline::Baseline
                | AlignmentBaseline::Metric(
                    BaselineMetric::Alphabetic
                    | BaselineMetric::Ideographic
                    | BaselineMetric::Middle
                    | BaselineMetric::Central
                    | BaselineMetric::Mathematical
                    | BaselineMetric::Hanging,
                ) => {
                    line_top - line_baseline_offset
                        + atom.baseline_shift
                        + metrics.content_baseline_offset
                        - metrics.content_block_size
                }
            }
        }
    }
}

pub(in crate::layout) fn trim_inline_content_rect(
    rect: PhysicalInlineRect,
    writing_mode: WritingMode,
    trim: TextBoxLineTrim,
) -> PhysicalInlineRect {
    if trim.block_start <= 0.0 && trim.block_end <= 0.0 {
        return rect;
    }
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalInlineRect::new(InlineRect::new(
            InlinePoint::new(rect.x(), rect.y() + trim.block_end),
            InlineSize::new(
                rect.width(),
                (rect.height() - trim.block_start - trim.block_end).max(0.0),
            ),
        )),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(rect.x() - trim.block_start, rect.y()),
                InlineSize::new(
                    (rect.width() - trim.block_start - trim.block_end).max(0.0),
                    rect.height(),
                ),
            ))
        }
        WritingMode::VerticalLr | WritingMode::SidewaysLr => {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(rect.x() + trim.block_end, rect.y()),
                InlineSize::new(
                    (rect.width() - trim.block_start - trim.block_end).max(0.0),
                    rect.height(),
                ),
            ))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineAtomHorizontalPlacement {
    pub(in crate::layout) line_top: f32,
    pub(in crate::layout) line_height: f32,
    pub(in crate::layout) line_baseline_offset: f32,
    pub(in crate::layout) line_rendered_baseline_shift: f32,
    pub(in crate::layout) content_block_size: f32,
    pub(in crate::layout) parent_metrics: InlineTextBoxMetrics,
}

/// Return the physical bottom y for a horizontal atomic inline content box.
///
/// CSS Inline Layout defines `baseline-shift: top | center | bottom` on
/// inline-level boxes as margin-box alignment to the line box; baseline-like
/// values use the atom's synthesized baseline:
/// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
pub(in crate::layout) fn inline_atom_horizontal_content_y(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
    placement: InlineAtomHorizontalPlacement,
) -> f32 {
    match atom.line_relative_alignment() {
        Some(InlineScopeLineRelativeAlignment::Top) => {
            placement.line_top
                - inline_atom_logical_block_start_margin(atom, containing_style)
                - placement.content_block_size
                + atom.baseline_shift
        }
        Some(InlineScopeLineRelativeAlignment::Bottom) => {
            placement.line_top - placement.line_height
                + inline_atom_logical_block_end_margin(atom, containing_style)
                + atom.baseline_shift
        }
        None => match atom.style().vertical_align.baseline_shift {
            BaselineShift::Center => centered_content_bottom_y(
                placement.line_top,
                placement.line_height,
                placement.content_block_size,
                inline_atom_logical_block_start_margin(atom, containing_style),
                inline_atom_logical_block_end_margin(atom, containing_style),
                atom.baseline_shift,
            ),
            BaselineShift::LengthPercentage(_)
            | BaselineShift::Sub
            | BaselineShift::Super
            | BaselineShift::Top
            | BaselineShift::Bottom => match atom.style().vertical_align.alignment_baseline {
                AlignmentBaseline::Metric(BaselineMetric::TextTop) => {
                    let parent_content_top = placement.line_top - placement.line_baseline_offset
                        + placement.parent_metrics.content_baseline_offset;
                    parent_content_top
                        - inline_atom_logical_block_start_margin(atom, containing_style)
                        - placement.content_block_size
                }
                AlignmentBaseline::Metric(BaselineMetric::TextBottom) => {
                    let parent_content_top = placement.line_top - placement.line_baseline_offset
                        + placement.parent_metrics.content_baseline_offset;
                    parent_content_top - placement.parent_metrics.content_block_size
                        + inline_atom_logical_block_end_margin(atom, containing_style)
                }
                AlignmentBaseline::Baseline
                | AlignmentBaseline::Metric(
                    BaselineMetric::Alphabetic
                    | BaselineMetric::Ideographic
                    | BaselineMetric::Middle
                    | BaselineMetric::Central
                    | BaselineMetric::Mathematical
                    | BaselineMetric::Hanging,
                ) => {
                    let baseline = inline_atom_logical_content_placement_baseline_offset(
                        atom,
                        containing_style,
                    );
                    placement.line_top - placement.line_baseline_offset + baseline
                        - placement.content_block_size
                        + atom.baseline_shift
                        - placement.line_rendered_baseline_shift
                }
            },
        },
    }
}

pub(in crate::layout) fn inline_atom_content_preserves_adjacent_space_summary(
    content: &InlineAtomContent,
) -> bool {
    matches!(
        content,
        InlineAtomContent::Canvas
            | InlineAtomContent::Iframe(_)
            | InlineAtomContent::Image(_)
            | InlineAtomContent::Gradient { .. }
            | InlineAtomContent::Svg { .. }
            | InlineAtomContent::InlineBox { .. }
            | InlineAtomContent::TextCombineUpright { .. }
            | InlineAtomContent::InlineFragment { .. }
            | InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
    )
}

pub(in crate::layout) fn pending_inline_fragments_are_collapsible_space(
    fragments: &[impl InlineFragmentAccess],
) -> bool {
    !fragments.is_empty()
        && fragments.iter().all(|fragment| {
            fragment.style().white_space.collapses_spaces()
                && fragment.text().chars().all(is_css_collapsible_whitespace)
        })
}

pub(in crate::layout) fn pending_inline_fragments_are_join_control_only(
    fragments: &[impl InlineFragmentAccess],
) -> bool {
    !fragments.is_empty()
        && fragments
            .iter()
            .all(|fragment| inline_fragment_is_join_control_only(fragment))
}

pub(in crate::layout) fn inline_fragment_can_append_collapsible_space(
    previous: &(impl InlineFragmentAccess + ?Sized),
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    inline_fragment_is_collapsible_space(fragment)
        && inline_text_sources_are_paint_compatible(previous.source(), fragment.source())
        && previous.link_target() == fragment.link_target()
        && (previous.style().font_size - fragment.style().font_size).abs() < 0.01
        && previous.style().vertical_align == fragment.style().vertical_align
        && previous.style().visibility == fragment.style().visibility
}

pub(in crate::layout) fn inline_box_edge_paint_offset(edge: InlineBoxEdgeFragment) -> f32 {
    match edge.logical_edge {
        InlineLogicalEdge::Start => edge.advance - edge.paint_extent,
        InlineLogicalEdge::End => 0.0,
    }
}

pub(in crate::layout) fn inline_box_edge_is_painted_by_adjacent_fragment(
    line: &[MeasuredInlineItem],
    item_index: usize,
    edge_style: &ComputedStyle,
    edge: InlineBoxEdgeFragment,
) -> bool {
    match edge.logical_edge {
        InlineLogicalEdge::Start => line[item_index + 1..]
            .iter()
            .find_map(|item| {
                adjacent_fragment_paints_or_stops_edge(item, edge_style, edge.logical_edge)
            })
            .unwrap_or(false),
        InlineLogicalEdge::End => line[..item_index]
            .iter()
            .rev()
            .find_map(|item| {
                adjacent_fragment_paints_or_stops_edge(item, edge_style, edge.logical_edge)
            })
            .unwrap_or(false),
    }
}

pub(in crate::layout) fn adjacent_fragment_paints_or_stops_edge(
    item: &MeasuredInlineItem,
    edge_style: &ComputedStyle,
    edge: InlineLogicalEdge,
) -> Option<bool> {
    match &item.item {
        InlineLineItem::Fragment(fragment) => {
            if *fragment.style() != *edge_style {
                return Some(false);
            }
            Some(match edge {
                InlineLogicalEdge::Start => fragment.hanging_edges().blocks_start,
                InlineLogicalEdge::End => fragment.hanging_edges().blocks_end,
            })
        }
        InlineLineItem::Atom(atom) if atom.content().is_box_edge() => None,
        _ => Some(false),
    }
}

pub(in crate::layout) fn apply_inline_box_edge_paint_style(
    style: &mut ComputedStyle,
    edge: InlineBoxEdgeFragment,
) {
    style.margin = css::Edges::ZERO;
    let opposite = opposite_physical_side(edge.physical_side);
    clear_box_edge_side(style, opposite);
}

pub(in crate::layout) fn clear_box_edge_side(style: &mut ComputedStyle, side: PhysicalSide) {
    match side {
        PhysicalSide::Top => {
            style.border_widths.top = 0.0;
            style.border_styles.top = BorderStyle::None;
            style.padding.top = 0.0;
        }
        PhysicalSide::Right => {
            style.border_widths.right = 0.0;
            style.border_styles.right = BorderStyle::None;
            style.padding.right = 0.0;
        }
        PhysicalSide::Bottom => {
            style.border_widths.bottom = 0.0;
            style.border_styles.bottom = BorderStyle::None;
            style.padding.bottom = 0.0;
        }
        PhysicalSide::Left => {
            style.border_widths.left = 0.0;
            style.border_styles.left = BorderStyle::None;
            style.padding.left = 0.0;
        }
    }
}

pub(in crate::layout) fn opposite_physical_side(side: PhysicalSide) -> PhysicalSide {
    match side {
        PhysicalSide::Top => PhysicalSide::Bottom,
        PhysicalSide::Right => PhysicalSide::Left,
        PhysicalSide::Bottom => PhysicalSide::Top,
        PhysicalSide::Left => PhysicalSide::Right,
    }
}

pub(in crate::layout) fn visual_leading_inline_end_box_edge_width(
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
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    )
            )
        })
        .map(|item| item.width)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn test_text_group() -> PreparedInlineTextGroup {
        PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(InlinePoint::new(0.0, 0.0), 0.0),
            style: ComputedStyle::initial(),
            paint_opacity: 1.0,
            paint_scope_ancestry: Rc::from(Vec::new().into_boxed_slice()),
            link_target: None,
            link_paint_rect: None,
            decoration_paint_rect: None,
            shaped: ShapedInlineLine {
                text: String::new().into(),
                width: 0.0,
                offset: 0.0,
                aligned_by_parley: false,
                line_height: 0.0,
                baseline_adjustment: 0.0,
                runs: Vec::new(),
            },
            source: InlineTextSource::Normal,
            source_run: Rc::new(()),
        }
    }

    #[test]
    fn positions_horizontal_text_group_from_content_bottom_using_descent() {
        let mut group = test_text_group();
        let metrics = InlineTextBoxMetrics {
            content_block_size: 30.0,
            content_baseline_offset: 22.0,
            line_block_size: 10.0,
            block_start_leading: -10.0,
            block_end_leading: -10.0,
            line_baseline_offset: 12.0,
        };

        position_horizontal_text_group_at_content_bottom(&mut group, 100.0, metrics);

        assert_eq!(group.y(), 108.0);
    }

    #[test]
    fn baseline_placement_keeps_the_atomic_block_start_margin() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 50.0;
        let atom = InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(20.0, 100.0),
            40.0,
            0.0,
            None,
            None,
        );
        let parent_metrics = InlineTextBoxMetrics {
            content_block_size: 16.0,
            content_baseline_offset: 12.0,
            line_block_size: 16.0,
            block_start_leading: 0.0,
            block_end_leading: 0.0,
            line_baseline_offset: 12.0,
        };

        let content_bottom = inline_atom_horizontal_content_y(
            &atom,
            &style,
            InlineAtomHorizontalPlacement {
                line_top: 200.0,
                line_height: 100.0,
                line_baseline_offset: 90.0,
                line_rendered_baseline_shift: 0.0,
                content_block_size: 50.0,
                parent_metrics,
            },
        );

        // The line baseline is the margin-box baseline (50 + 40).  The
        // border box therefore begins after the 50px block-start margin,
        // rather than cancelling that margin during paint placement.
        assert_eq!(content_bottom, 150.0);
    }

    #[test]
    fn top_alignment_applies_the_atomic_margin_once() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 16.0;
        style.vertical_align = style.vertical_align.with_baseline_shift(BaselineShift::Top);
        let atom = InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(20.0, 32.0),
            16.0,
            0.0,
            None,
            None,
        );

        let content_bottom = inline_atom_horizontal_content_y(
            &atom,
            &style,
            InlineAtomHorizontalPlacement {
                line_top: 100.0,
                line_height: 48.0,
                line_baseline_offset: 32.0,
                line_rendered_baseline_shift: 0.0,
                content_block_size: 32.0,
                parent_metrics: InlineTextBoxMetrics {
                    content_block_size: 16.0,
                    content_baseline_offset: 12.0,
                    line_block_size: 16.0,
                    block_start_leading: 0.0,
                    block_end_leading: 0.0,
                    line_baseline_offset: 12.0,
                },
            },
        );

        // The 1em outer margin is consumed by the atomic placement. A
        // captured child margin belongs to `content_block_size`; shifting the
        // whole line first would consume the outer margin twice.
        assert_eq!(content_bottom, 52.0);
    }

    #[test]
    fn inline_table_placement_does_not_reapply_wrapper_margin() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 50.0;
        let atom = InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(20.0, 100.0),
            40.0,
            0.0,
            None,
            None,
        )
        .with_exported_table_box_baseline();
        let parent_metrics = InlineTextBoxMetrics {
            content_block_size: 16.0,
            content_baseline_offset: 12.0,
            line_block_size: 16.0,
            block_start_leading: 0.0,
            block_end_leading: 0.0,
            line_baseline_offset: 12.0,
        };

        let content_bottom = inline_atom_horizontal_content_y(
            &atom,
            &style,
            InlineAtomHorizontalPlacement {
                line_top: 200.0,
                line_height: 100.0,
                line_baseline_offset: 40.0,
                line_rendered_baseline_shift: 0.0,
                content_block_size: 50.0,
                parent_metrics,
            },
        );

        // The line aligns to the table-box baseline (40). The inline-table's
        // captured fragment owns its wrapper margin, so this border-box
        // placement must not add the 50px margin again.
        assert_eq!(content_bottom, 150.0);
    }

    #[test]
    fn line_anchor_margin_only_comes_from_baseline_participants() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 16.0;
        let baseline_atom = InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(20.0, 32.0),
            16.0,
            0.0,
            None,
            None,
        );
        assert_eq!(
            inline_atom_line_anchor_block_start_margin(&baseline_atom, &style),
            16.0
        );

        for baseline_shift in [
            BaselineShift::Top,
            BaselineShift::Center,
            BaselineShift::Bottom,
        ] {
            let mut aligned_style = style.clone();
            aligned_style.vertical_align = aligned_style
                .vertical_align
                .with_baseline_shift(baseline_shift.clone());
            let aligned_atom = InlineAtom::new(
                InlineAtomContent::Canvas,
                aligned_style.clone(),
                None,
                InlineSize::new(20.0, 32.0),
                16.0,
                0.0,
                None,
                None,
            );
            assert_eq!(
                inline_atom_line_anchor_block_start_margin(&aligned_atom, &aligned_style),
                0.0,
                "{baseline_shift:?} aligns its own margin box"
            );
        }

        assert_eq!(
            inline_atom_line_anchor_block_start_margin(
                &baseline_atom.with_exported_table_box_baseline(),
                &style,
            ),
            0.0
        );
    }
}
