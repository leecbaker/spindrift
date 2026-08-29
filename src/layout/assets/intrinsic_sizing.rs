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
        placement_axes: FloatPlacementAxes,
        width: f32,
    ) {
        if self
            .preceding_sides
            .iter()
            .copied()
            .any(|preceding| preceding.matches_clear(clear, placement_axes))
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
    style.position.is_out_of_flow_positioned()
        || style.float != Float::None
        || style.display.is_inline_level()
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn estimate_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
            preferred_min: contribution
                .min_content
                .points()
                .max(float_widths.preferred_min),
            preferred: contribution.max_content.points() + float_widths.preferred,
        });
    }

    fn inline_float_widths_in_run_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
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
                    let placement_axes = FloatPlacementAxes::new(
                        self.containing_block_writing_mode,
                        self.containing_block_direction,
                    );
                    if let Some(side) = UsedFloatSide::from_float(child_style.float, placement_axes)
                    {
                        float_run.push(
                            side,
                            child_style.clear,
                            placement_axes,
                            child_width.points(),
                        );
                    }
                    continue;
                }
            }
            if let box_tree::FormattingBox::Inline(box_) = child {
                widths.include(self.inline_float_widths_in_run_boxes(
                    &box_.core.children,
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
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> (f32, f32) {
        // Intrinsic sizing consumes computed CSS lengths. In particular,
        // font-relative values such as `width: 2em` are definite even when
        // the enclosing intrinsic percentage basis is indefinite.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        let source_style = style;
        let used_style = self.style_with_current_used_lengths(style);
        let style = &used_style;
        let containment = used_property_containment(element, style);
        let built_multicol_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (matches!(style.column_count, css::ColumnCount::Count(_))
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
        {
            built_multicol_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                source_style,
            );
            Some(built_multicol_child_boxes.as_slice())
        } else {
            child_boxes
        };
        // Descendants do not contribute to either intrinsic inline size of a
        // size-containment box. Callers add the box's own padding, border, and
        // sizing constraints separately.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if containment.size {
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
        // A replaced element's used content dimensions are its intrinsic
        // contributions. Floats use this same physical-width query for their
        // shrink-to-fit width before the isolated replay paints the object.
        // Do not fall through to the empty-child inline contribution: canvas
        // and SVG elements have no ordinary descendants, but their intrinsic
        // dimensions are still part of their preferred widths.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        let containing_block_height = self
            .block_percentage_context_stack
            .current_percentage_basis();
        let replaced_width = match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => Some(
                used_canvas(
                    element,
                    style,
                    available_width.max(0.0),
                    containing_block_height,
                )
                .content_size
                .width,
            ),
            Some(ReplacedElementKind::Image) => used_image(
                element,
                style,
                available_width.max(0.0),
                containing_block_height,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| image.content_size.width),
            Some(ReplacedElementKind::Svg) => used_svg(
                element,
                style,
                available_width.max(0.0),
                containing_block_height,
            )
            .map(|svg| svg.content_size.width),
            None => None,
        };
        if let Some(width) = replaced_width {
            return (width, width);
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
            let contributions = self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                PhysicalContentWidth::new(content_box_pt(available_width)),
                child_boxes,
            );
            return (
                contributions.min_content.points(),
                contributions.max_content.points(),
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
                        source_style,
                    );
                    &built_child_boxes
                };
                let signature = self
                    .ancestors
                    .last()
                    .cloned()
                    .unwrap_or_else(|| element_signature(element));
                built_fragment = box_tree::build_frozen_table_fragment(
                    element,
                    &signature,
                    style,
                    table_children,
                );
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
        // A negative fixed `text-indent` can legitimately make max-content
        // smaller than min-content: the former measures the first unwrapped
        // line, while the latter measures its longest unbreakable segment.
        // Preserve that exceptional signed contribution through the
        // formatting-context boundary; ordinary contributions retain the
        // traditional max >= min invariant.
        // <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
        let signed_first_line_max_content =
            contribution.max_content.points() < contribution.min_content.points();
        let mut preferred = contribution.max_content.points();
        let mut preferred_min = contribution.min_content.points();
        let establishes_multicol = matches!(style.column_count, css::ColumnCount::Count(_))
            || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
            || matches!(style.column_height, css::ComputedColumnHeight::Length(_));
        let mut multicol_column_preferred = contribution.max_content.points();
        let mut multicol_column_preferred_min = contribution.min_content.points();
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
                            table_box.core.element,
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
                let child_extras = child_metrics.margin.left.points()
                    + child_metrics.margin.right.points()
                    + child_metrics.horizontal_non_content_length().points();
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        let contributions = self.estimate_flex_intrinsic_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            PhysicalContentWidth::new(content_box_pt(available_width)),
                            Some(child_children),
                        );
                        (
                            contributions.min_content.points(),
                            contributions.max_content.points(),
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
                    float_run_width += child_width.points();
                    max_float_run_width = max_float_run_width.max(float_run_width);
                    max_float_width = max_float_width.max(child_width.points());
                    continue;
                }
                float_run_width = 0.0;
                if child_style.display.is_inline_level() {
                    continue;
                }
                let child_metrics = intrinsic_box_metrics(&child_style);
                let child_extras = child_metrics.margin.left.points()
                    + child_metrics.margin.right.points()
                    + child_metrics.horizontal_non_content_length().points();
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        let contributions = self.estimate_flex_intrinsic_widths(
                            child_element,
                            &child_style,
                            stylesheets,
                            PhysicalContentWidth::new(content_box_pt(available_width)),
                            None,
                        );
                        (
                            contributions.min_content.points(),
                            contributions.max_content.points(),
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
            let specified_count = match style.column_count {
                css::ColumnCount::Auto => 1,
                css::ColumnCount::Count(count) => count.get(),
            };
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
        } else if let css::ColumnCount::Count(column_count) = style.column_count
            && column_count.get() > 1
        {
            let column_count = column_count.get();
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

        let preferred = if signed_first_line_max_content {
            preferred.max(0.0)
        } else {
            preferred.max(preferred_min).max(0.0)
        };
        (preferred_min.max(0.0), preferred)
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
                FloatPlacementAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
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
            FloatPlacementAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            20.0,
        );
        run.push(
            UsedFloatSide::Left,
            Clear::Left,
            FloatPlacementAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            30.0,
        );

        let widths = run.finish();
        assert_eq!(widths.preferred_min, 30.0);
        assert_eq!(widths.preferred, 50.0);
    }
}
