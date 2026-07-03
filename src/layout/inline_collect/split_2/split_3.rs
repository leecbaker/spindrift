use super::generated_content::annotate_line_break_element_breaks;
use super::*;

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
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        if let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        {
            let image = used_generated_image_value(
                image,
                style,
                available_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )?;
            let border_box_width = image.border_box_size.width;
            let border_box_height = image.border_box_size.height;
            return Some(
                InlineAtom::new(
                    InlineAtomContent::Image(image.decoded),
                    style.clone(),
                    None,
                    border_box_width + style.margin.left + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
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
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let (width, height) = used_canvas_size_with_height_basis(
                    element,
                    style,
                    available_width,
                    containing_block_height,
                );
                (
                    width + style.margin.left + style.margin.right,
                    height + style.margin.top + style.margin.bottom,
                    height,
                )
            }
            Some(ReplacedElementKind::Image) => used_image(
                element,
                style,
                available_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                (
                    border_box_width + style.margin.left + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
                    border_box_height,
                )
            })?,
            Some(ReplacedElementKind::Svg) => {
                let (width, height, _) = svg_rect(element)?;
                (
                    width + style.margin.left + style.margin.right,
                    height + style.margin.top + style.margin.bottom,
                    height,
                )
            }
            None if style.display.is_table() => {
                let fragment = table_fragment?;
                let horizontal_extras =
                    style.padding.left + style.padding.right + horizontal_border_width(style);
                let (min_width, width) = self.table_parent_intrinsic_content_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                );
                let content_width = intrinsic::shrink_to_fit_width(
                    min_width,
                    width,
                    (available_width - horizontal_extras).max(0.0),
                );
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_flex() && style.display.is_inline_level() => {
                let box_metrics = used_box_metrics(style, available_width);
                let horizontal_extras = box_metrics.horizontal_non_content();
                let (min_width, width) = self.estimate_flex_intrinsic_widths(
                    element,
                    style,
                    stylesheets,
                    available_width,
                    Some(children),
                );
                let content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    min_width,
                    width,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
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
                let box_metrics = used_box_metrics(style, available_width);
                let horizontal_extras = box_metrics.horizontal_non_content();
                let vertical_extras = box_metrics.vertical_non_content();
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let definite_content_height = used_content_height_or_auto_with_optional_basis(
                    style,
                    containing_block_height,
                    vertical_extras,
                )
                .map(|height| constrain_height(style, height, available_width));
                self.definite_block_size_stack.push(definite_content_height);
                let contribution = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    let contribution = self
                        .intrinsic_inline_measurement_for_text(&text, style, available_width)
                        .contribution;
                    inline_layout::InlineIntrinsicContribution {
                        min_content: contribution.min_content,
                        max_content: contribution.max_content,
                    }
                } else {
                    self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                    )
                };
                self.definite_block_size_stack.pop();
                let content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    contribution.min_content,
                    contribution.max_content,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                let content_width = if style.box_values.width.is_auto() {
                    content_width.max(style.font_size)
                } else {
                    content_width
                };
                let measured_content_height = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    self.intrinsic_inline_measurement_for_text(&text, style, content_width)
                        .height()
                        .max(style.line_height)
                } else {
                    self.definite_block_size_stack.push(definite_content_height);
                    self.intrinsic_inline_measurement_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                        content_width,
                    )
                    .height()
                    .max(style.line_height)
                };
                if !children.is_empty() {
                    self.definite_block_size_stack.pop();
                }
                let content_height = definite_content_height.unwrap_or_else(|| {
                    constrain_height(style, measured_content_height, available_width)
                });
                let border_box_height = content_height + vertical_extras;
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
                    border_box_height,
                )
            }
            None => return None,
        };
        Some(
            InlineAtom::new(
                InlineAtomContent::Svg {
                    fill: Color::TRANSPARENT,
                },
                style.clone(),
                None,
                width,
                height,
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
                .rendered_first_line_baseline_offset(atom.style());
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
                !atom.content().is_inline_edge() || atom.width > 0.0 || atom.height > 0.0
            }
            InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => false,
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
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
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
        InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            block_style.clone(),
            None,
            0.0,
            block_style.line_height,
            self.font_system
                .rendered_first_line_baseline_offset(block_style),
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
        let counter_scope = self.begin_pseudo_counter_scope(pseudo_style);
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
                let text = evaluate_generated_content_text(
                    element,
                    std::slice::from_ref(part),
                    self.counter_set.stacks(),
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
                        0.0,
                        style.line_height,
                        style.font_size,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )));
            }
            GeneratedContentPart::Image { image } => {
                if let Some(atom) = self.generated_image_atom_for_image(
                    image,
                    style,
                    baseline_shift,
                    visual_offset,
                    link_target,
                    alt_text,
                ) {
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
        apply_used_box_metrics(&mut used_style, available_width);
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
        Some(
            InlineAtom::new(
                InlineAtomContent::Image(image.decoded),
                style.clone(),
                None,
                border_box_width + style.margin.left + style.margin.right,
                border_box_height + style.margin.top + style.margin.bottom,
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
                self.counter_set.stacks(),
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
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if !text.is_empty() {
            output.push(InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: inline_style(style),
                baseline_shift: placement.baseline_shift,
                visual_offset: placement.visual_offset,
                link_target,
                mergeable: true,
                source,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
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
            return self.generated_image_atom_for_image(
                image,
                style,
                baseline_shift,
                visual_offset,
                link_target,
                alt_text,
            );
        }
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let style = self.style_with_current_viewport_lengths(style);
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let (width, height) = used_canvas_size_with_height_basis(
                    element,
                    &style,
                    available_width,
                    containing_block_height,
                );
                let atom_width = width + style.margin.left + style.margin.right;
                Some(
                    InlineAtom::new(
                        InlineAtomContent::Canvas,
                        style,
                        None,
                        atom_width,
                        height,
                        height,
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
                apply_used_box_metrics(&mut used_style, available_width);
                let style = &used_style;
                let image = used_image(
                    element,
                    style,
                    available_width,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?;
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                Some(
                    InlineAtom::new(
                        InlineAtomContent::Image(image.decoded),
                        style.clone(),
                        None,
                        border_box_width + style.margin.left + style.margin.right,
                        border_box_height + style.margin.top + style.margin.bottom,
                        border_box_height,
                        baseline_shift,
                        link_target,
                        element.attrs.get("alt").cloned(),
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            Some(ReplacedElementKind::Svg) => {
                let (width, height, fill) = svg_rect(element)?;
                Some(
                    InlineAtom::new(
                        InlineAtomContent::Svg { fill },
                        style.clone(),
                        None,
                        width + style.margin.left + style.margin.right,
                        height,
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
                    .max(style.font_size);
                let mut used_style = self.style_with_current_viewport_lengths(style);
                let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
                let style = &used_style;
                let border_widths = box_metrics.border;
                let horizontal_extras = box_metrics.horizontal_non_content();
                let vertical_extras = box_metrics.vertical_non_content();
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let definite_content_height = used_content_height_or_auto_with_optional_basis(
                    style,
                    containing_block_height,
                    vertical_extras,
                )
                .map(|height| constrain_height(style, height, available_width));
                self.definite_block_size_stack.push(definite_content_height);
                let intrinsic = self.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    Some(children),
                    available_width,
                );
                let requested_content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    intrinsic.contribution.min_content,
                    intrinsic.contribution.max_content,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                let requested_content_width = if style.box_values.width.is_auto() {
                    requested_content_width.max(style.font_size)
                } else {
                    requested_content_width
                };
                let content_width =
                    constrain_width(style, requested_content_width, available_width)
                        .max(style.font_size);
                let mut sequence_items = Vec::new();
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
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        style,
                        style.text_decoration,
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
                        style.text_decoration,
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
                let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                    sequence_items,
                    style,
                    content_width,
                    0.0,
                    0.0,
                );
                let measured_content_height = sequence.total_height().max(style.line_height);
                // CSS Sizing applies `height` to the content box; line-height
                // can overflow explicit-height inline-blocks but must not
                // increase their used height:
                // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
                let content_height = definite_content_height.unwrap_or_else(|| {
                    constrain_height(style, measured_content_height, available_width)
                });
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
                        content_width + horizontal_extras + style.margin.left + style.margin.right,
                        border_box_height + style.margin.top + style.margin.bottom,
                        baseline_offset,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            None => None,
        }
    }
}
