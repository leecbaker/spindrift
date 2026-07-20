use super::generated_content::annotate_line_break_element_breaks;
use super::*;
use std::rc::Rc;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn intrinsic_inline_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        // Atomic inline dimensions participate directly in their containing
        // line's intrinsic contribution. Resolve viewport and font-relative
        // components before that contribution is captured; otherwise a
        // vertical `height: 20vh` is reduced to its line strut and cannot
        // negotiate against the orthogonal flow's available inline size.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flow>
        let used_style = self.style_with_current_used_lengths(style);
        let style = &used_style;
        let intrinsic_metrics = intrinsic_box_metrics(style);
        let available_width = (self.content_right
            - self.content_left
            - intrinsic_metrics.margin.left.points()
            - intrinsic_metrics.margin.right.points())
        .max(0.0);
        let inline_percentage_basis = self
            .intrinsic_inline_percentage_basis_stack
            .last()
            .cloned()
            .unwrap_or_else(|| {
                PercentageBasis::definite_from(
                    content_box_pt(available_width),
                    IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                )
            });
        if let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        {
            let image = used_generated_image_value(
                image.as_image()?,
                style,
                available_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )?;
            let border_box_width = image.border_box_size.width;
            let border_box_height = image.border_box_size.height;
            let content = image
                .svg
                .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                .unwrap_or(InlineAtomContent::Image(image.decoded));
            return Some(
                InlineAtom::new(
                    content,
                    style.clone(),
                    None,
                    InlineSize::new(
                        border_box_width
                            + intrinsic_metrics.margin.left.points()
                            + intrinsic_metrics.margin.right.points(),
                        border_box_height
                            + intrinsic_metrics.margin.top.points()
                            + intrinsic_metrics.margin.bottom.points(),
                    ),
                    border_box_height,
                    baseline_shift,
                    link_target,
                    self.generated_alt_text(element, style),
                )
                .with_visual_offset(visual_offset),
            );
        }
        let (width, height, baseline_offset) = match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let containing_block_height = self
                    .definite_block_size_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(PercentageBasis::indefinite);
                let canvas = used_canvas_with_inline_percentage_basis(
                    element,
                    style,
                    available_width,
                    inline_percentage_basis,
                    containing_block_height,
                );
                let border_box_width = canvas.border_box_size.width;
                let border_box_height = canvas.border_box_size.height;
                (
                    border_box_width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    border_box_height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    border_box_height,
                )
            }
            Some(ReplacedElementKind::Image) => used_image_with_inline_percentage_basis(
                element,
                style,
                IntrinsicInlineImageSizingContext {
                    available_width: content_box_pt(available_width),
                    inline_percentage_basis,
                    height_basis: self
                        .definite_block_size_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(PercentageBasis::indefinite),
                },
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                (
                    border_box_width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    border_box_height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    border_box_height,
                )
            })?,
            Some(ReplacedElementKind::Svg) => {
                let svg = used_svg(
                    element,
                    style,
                    available_width,
                    self.definite_block_size_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(PercentageBasis::indefinite),
                )?;
                let width = svg.border_box_size.width;
                let height = svg.border_box_size.height;
                (
                    width
                        + intrinsic_metrics.margin.left.points()
                        + intrinsic_metrics.margin.right.points(),
                    height
                        + intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points(),
                    height,
                )
            }
            None if style.display.is_table() => {
                let fragment = table_fragment?;
                let box_metrics = intrinsic_box_metrics(style);
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let (min_width, width) = self.table_parent_intrinsic_content_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                );
                let content_width = intrinsic::shrink_to_fit_width(
                    content_box_pt(min_width),
                    content_box_pt(width),
                    content_box_pt((available_width - horizontal_extras).max(0.0)),
                )
                .points();
                (
                    constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                        + horizontal_extras
                        + box_metrics.margin.left.points()
                        + box_metrics.margin.right.points(),
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_flex() && style.display.is_inline_level() => {
                let box_metrics = intrinsic_box_metrics(style);
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let contributions = self.estimate_flex_intrinsic_widths(
                    element,
                    style,
                    stylesheets,
                    PhysicalContentWidth::new(content_box_pt(available_width)),
                    Some(children),
                );
                let content_width = intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_width),
                    non_content_pt(horizontal_extras),
                    contributions.min_content.content_box_length(),
                    contributions.max_content.content_box_length(),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
                .points();
                (
                    constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                        + horizontal_extras
                        + box_metrics.margin.left.points()
                        + box_metrics.margin.right.points(),
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_grid() && style.display.is_inline_level() => {
                return Some(self.intrinsic_inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                ));
            }
            None if style.display.is_atomic_inline() => {
                let box_metrics = intrinsic_box_metrics(style);
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let vertical_extras = box_metrics.vertical_non_content_length().points();
                let containing_block_height = self
                    .definite_block_size_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(PercentageBasis::indefinite);
                let definite_content_height = used_content_box_height_or_auto_with_basis(
                    style,
                    containing_block_height,
                    non_content_pt(vertical_extras),
                )
                .map(|height| {
                    constrain_content_height(
                        style,
                        height,
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                });
                self.definite_block_size_stack
                    .push(block_size_percentage_basis_from_points(
                        definite_content_height,
                        BlockSizeBasisSource::InlineBlock,
                    ));
                let contribution = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    self.intrinsic_inline_measurement_for_text(&text, style, available_width)
                        .contribution
                } else {
                    self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                    )
                };
                self.definite_block_size_stack.pop();
                let content_width = intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_width),
                    non_content_pt(horizontal_extras),
                    contribution.min_content.content_box_length(),
                    contribution.max_content.content_box_length(),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
                .points();
                let mut content_width = if style.box_values.width.is_auto() {
                    content_width.max(style.font_size)
                } else {
                    content_width
                };
                let (measured_content_height, measured_physical_height) = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    let measurement =
                        self.intrinsic_inline_measurement_for_text(&text, style, content_width);
                    (
                        measurement.height().max(style.line_height),
                        measurement.physical_height(style),
                    )
                } else {
                    self.definite_block_size_stack
                        .push(block_size_percentage_basis_from_points(
                            definite_content_height,
                            BlockSizeBasisSource::InlineBlock,
                        ));
                    let measurement = self.intrinsic_inline_measurement_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                        content_width,
                    );
                    (
                        measurement.height().max(style.line_height),
                        measurement.physical_height(style),
                    )
                };
                if !children.is_empty() {
                    self.definite_block_size_stack.pop();
                }
                let vertical_writing_mode = style.writing_mode.has_vertical_lines();
                let has_intrinsic_inline_content =
                    !children.is_empty() || !inline_text_for_style(element, style).is_empty();
                if vertical_writing_mode
                    && style.box_values.width.is_auto()
                    && has_intrinsic_inline_content
                {
                    // A vertical inline-block's physical width is its logical
                    // block contribution, i.e. the stacked line-box extent.
                    // Its physical height is instead the used logical inline
                    // extent below. Keeping those projections distinct makes
                    // intrinsic atom sizing agree with final inline paint.
                    // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                    content_width = measured_content_height;
                }
                let content_height = if vertical_writing_mode {
                    definite_content_height.unwrap_or(measured_physical_height)
                } else {
                    definite_content_height.unwrap_or_else(|| {
                        constrain_content_height(
                            style,
                            content_box_pt(measured_content_height),
                            PercentageBasis::definite(layout_pt(available_width)),
                        )
                        .points()
                    })
                };
                let border_box_height = content_height + vertical_extras;
                (
                    constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                        + horizontal_extras
                        + box_metrics.margin.left.points()
                        + box_metrics.margin.right.points(),
                    border_box_height
                        + box_metrics.margin.top.points()
                        + box_metrics.margin.bottom.points(),
                    border_box_height,
                )
            }
            None => return None,
        };
        Some(
            InlineAtom::new(
                InlineAtomContent::Svg { asset: None },
                style.clone(),
                None,
                InlineSize::new(width, height),
                baseline_offset,
                baseline_shift,
                link_target,
                None,
            )
            .with_visual_offset(visual_offset),
        )
    }

    pub(in crate::layout) fn inline_static_baseline_y_from_buffer(
        &mut self,
        output: &[InlineItem],
        fallback_style: &ComputedStyle,
    ) -> f32 {
        if let Some(atom) = output.iter().rev().find_map(|item| match item {
            InlineItem::Atom(atom) if !atom.content().is_inline_edge() => Some(atom),
            _ => None,
        }) {
            let borders = used_border_widths(atom.style());
            let atom_baseline_offset =
                atom.style().margin.top + atom.baseline_offset - atom.baseline_shift;
            let parent_baseline_offset = self
                .font_system
                .rendered_first_line_baseline_offset(atom.style())
                .points();
            let line_baseline_offset = atom_baseline_offset.max(parent_baseline_offset);
            return self.cursor_y - line_baseline_offset
                + atom.baseline_offset
                + atom.baseline_shift
                - borders.top
                - atom.style().padding.top
                - atom.style().font_size;
        }

        self.cursor_y
            - self
                .font_system
                .rendered_first_line_baseline_offset(fallback_style)
                .points()
    }

    pub(in crate::layout) fn block_static_position_y_offset_from_buffer(
        &mut self,
        output: &[InlineItem],
        block_style: &ComputedStyle,
    ) -> f32 {
        // Zero-sized split inline edge atoms preserve decoration boundaries,
        // but do not themselves occupy the line that selects the static
        // position. Nonzero edge atoms, such as an inline-start border before
        // a block-in-inline split, still create the hypothetical line that
        // precedes the block-level positioned box.
        let has_buffered_content = output.iter().any(|item| match item {
            InlineItem::Word(_) => !inline_item_is_collapsible_space(item),
            InlineItem::Atom(atom) => {
                !atom.content().is_inline_edge() || atom.size.width > 0.0 || atom.size.height > 0.0
            }
            InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
                false
            }
            // A forced line break has no inline advance, but it does create
            // a line box. Its hypothetical block position is therefore part
            // of the static-position rectangle for a following block-level
            // positioned descendant.
            // <https://www.w3.org/TR/css-position-3/#staticpos-rect>
            InlineItem::Break(_) => true,
        });
        if !has_buffered_content {
            return 0.0;
        }
        let available_width = self.current_content_logical_inline_size().max(1.0);
        // CSS Positioned Layout removes the abspos from flow, but CSS 2.2
        // computes auto inset static position from its hypothetical normal-flow
        // box. For a block-level source after inline content, keep a
        // non-painting placeholder in the buffered run on the next line so
        // whitespace, wrapping, and line metrics are measured by the same inline
        // machinery as real content:
        // https://www.w3.org/TR/css-position-3/#absolute-positioning
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        let mut hypothetical_items = output.to_vec();
        if !matches!(hypothetical_items.last(), Some(InlineItem::Break(_))) {
            hypothetical_items.push(InlineItem::Break(InlineBreak::default()));
        }
        hypothetical_items.push(InlineItem::Atom(Box::new(
            self.block_static_position_placeholder_atom(block_style),
        )));
        // The placeholder sequence is measurement only. In particular,
        // buffered source floats must not be registered a second time while
        // determining an absolute box's block static position.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        let snapshot = self.snapshot();
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.restore(snapshot);
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let mut offset = 0.0;
        for record in &records {
            if record.fragment.as_ref().is_some_and(|fragment| {
                fragment.items().iter().any(|item| {
                    matches!(
                        &item.item,
                        InlineLineItem::Atom(atom)
                            if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                    )
                })
            }) {
                return offset;
            }
            offset += record.height();
        }
        offset
    }

    /// Builds a non-painting line-selection atom for the block-level static
    /// position of an absolutely positioned box.
    ///
    /// CSS Positioned Layout resolves auto insets from the hypothetical
    /// normal-flow static-position rectangle; for block-level sources inside
    /// inline collection, that rectangle starts after the preceding line boxes:
    /// <https://www.w3.org/TR/css-position-3/#staticpos-rect> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
    pub(in crate::layout) fn block_static_position_placeholder_atom(
        &mut self,
        block_style: &ComputedStyle,
    ) -> InlineAtom {
        self.block_static_position_placeholder_atom_with_inline_size(block_style, 0.0)
    }

    /// Builds a non-painting static-position atom with an explicit inline
    /// footprint. Split inline floats use this to participate in the line
    /// selection that determines their source block position.
    pub(in crate::layout) fn block_static_position_placeholder_atom_with_inline_size(
        &mut self,
        block_style: &ComputedStyle,
        inline_size: f32,
    ) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            block_style.clone(),
            None,
            InlineSize::new(inline_size.max(0.0), block_style.line_height),
            self.font_system
                .rendered_first_line_baseline_offset(block_style)
                .points(),
            0.0,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_generated_pseudo_items(
        &mut self,
        element: &Element,
        originating_style: &ComputedStyle,
        pseudo_style: Option<&ComputedStyle>,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        counter_mode: GeneratedPseudoCounterMode,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(pseudo_style) = pseudo_style else {
            return;
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return;
        };
        let counter_snapshot = (counter_mode == GeneratedPseudoCounterMode::Rollback)
            .then(|| self.counter_set.clone());
        let source = if originating_style
            .before_style
            .as_deref()
            .is_some_and(|before| std::ptr::eq(before, pseudo_style))
        {
            box_tree::CounterEventSource::Before
        } else {
            box_tree::CounterEventSource::After
        };
        let counter_scope = self.begin_pseudo_counter_scope(element, source, pseudo_style);
        let alt_text = self.generated_alt_text(element, pseudo_style);
        let visual_offset = visual_offset.plus(self.inline_visual_offset_for_style(pseudo_style));
        let is_block = pseudo_style.display.is_block_level();
        if is_block
            && output
                .last()
                .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
        {
            trim_trailing_inline_spaces(output);
            output.push(InlineItem::Break(InlineBreak::default()));
        }
        let start_len = output.len();
        self.push_bidi_scope_start(
            pseudo_style,
            link_target.clone(),
            baseline_shift,
            visual_offset,
            output,
        );
        let scope_start_len = output.len();
        for part in content {
            self.push_generated_content_part(
                element,
                part,
                pseudo_style,
                source,
                link_target.clone(),
                baseline_shift,
                visual_offset,
                alt_text.clone(),
                output,
            );
        }
        annotate_line_break_element_breaks(element, originating_style, output, scope_start_len);
        let emitted_content = output.len() > scope_start_len;
        if emitted_content {
            self.push_bidi_scope_end(
                pseudo_style,
                link_target,
                baseline_shift,
                visual_offset,
                output,
            );
        } else {
            output.truncate(start_len);
        }
        if emitted_content
            && is_block
            && output
                .last()
                .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
        {
            trim_trailing_inline_spaces(output);
            output.push(InlineItem::Break(InlineBreak::default()));
        }
        self.end_counter_scope(counter_scope);
        if let Some(counter_snapshot) = counter_snapshot {
            self.counter_set = counter_snapshot;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_generated_content_part(
        &mut self,
        element: &Element,
        part: &GeneratedContentPart,
        style: &ComputedStyle,
        source: box_tree::CounterEventSource,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        alt_text: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        match part {
            GeneratedContentPart::Text(text) => {
                push_generated_inline_words_for_style(
                    text,
                    style,
                    link_target,
                    baseline_shift,
                    visual_offset,
                    output,
                );
            }
            GeneratedContentPart::Contents => {
                let text = inline_text_for_style(element, style);
                self.push_inline_words(
                    &text,
                    style,
                    link_target,
                    baseline_shift,
                    visual_offset,
                    output,
                );
            }
            GeneratedContentPart::Attr { .. }
            | GeneratedContentPart::Counter { .. }
            | GeneratedContentPart::Counters { .. } => {
                let counter_stacks = self.counter_stacks_at_origin(element, source);
                let text = evaluate_generated_content_text(
                    element,
                    std::slice::from_ref(part),
                    &counter_stacks,
                    &self.counter_styles,
                );
                push_generated_inline_words_for_style(
                    &text,
                    style,
                    link_target,
                    baseline_shift,
                    visual_offset,
                    output,
                );
            }
            GeneratedContentPart::TargetCounter { .. }
            | GeneratedContentPart::TargetText { .. } => {}
            GeneratedContentPart::Quote(quote) => {
                let text = self.generated_quote_text(*quote, style);
                push_generated_inline_words_for_style(
                    &text,
                    style,
                    link_target,
                    baseline_shift,
                    visual_offset,
                    output,
                );
            }
            GeneratedContentPart::Leader(text) => {
                output.push(InlineItem::Atom(Box::new(
                    InlineAtom::new(
                        InlineAtomContent::Leader(text.clone()),
                        style.clone(),
                        None,
                        InlineSize::new(0.0, style.line_height),
                        style.font_size,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )));
            }
            GeneratedContentPart::Image { image } => {
                if let Some(atom) = image.as_image().and_then(|image| {
                    self.generated_image_atom_for_image(
                        image,
                        style,
                        baseline_shift,
                        visual_offset,
                        link_target,
                        alt_text,
                    )
                }) {
                    output.push(InlineItem::Atom(Box::new(atom)));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn generated_image_atom_for_image(
        &mut self,
        image_value: &BackgroundImage,
        style: &ComputedStyle,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Option<InlineAtom> {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
        let style = &used_style;
        let image = used_generated_image_value(
            image_value,
            style,
            available_width,
            self.base_url,
            self.root_url,
            self.resource_cache,
        )?;
        let border_box_width = image.border_box_size.width;
        let border_box_height = image.border_box_size.height;
        let content = image
            .svg
            .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
            .unwrap_or(InlineAtomContent::Image(image.decoded));
        Some(
            InlineAtom::new(
                content,
                style.clone(),
                None,
                InlineSize::new(
                    border_box_width + style.margin.left + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
                ),
                border_box_height,
                baseline_shift,
                link_target,
                alt_text,
            )
            .with_visual_offset(visual_offset),
        )
    }

    pub(in crate::layout) fn generated_alt_text(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Option<String> {
        style.content.alt().map(|alt| {
            evaluate_generated_alt_text(
                element,
                alt,
                &self.counter_set.stacks(),
                &self.counter_styles,
            )
        })
    }

    pub(in crate::layout) fn generated_quote_text(
        &mut self,
        quote: GeneratedQuote,
        style: &ComputedStyle,
    ) -> String {
        match quote {
            GeneratedQuote::Open => {
                let text = quote_pair(style, self.quote_depth).0;
                self.quote_depth += 1;
                text
            }
            GeneratedQuote::Close => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                quote_pair(style, self.quote_depth).1
            }
            GeneratedQuote::NoOpen => {
                self.quote_depth += 1;
                String::new()
            }
            GeneratedQuote::NoClose => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                String::new()
            }
        }
    }

    /// Push UBA start controls for a CSS `unicode-bidi` inline scope.
    ///
    /// CSS Writing Modes defines `unicode-bidi` as adding embedding,
    /// isolation, override, or plaintext bidi controls around generated inline
    /// boxes:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
    pub(in crate::layout) fn push_bidi_scope_start(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_start_with_source(
            style,
            link_target,
            baseline_shift,
            visual_offset,
            InlineTextSource::Normal,
            output,
        );
    }

    pub(in crate::layout) fn push_bidi_scope_start_with_source(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((start, _)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(
                start,
                style,
                link_target,
                InlinePlacement::new(baseline_shift, visual_offset),
                source,
                output,
            );
        }
    }

    /// Push UBA end controls for a CSS `unicode-bidi` inline scope.
    ///
    /// CSS Writing Modes scopes embedding, isolation, and override controls to
    /// the element's inline box and terminates them with UAX #9 PDF/PDI
    /// controls:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
    pub(in crate::layout) fn push_bidi_scope_end(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_end_with_source(
            style,
            link_target,
            baseline_shift,
            visual_offset,
            InlineTextSource::Normal,
            output,
        );
    }

    pub(in crate::layout) fn push_bidi_scope_end_with_source(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((_, end)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(
                end,
                style,
                link_target,
                InlinePlacement::new(baseline_shift, visual_offset),
                source,
                output,
            );
        }
    }

    /// Push invisible bidi control text without CSS text transforms.
    ///
    /// Directional formatting controls are UAX #9 algorithmic input; they
    /// affect ordering but do not create visible CSS text or PDF glyphs:
    /// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
    pub(in crate::layout) fn push_bidi_control_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        placement: InlinePlacement,
        _source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if !text.is_empty() {
            output.push(InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: inline_style(style),
                baseline_shift: placement.baseline_shift,
                visual_offset: placement.visual_offset,
                link_target: link_target.map(Rc::from),
                mergeable: true,
                // This is CSS-generated UAX #9 input, rather than authored
                // text. Retaining that provenance lets later line selection
                // balance only these controls across a soft wrap.
                source: InlineTextSource::BidiControl,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
    }

    pub(in crate::layout) fn push_inline_words(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        push_inline_words_for_style(
            text,
            style,
            link_target,
            baseline_shift,
            visual_offset,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_atom_for_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        if let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        {
            let alt_text = self.generated_alt_text(element, style);
            return image.as_image().and_then(|image| {
                self.generated_image_atom_for_image(
                    image,
                    style,
                    baseline_shift,
                    visual_offset,
                    link_target,
                    alt_text,
                )
            });
        }
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut style = self.style_with_current_viewport_lengths(style);
                let metrics = apply_used_box_metrics(
                    &mut style,
                    PercentageBasis::definite(layout_pt(available_width)),
                );
                let containing_block_height = self
                    .definite_block_size_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(PercentageBasis::indefinite);
                let canvas = used_canvas(element, &style, available_width, containing_block_height);
                let content = if element.tag.eq_ignore_ascii_case("iframe") {
                    self.resource_cache.record_iframe_viewport(
                        element.id,
                        canvas.content_size.width,
                        canvas.content_size.height,
                    );
                    InlineAtomContent::Iframe(element.id)
                } else {
                    InlineAtomContent::Canvas
                };
                let border_box_width = canvas.border_box_size.width;
                let border_box_height = canvas.border_box_size.height;
                let atom_width =
                    border_box_width + metrics.margin.left.points() + metrics.margin.right.points();
                Some(
                    InlineAtom::new(
                        content,
                        style,
                        None,
                        InlineSize::new(
                            atom_width,
                            border_box_height
                                + metrics.margin.top.points()
                                + metrics.margin.bottom.points(),
                        ),
                        border_box_height,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            Some(ReplacedElementKind::Image) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut used_style = self.style_with_current_viewport_lengths(style);
                apply_used_box_metrics(
                    &mut used_style,
                    PercentageBasis::definite(layout_pt(available_width)),
                );
                let style = &used_style;
                let image = used_image(
                    element,
                    style,
                    available_width,
                    self.definite_block_size_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(PercentageBasis::indefinite),
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?;
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                let content = image
                    .svg
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or(InlineAtomContent::Image(image.decoded));
                Some(
                    InlineAtom::new(
                        content,
                        style.clone(),
                        None,
                        InlineSize::new(
                            border_box_width + style.margin.left + style.margin.right,
                            border_box_height + style.margin.top + style.margin.bottom,
                        ),
                        border_box_height,
                        baseline_shift,
                        link_target,
                        element.attrs.get("alt").cloned(),
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            Some(ReplacedElementKind::Svg) => {
                let asset = self.resource_cache.inline_svg_asset(element)?;
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut style = self.style_with_current_viewport_lengths(style);
                let metrics = apply_used_box_metrics(
                    &mut style,
                    PercentageBasis::definite(layout_pt(available_width)),
                );
                let svg = used_svg(
                    element,
                    &style,
                    available_width,
                    self.definite_block_size_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(PercentageBasis::indefinite),
                )?;
                let width = svg.border_box_size.width;
                let height = svg.border_box_size.height;
                Some(
                    InlineAtom::new(
                        InlineAtomContent::Svg { asset: Some(asset) },
                        style,
                        None,
                        InlineSize::new(
                            width + metrics.margin.left.points() + metrics.margin.right.points(),
                            height + metrics.margin.top.points() + metrics.margin.bottom.points(),
                        ),
                        height,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            None if style.display.is_table() => self
                .inline_table_atom_for_element(
                    element,
                    style,
                    children,
                    table_fragment?,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .map(|atom| atom.with_visual_offset(visual_offset)),
            None if style.display.is_flex() && style.display.is_inline_level() => Some(
                self.inline_flex_atom_for_element(
                    element,
                    signature,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .with_visual_offset(visual_offset),
            ),
            None if style.display.is_grid() && style.display.is_inline_level() => Some(
                self.inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .with_visual_offset(visual_offset),
            ),
            None if style.display.is_atomic_inline() => {
                if has_non_inline_formatting_box(children)
                    || has_atomic_inline_formatting_box(children)
                    || has_inline_container_formatting_box(children)
                    || has_out_of_flow_formatting_box(children)
                {
                    return Some(
                        self.inline_fragment_atom_for_children(
                            Some(element),
                            style,
                            children,
                            stylesheets,
                            baseline_shift,
                            link_target,
                        )
                        .with_visual_offset(visual_offset),
                    );
                }
                let available_width = (self.content_right
                    - self.content_left
                    - style.margin.left
                    - style.margin.right)
                    .max(0.0);
                let mut used_style = self.style_with_current_used_lengths(style);
                let box_metrics = apply_used_box_metrics(
                    &mut used_style,
                    PercentageBasis::definite(layout_pt(available_width)),
                );
                let style = &used_style;
                let border_widths = box_metrics.border.to_css_edges();
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let vertical_extras = box_metrics.vertical_non_content_length().points();
                let containing_block_height = self
                    .definite_block_size_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(PercentageBasis::indefinite);
                let definite_content_height = used_content_box_height_or_auto_with_basis(
                    style,
                    containing_block_height,
                    non_content_pt(vertical_extras),
                )
                .map(|height| {
                    constrain_content_height(
                        style,
                        height,
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                });
                self.definite_block_size_stack
                    .push(block_size_percentage_basis_from_points(
                        definite_content_height,
                        BlockSizeBasisSource::InlineBlock,
                    ));
                let intrinsic = self.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    Some(children),
                    available_width,
                );
                // This is the used size of an atomic inline box. Resolve a
                // specified percentage against the definite line containing
                // block; reserve intrinsic shrink-to-fit for `auto` only.
                // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>
                let requested_content_width = used_content_box_width_or_auto(
                    style,
                    layout_pt(available_width),
                    non_content_pt(horizontal_extras),
                )
                .unwrap_or_else(|| {
                    intrinsic::content_box_width_from_intrinsic(
                        style,
                        layout_pt(available_width),
                        non_content_pt(horizontal_extras),
                        intrinsic.contribution.min_content.content_box_length(),
                        intrinsic.contribution.max_content.content_box_length(),
                        intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                    )
                });
                let mut content_width = constrain_content_width(
                    style,
                    requested_content_width,
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                )
                .points();
                let mut sequence_items = Vec::new();
                let mut outside_marker = None;
                if style.display.is_list_item()
                    && let Some(marker) =
                        self.marker_for_list_item(element, style, self.containing_block_direction)
                {
                    if marker.participates_in_first_line() {
                        self.push_inside_marker_items(
                            &marker,
                            style,
                            link_target.clone(),
                            &mut sequence_items,
                        );
                    } else {
                        outside_marker = Some(marker);
                    }
                }
                self.push_generated_pseudo_items(
                    element,
                    style,
                    style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                if style.content.is_generated() {
                    self.push_element_content_items_from_boxes(
                        element,
                        style,
                        box_tree::CounterEventSource::Principal,
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        style,
                        style.text_decoration.clone(),
                        &mut sequence_items,
                    );
                } else {
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        style,
                        style.text_decoration.clone(),
                        &mut sequence_items,
                    );
                }
                self.push_generated_pseudo_items(
                    element,
                    style,
                    style.after_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                self.definite_block_size_stack.pop();
                let vertical_writing_mode = style.writing_mode.has_vertical_lines();
                // A physical `width` is the logical block size in vertical
                // writing. Select the line's logical inline measure from
                // `height` (or its shrink-to-fit intrinsic contribution),
                // then derive the physical width from the resulting wrapped
                // logical block contribution.
                // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                let logical_inline_measure = if vertical_writing_mode {
                    definite_content_height.unwrap_or_else(|| {
                        intrinsic::content_box_width_from_intrinsic(
                            style,
                            layout_pt(available_width),
                            non_content_pt(horizontal_extras),
                            intrinsic.contribution.min_content.content_box_length(),
                            intrinsic.contribution.max_content.content_box_length(),
                            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                        )
                        .points()
                    })
                } else {
                    content_width
                };
                let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                    sequence_items,
                    style,
                    logical_inline_measure,
                    0.0,
                    0.0,
                );
                let measured_logical_block_size = if vertical_writing_mode {
                    sequence
                        .records
                        .iter()
                        .map(|record| record.block_before + record.height().max(style.line_height))
                        .sum::<f32>()
                        .max(0.0)
                } else {
                    sequence.total_height().max(0.0)
                };
                if vertical_writing_mode && style.box_values.width.is_auto() {
                    content_width = constrain_content_width(
                        style,
                        content_box_pt(measured_logical_block_size),
                        PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                    )
                    .points();
                }
                // CSS Sizing applies `height` to the content box; line-height
                // can overflow explicit-height inline-blocks but must not
                // increase their used height:
                // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
                let content_height = if vertical_writing_mode {
                    definite_content_height.unwrap_or(logical_inline_measure)
                } else {
                    definite_content_height.unwrap_or_else(|| {
                        constrain_content_height(
                            style,
                            content_box_pt(if style.contain.size {
                                0.0
                            } else {
                                measured_logical_block_size
                            }),
                            PercentageBasis::definite(layout_pt(available_width)),
                        )
                        .points()
                    })
                };
                let border_box_height = content_height + vertical_extras;
                let line_baseline_offset =
                    self.inline_box_sequence_baseline_offset(&sequence, style, border_widths);
                let baseline_offset = Self::inline_block_baseline_offset(
                    style,
                    border_box_height,
                    line_baseline_offset,
                );
                Some(
                    InlineAtom::new(
                        InlineAtomContent::InlineBox { sequence },
                        style.clone(),
                        None,
                        InlineSize::new(
                            content_width
                                + horizontal_extras
                                + style.margin.left
                                + style.margin.right,
                            border_box_height + style.margin.top + style.margin.bottom,
                        ),
                        baseline_offset,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_outside_marker(outside_marker)
                    .with_visual_offset(visual_offset),
                )
            }
            None => None,
        }
    }
}
