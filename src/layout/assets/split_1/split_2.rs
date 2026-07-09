use super::*;

#[derive(Debug, Clone, Copy, Default)]
struct InlineFloatRunIntrinsicWidths {
    preferred_min: f32,
    preferred: f32,
}

impl InlineFloatRunIntrinsicWidths {
    fn include(&mut self, other: Self) {
        self.preferred_min = self.preferred_min.max(other.preferred_min);
        self.preferred = self.preferred.max(other.preferred);
    }
}

/// Source-ordered intrinsic row geometry for floats in one inline run.
///
/// `clear` moves a float's hypothetical border edge below matching preceding
/// float margin boxes. Consequently, cleared floats begin a fresh intrinsic
/// row rather than extending the preceding row's max-content contribution:
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
#[derive(Default)]
struct IntrinsicFloatRun {
    preceding_sides: Vec<UsedFloatSide>,
    row_width: f32,
    widths: InlineFloatRunIntrinsicWidths,
}

impl IntrinsicFloatRun {
    fn push(
        &mut self,
        side: UsedFloatSide,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        width: f32,
    ) {
        if self
            .preceding_sides
            .iter()
            .copied()
            .any(|preceding| preceding.matches_clear(clear, writing_mode, direction))
        {
            self.row_width = 0.0;
        }
        let width = width.max(0.0);
        self.row_width += width;
        self.widths.preferred = self.widths.preferred.max(self.row_width);
        self.widths.preferred_min = self.widths.preferred_min.max(width);
        self.preceding_sides.push(side);
    }

    fn finish(self) -> InlineFloatRunIntrinsicWidths {
        self.widths
    }
}

fn formatting_box_participates_in_inline_float_run(child: &box_tree::FormattingBox<'_>) -> bool {
    if matches!(child, box_tree::FormattingBox::Text(_)) {
        return true;
    }
    let Some((_, _, style, _)) = child.element_parts() else {
        return false;
    };
    matches!(style.position, Position::Absolute | Position::Fixed)
        || style.float != Float::None
        || style.display.is_inline_level()
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn estimate_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: ContentBoxLength,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ContentBoxLength {
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width.points(),
            child_boxes,
            table_fragment,
        );
        intrinsic::shrink_to_fit_width(
            content_box_pt(preferred_min),
            content_box_pt(preferred),
            available_width,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn used_intrinsic_or_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: LayoutLength,
        horizontal_non_content: NonContentLength,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ContentBoxLength {
        let content_available_width =
            content_box_pt((available_width.points() - horizontal_non_content.points()).max(0.0));
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            content_available_width.points(),
            child_boxes,
            table_fragment,
        );
        intrinsic::intrinsic_content_box_width_keyword(
            style.box_values.width.clone(),
            content_box_pt(preferred_min),
            content_box_pt(preferred),
            available_width,
            horizontal_non_content,
        )
        .unwrap_or_else(|| {
            intrinsic::shrink_to_fit_width(
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                content_available_width,
            )
        })
    }

    /// Estimate inline-run intrinsic widths including same-line floats.
    ///
    /// CSS 2.2 shrink-to-fit uses the preferred minimum and preferred widths
    /// of the float or inline-block formatting context. Floats generated in an
    /// inline run are zero-advance markers for line selection, but their
    /// margin boxes still occupy the same line when max-content sizing asks
    /// how wide the line would be without wrapping:
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) fn inline_float_run_intrinsic_widths_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        block_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> (f32, f32) {
        let mut widths = InlineFloatRunIntrinsicWidths::default();
        let mut run_start = None;
        for (index, child) in children.iter().enumerate() {
            if let box_tree::FormattingBox::AnonymousBlock(box_) = child {
                self.flush_inline_float_intrinsic_run(
                    &mut widths,
                    children,
                    run_start.take(),
                    index,
                    block_style,
                    stylesheets,
                    available_width,
                );
                let nested = self.inline_float_run_intrinsic_widths_for_boxes(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                );
                widths.include(InlineFloatRunIntrinsicWidths {
                    preferred_min: nested.0,
                    preferred: nested.1,
                });
                continue;
            }
            if formatting_box_participates_in_inline_float_run(child) {
                run_start.get_or_insert(index);
            } else {
                self.flush_inline_float_intrinsic_run(
                    &mut widths,
                    children,
                    run_start.take(),
                    index,
                    block_style,
                    stylesheets,
                    available_width,
                );
            }
        }
        self.flush_inline_float_intrinsic_run(
            &mut widths,
            children,
            run_start,
            children.len(),
            block_style,
            stylesheets,
            available_width,
        );
        (widths.preferred_min, widths.preferred)
    }

    #[allow(clippy::too_many_arguments)]
    fn flush_inline_float_intrinsic_run(
        &mut self,
        output: &mut InlineFloatRunIntrinsicWidths,
        children: &[box_tree::FormattingBox<'_>],
        run_start: Option<usize>,
        run_end: usize,
        block_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) {
        let Some(run_start) = run_start else {
            return;
        };
        if run_start >= run_end {
            return;
        }
        let run = &children[run_start..run_end];
        let contribution =
            self.intrinsic_inline_contribution_for_boxes(run, block_style, stylesheets);
        let float_widths = self.inline_float_widths_in_run_boxes(run, stylesheets, available_width);
        output.include(InlineFloatRunIntrinsicWidths {
            preferred_min: contribution.min_content.max(float_widths.preferred_min),
            preferred: contribution.max_content + float_widths.preferred,
        });
    }

    fn inline_float_widths_in_run_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> InlineFloatRunIntrinsicWidths {
        let mut widths = InlineFloatRunIntrinsicWidths::default();
        let mut float_run = IntrinsicFloatRun::default();
        for child in children {
            if let Some((child_element, _, child_style, child_children)) = child.element_parts() {
                if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if child_style.float != Float::None {
                    let table_fragment = if let box_tree::FormattingBox::Table(table_box) = child {
                        Some(&table_box.fragment)
                    } else {
                        None
                    };
                    let child_width = self.float_margin_box_width(
                        child_element,
                        child_style,
                        stylesheets,
                        available_width,
                        Some(child_children),
                        table_fragment,
                    );
                    if let Some(side) = UsedFloatSide::from_float(
                        child_style.float,
                        child_style.writing_mode,
                        child_style.direction,
                    ) {
                        float_run.push(
                            side,
                            child_style.clear,
                            child_style.writing_mode,
                            child_style.direction,
                            child_width,
                        );
                    }
                    continue;
                }
            }
            if let box_tree::FormattingBox::Inline(box_) = child {
                widths.include(self.inline_float_widths_in_run_boxes(
                    &box_.children,
                    stylesheets,
                    available_width,
                ));
            }
        }
        widths.include(float_run.finish());
        widths
    }

    pub(in crate::layout) fn formatting_context_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> (f32, f32) {
        let built_multicol_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (style.column_count.is_some()
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
        {
            built_multicol_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_multicol_child_boxes.as_slice())
        } else {
            child_boxes
        };
        // Descendants do not contribute to either intrinsic inline size of a
        // size-containment box. Callers add the box's own padding, border, and
        // sizing constraints separately.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if style.contain.size {
            if let Some(width) = &style.contain_intrinsic_size.width {
                let width = used_length_percentage(
                    width.clone(),
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                )
                .points();
                return (width, width);
            }
            if style.display.is_grid() {
                return self.size_contained_grid_intrinsic_widths(style);
            }
            return size_contained_multicol_intrinsic_inline_sizes(style).unwrap_or((0.0, 0.0));
        }
        // This API supplies a physical width to shrink-to-fit callers such
        // as floats. In a vertical writing mode, the logical inline
        // contribution is physical height; physical width instead projects
        // from the logical block contribution. Keeping that projection at the
        // physical-width boundary prevents a vertical text run's advance from
        // being mistaken for the width of its float margin box.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        if style.writing_mode.has_vertical_lines()
            && !style.display.is_flex()
            && !style.display.is_table()
            && child_boxes.is_some_and(has_direct_inline_content_box)
        {
            let sizes = self.block_intrinsic_content_sizes(
                element,
                style,
                stylesheets,
                child_boxes,
                available_width,
            );
            let (min, max) = sizes.physical_width_min_max(FlowAxes::for_style(style));
            return (
                min.points().max(0.0),
                max.points().max(min.points()).max(0.0),
            );
        }
        if style.display.is_flex() {
            let (preferred_min, preferred) = self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_width,
                child_boxes,
            );
            return (
                preferred_min.max(0.0),
                preferred.max(preferred_min).max(0.0),
            );
        }
        if style.display.is_table() {
            let built_child_boxes;
            let built_fragment;
            let fragment = if let Some(fragment) = table_fragment {
                fragment
            } else {
                let table_children = if let Some(children) = child_boxes {
                    children
                } else {
                    built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let signature = self
                    .ancestors
                    .last()
                    .cloned()
                    .unwrap_or_else(|| element_signature(element));
                built_fragment =
                    box_tree::build_frozen_table_fragment(element, &signature, table_children);
                &built_fragment
            };
            let (preferred_min, preferred) = if style.writing_mode.has_vertical_lines() {
                // A table's grid columns are its logical inline axis. For a
                // vertical root that axis is physical height; a parent
                // formatting context instead needs the projected logical
                // block contribution as its physical width.
                // <https://drafts.csswg.org/css-tables-3/#table-layout>
                // <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
                let (outer_min, outer) = self.table_outer_intrinsic_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                );
                let border_widths = if style.border_collapse == css::BorderCollapse::Collapse {
                    css::Edges::ZERO
                } else {
                    used_border_widths(style)
                };
                let horizontal_non_content = border_widths.left
                    + border_widths.right
                    + style.padding.left
                    + style.padding.right
                    + style.margin.left
                    + style.margin.right;
                (
                    (outer_min - horizontal_non_content).max(0.0),
                    (outer - horizontal_non_content).max(0.0),
                )
            } else {
                self.table_parent_intrinsic_content_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                )
            };
            return (
                preferred_min.max(0.0),
                preferred.max(preferred_min).max(0.0),
            );
        }

        let contribution = if child_boxes.is_some_and(|children| {
            has_non_inline_formatting_box(children)
                && !has_direct_inline_content_box(children)
                && !has_atomic_inline_formatting_box(children)
        }) {
            inline_layout::InlineIntrinsicContribution::default()
        } else {
            self.intrinsic_inline_contribution_for_element(element, style, stylesheets, child_boxes)
        };
        let mut preferred = contribution.max_content;
        let mut preferred_min = contribution.min_content;
        let establishes_multicol = style.column_count.is_some()
            || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
            || matches!(style.column_height, css::ComputedColumnHeight::Length(_));
        let mut multicol_column_preferred = contribution.max_content;
        let mut multicol_column_preferred_min = contribution.min_content;
        let mut multicol_spanner_preferred = 0.0f32;
        let mut multicol_spanner_preferred_min = 0.0f32;

        if let Some(child_boxes) = child_boxes {
            let (inline_float_preferred_min, inline_float_preferred) = self
                .inline_float_run_intrinsic_widths_for_boxes(
                    child_boxes,
                    style,
                    stylesheets,
                    available_width,
                );
            preferred = preferred.max(inline_float_preferred);
            preferred_min = preferred_min.max(inline_float_preferred_min);
            if establishes_multicol {
                multicol_column_preferred = multicol_column_preferred.max(inline_float_preferred);
                multicol_column_preferred_min =
                    multicol_column_preferred_min.max(inline_float_preferred_min);
            }
            for child_box in child_boxes {
                let Some((child_element, _, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                // Out-of-flow positioned descendants affect neither the
                // min-content nor max-content size of their containing
                // formatting context. Their static-position placeholder has
                // zero inline advance and is handled during committed layout.
                // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
                if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if child_style.float != Float::None {
                    continue;
                }
                if child_style.display.is_inline_level() {
                    continue;
                }
                if let box_tree::FormattingBox::Table(table_box) = child_box {
                    let (child_preferred_min, child_preferred) = self
                        .table_outer_intrinsic_widths_from_fragment(
                            table_box.element,
                            child_style,
                            stylesheets,
                            &table_box.fragment,
                            available_width,
                        );
                    preferred = preferred.max(child_preferred);
                    preferred_min = preferred_min.max(child_preferred_min);
                    if establishes_multicol {
                        multicol_column_preferred = multicol_column_preferred.max(child_preferred);
                        multicol_column_preferred_min =
                            multicol_column_preferred_min.max(child_preferred_min);
                    }
                    continue;
                }
                let child_metrics = intrinsic_box_metrics(child_style);
                let child_extras = child_metrics.margin.left
                    + child_metrics.margin.right
                    + child_metrics.horizontal_non_content_length().points();
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        self.estimate_flex_intrinsic_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            available_width,
                            Some(child_children),
                        )
                    } else {
                        // A block container's intrinsic physical width is
                        // contributed by its complete formatting context, not
                        // only its own direct inline run. Recursing also keeps
                        // an orthogonal child's logical inline contribution
                        // (physical height) from being projected onto the
                        // parent's physical-width axis.
                        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
                        // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-flows>
                        self.formatting_context_intrinsic_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            available_width,
                            Some(child_children),
                            None,
                        )
                    };
                let (child_preferred_min, child_preferred) =
                    constrain_non_replaced_intrinsic_widths(
                        child_style,
                        intrinsic_preferred_min,
                        intrinsic_preferred,
                        child_extras,
                    );
                let child_outer_preferred = child_preferred + child_extras;
                let child_outer_preferred_min = child_preferred_min + child_extras;
                preferred = preferred.max(child_outer_preferred);
                preferred_min = preferred_min.max(child_outer_preferred_min);
                if establishes_multicol {
                    if crate::layout::block::formatting_boxes_have_eligible_multicol_spanner(
                        std::slice::from_ref(child_box),
                    ) {
                        multicol_spanner_preferred =
                            multicol_spanner_preferred.max(child_outer_preferred);
                        multicol_spanner_preferred_min =
                            multicol_spanner_preferred_min.max(child_outer_preferred_min);
                    } else {
                        multicol_column_preferred =
                            multicol_column_preferred.max(child_outer_preferred);
                        multicol_column_preferred_min =
                            multicol_column_preferred_min.max(child_outer_preferred_min);
                    }
                }
            }
        } else if preferred <= 0.0 {
            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            let mut float_run_width = 0.0f32;
            let mut max_float_run_width = 0.0f32;
            let mut max_float_width = 0.0f32;
            for child in &element.children {
                let NodeKind::Element(child_element) = &child.kind else {
                    continue;
                };
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = self.style_for_layout_element_with_parent_font_metrics(
                    child_element,
                    signature,
                    stylesheets,
                    Some(style),
                );
                if child_style.float != Float::None {
                    let child_width = self.float_margin_box_width(
                        child_element,
                        &child_style,
                        stylesheets,
                        available_width,
                        None,
                        None,
                    );
                    float_run_width += child_width;
                    max_float_run_width = max_float_run_width.max(float_run_width);
                    max_float_width = max_float_width.max(child_width);
                    continue;
                }
                float_run_width = 0.0;
                if child_style.display.is_inline_level() {
                    continue;
                }
                let child_metrics = intrinsic_box_metrics(&child_style);
                let child_extras = child_metrics.margin.left
                    + child_metrics.margin.right
                    + child_metrics.horizontal_non_content_length().points();
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        self.estimate_flex_intrinsic_widths(
                            child_element,
                            &child_style,
                            stylesheets,
                            available_width,
                            None,
                        )
                    } else {
                        self.formatting_context_intrinsic_widths(
                            child_element,
                            &child_style,
                            stylesheets,
                            available_width,
                            None,
                            None,
                        )
                    };
                let (child_preferred_min, child_preferred) =
                    constrain_non_replaced_intrinsic_widths(
                        &child_style,
                        intrinsic_preferred_min,
                        intrinsic_preferred,
                        child_extras,
                    );
                preferred = preferred.max(child_preferred + child_extras);
                preferred_min = preferred_min.max(child_preferred_min + child_extras);
            }
            preferred = preferred.max(max_float_run_width);
            preferred_min = preferred_min.max(max_float_width);
        }

        if let css::ComputedColumnWidth::Length(column_width) = &style.column_width
            && let Some(column_width) = column_width
                .length_if_no_percent()
                .filter(|width| *width > 0.0)
        {
            // A shrink-to-fit multicol must expose its authored column width
            // to intrinsic sizing. When a definite sequential column row can
            // hold all of the content, only the columns that are actually
            // needed contribute to the preferred width; the used-count
            // algorithm can consequently reduce `column-count` without
            // shrinking a column below `column-width`.
            // <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>
            // <https://www.w3.org/TR/css-sizing-3/#multicol-intrinsic>
            let gap = used_multicol_column_gap(
                style.column_gap.clone(),
                PercentageBasis::definite(content_box_pt(available_width)),
                style.font_size,
            )
            .points();
            let specified_count = style.column_count.unwrap_or(1).max(1);
            let actual_count = if style.column_fill == css::ColumnFill::Auto
                && let Some(column_height) = style
                    .box_values
                    .height
                    .length_if_no_percent()
                    .filter(|height| *height > 0.0)
            {
                let content_height = child_boxes
                    .into_iter()
                    .flatten()
                    .filter_map(|child| {
                        child.element_parts().and_then(
                            |(child_element, _, child_style, child_children)| {
                                self.estimate_element_height(
                                    child_element,
                                    child_style,
                                    stylesheets,
                                    column_width,
                                    Some(child_children),
                                )
                            },
                        )
                    })
                    .sum::<f32>();
                ((content_height / column_height).ceil().max(1.0) as usize).min(specified_count)
            } else {
                specified_count
            };
            let preferred_multicol =
                column_width * actual_count as f32 + gap * actual_count.saturating_sub(1) as f32;
            preferred = preferred_multicol.max(multicol_spanner_preferred);
            preferred_min = preferred_multicol.max(multicol_spanner_preferred_min);
        } else if let Some(column_count) = style.column_count.filter(|count| *count > 1) {
            // With an automatic column width and a definite count, intrinsic
            // max-content sizing accounts for one max-content column per
            // requested column plus the intervening gaps. A spanner instead
            // contributes once across the complete multicol inline size; the
            // promotion classifier above uses the same formatting-context
            // boundaries as committed layout.
            // <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>
            let gap = used_multicol_column_gap(
                style.column_gap.clone(),
                PercentageBasis::definite(content_box_pt(available_width)),
                style.font_size,
            )
            .points();
            let total_gap = gap * column_count.saturating_sub(1) as f32;
            preferred = (multicol_column_preferred * column_count as f32 + total_gap)
                .max(multicol_spanner_preferred);
            preferred_min = (multicol_column_preferred_min * column_count as f32 + total_gap)
                .max(multicol_spanner_preferred_min);
        }

        (
            preferred_min.max(0.0),
            preferred.max(preferred_min).max(0.0),
        )
    }

    pub(in crate::layout) fn measure_auto_positioned_block_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let vertical_border_width_for_positioning =
            self.positioned_vertical_border_width(element, style, stylesheets, table_fragment);
        let snapshot = self.snapshot();
        self.content_left = 0.0;
        self.content_right = width.max(style.font_size);
        let start_page_index = self.pages.len();
        let start_page_context = self.current_page_context;
        self.cursor_y = self.page_top();
        self.containing_blocks
            .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left,
                self.cursor_y,
                self.content_right - self.content_left,
                10_000.0,
            )));
        // Match final absolute-positioned replay: the box is an independent
        // block formatting context, so ambient source-float exclusions cannot
        // inflate its measured auto height.
        // <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>
        self.push_float_context();
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
        self.pop_float_context();
        self.containing_blocks.pop();
        let consumed = self
            .positioned_measurement_fragmented_block_extent(start_page_index, start_page_context);
        self.restore(snapshot);
        // CSS 2.2 absolute positioning equations use content height as the
        // `height` term and add padding/borders separately. Collapsed table
        // borders contribute resolved outer grid insets rather than authored
        // full border widths, so use the same vertical non-content size that
        // will be used by the absolute-position equation.
        (consumed
            - style.padding.top
            - style.padding.bottom
            - vertical_border_width_for_positioning)
            .max(0.0)
    }

    /// Returns the continuous block-axis extent traversed by a positioned
    /// auto-height measurement that may have crossed page fragmentainers.
    ///
    /// Absolutely positioned boxes are laid out as though fragmentation breaks
    /// were absent, then split into fragmentainers. Measuring their auto height
    /// must therefore glue crossed page areas together instead of subtracting
    /// page-local cursor coordinates:
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
    fn positioned_measurement_fragmented_block_extent(
        &mut self,
        start_page_index: usize,
        start_page_context: PageContext,
    ) -> f32 {
        let completed_page_area_height = if self.pages.len() <= start_page_index {
            0.0
        } else {
            let later_page_sizes = self
                .pages
                .iter()
                .skip(start_page_index + 1)
                .map(|page| PageSize::from_points(page.width(), page.height()))
                .collect::<Vec<_>>();
            let mut height = start_page_context.area_height();
            for (offset, page_size) in later_page_sizes.into_iter().enumerate() {
                let page_index = start_page_index + offset + 1;
                height += self
                    .finished_page_context(page_index + 1, page_size)
                    .area_height();
            }
            height
        };

        completed_page_area_height + (self.page_top() - self.cursor_y).max(0.0)
    }

    pub(in crate::layout) fn positioned_vertical_border_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        if is_html_table_element(element) {
            self.collapsed_table_outer_vertical_insets(style, stylesheets, table_fragment)
                .unwrap_or_else(|| vertical_border_width(style))
        } else {
            vertical_border_width(style)
        }
    }

    pub(in crate::layout) fn page_containing_block(&self) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(
            self.page_left(),
            self.page_top(),
            self.page_area_width(),
            // Out-of-flow layout suppresses fragmentation while collecting
            // descendants, but it must not enlarge the initial containing
            // block. Its physical dimensions are always the current page
            // area's dimensions, including for percentage block sizes:
            // <https://www.w3.org/TR/css-display-3/#initial-containing-block>
            // and <https://www.w3.org/TR/css-position-3/#def-cb>.
            self.current_page_context.area_height(),
        ))
        // This is the initial containing block. Explicit absolute offsets are
        // measured from the initial page, even when ordinary flow has already
        // generated later pages before the out-of-flow element is collected.
        // Auto static positions retain their source fragment separately in
        // `layout_positioned_block`.
        // <https://www.w3.org/TR/css-position-3/#containing-block>
        .on_page(0)
    }

    pub(in crate::layout) fn current_containing_block(&self) -> ContainingBlock {
        self.containing_blocks
            .last()
            .cloned()
            .unwrap_or_else(|| self.page_containing_block())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleared_floats_start_a_new_intrinsic_row() {
        let mut run = IntrinsicFloatRun::default();
        for clear in [Clear::None, Clear::Both, Clear::Both] {
            run.push(
                UsedFloatSide::Left,
                clear,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                30.0,
            );
        }

        let widths = run.finish();
        assert_eq!(widths.preferred_min, 30.0);
        assert_eq!(widths.preferred, 30.0);
    }

    #[test]
    fn unmatched_clear_keeps_floats_in_the_same_intrinsic_row() {
        let mut run = IntrinsicFloatRun::default();
        run.push(
            UsedFloatSide::Right,
            Clear::None,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            20.0,
        );
        run.push(
            UsedFloatSide::Left,
            Clear::Left,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            30.0,
        );

        let widths = run.finish();
        assert_eq!(widths.preferred_min, 30.0);
        assert_eq!(widths.preferred, 50.0);
    }
}
