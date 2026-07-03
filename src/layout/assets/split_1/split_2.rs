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
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
            table_fragment,
        );
        intrinsic::shrink_to_fit_width(preferred_min, preferred, available_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn used_intrinsic_or_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        horizontal_non_content: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let content_available_width = (available_width - horizontal_non_content).max(0.0);
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            content_available_width,
            child_boxes,
            table_fragment,
        );
        intrinsic::intrinsic_width_keyword(
            style.box_values.width,
            preferred_min,
            preferred,
            available_width,
            horizontal_non_content,
        )
        .unwrap_or_else(|| {
            intrinsic::shrink_to_fit_width(preferred_min, preferred, content_available_width)
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
                    widths.preferred += child_width;
                    widths.preferred_min = widths.preferred_min.max(child_width);
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
            let (preferred_min, preferred) = self
                .table_parent_intrinsic_content_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                );
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
            for child_box in child_boxes {
                let Some((child_element, _, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
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
                    continue;
                }
                let child_extras = child_style.margin.left
                    + child_style.margin.right
                    + child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(child_style);
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
                        let contribution = self.intrinsic_inline_contribution_for_element(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                        );
                        (contribution.min_content, contribution.max_content)
                    };
                let (child_preferred_min, child_preferred) =
                    constrain_non_replaced_intrinsic_widths(
                        child_style,
                        intrinsic_preferred_min,
                        intrinsic_preferred,
                        child_extras,
                    );
                preferred = preferred.max(child_preferred + child_extras);
                preferred_min = preferred_min.max(child_preferred_min + child_extras);
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
                let child_extras = child_style.margin.left
                    + child_style.margin.right
                    + child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(&child_style);
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
                        let contribution = self.intrinsic_inline_contribution_for_element(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                        );
                        (contribution.min_content, contribution.max_content)
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
        self.cursor_y = self.page_bottom() + 10_000.0;
        let start_y = self.cursor_y;
        self.containing_blocks
            .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left,
                self.cursor_y,
                self.content_right - self.content_left,
                10_000.0,
            )));
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
        self.containing_blocks.pop();
        let consumed = (start_y - self.cursor_y).max(0.0);
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
            self.page_area_height(),
        ))
    }

    pub(in crate::layout) fn current_containing_block(&self) -> ContainingBlock {
        self.containing_blocks
            .last()
            .copied()
            .unwrap_or_else(|| self.page_containing_block())
    }
}
