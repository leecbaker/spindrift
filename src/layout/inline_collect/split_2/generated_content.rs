use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn push_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_items(
                        element,
                        style,
                        stylesheets,
                        inherited_link.clone(),
                        placement,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                placement.baseline_shift,
                placement.visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        propagated_decoration,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                visual_offset,
                alt_text.clone(),
                output,
            );
        }
    }

    pub(in crate::layout) fn collect_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for child in &element.children {
            match &child.kind {
                NodeKind::Text(text) => {
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        placement.baseline_shift,
                        placement.visual_offset,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    let child_signature = ElementSignature::with_sibling_list(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(style),
                    );
                    if child_style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            child_element.clone(),
                            child_signature,
                            child_style,
                            inline_style_establishes_positioning_containing_block(style)
                                .then_some(style.clone()),
                        ))));
                        continue;
                    }
                    if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                        self.layout_positioned_inline_descendant(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            None,
                            style,
                            output,
                        );
                        continue;
                    }
                    child_style.text_decoration = child_style
                        .text_decoration
                        .with_propagated_lines(style.text_decoration);
                    if child_style.display.is_none()
                        || child_style.display.is_block_level()
                        || child_style.display.is_table()
                    {
                        continue;
                    }
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement = placement
                        .with_added_baseline_shift(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &child_style,
                                style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&child_style),
                        );
                    let scope = self.begin_inline_element_scope(
                        child_element,
                        &child_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::DOM_PAINT,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.before_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    self.collect_element_content_or_inline_items(
                        child_element,
                        &child_style,
                        stylesheets,
                        link.clone(),
                        child_placement,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        &child_style,
                        child_style.after_style.as_deref(),
                        link.clone(),
                        child_placement.baseline_shift,
                        child_placement.visual_offset,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    self.end_inline_element_scope(scope, &child_style, output);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn collect_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        self.collect_inline_box_items_with_float_containing_block(
            children,
            stylesheets,
            inherited_link,
            baseline_shift,
            visual_offset,
            block_style,
            propagated_decoration,
            None,
            output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_inline_box_items_with_float_containing_block(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        active_float_containing_block_style: Option<&ComputedStyle>,
        output: &mut Vec<InlineItem>,
    ) {
        for (child_index, child) in children.iter().enumerate() {
            if let Some((element, _, style, child_boxes)) = child.element_parts()
                && matches!(style.position, Position::Absolute | Position::Fixed)
            {
                let table_fragment = match child {
                    box_tree::FormattingBox::AtomicInline(box_) => box_.table_fragment.as_ref(),
                    box_tree::FormattingBox::Table(box_) => Some(&box_.fragment),
                    _ => None,
                };
                self.layout_positioned_inline_descendant(
                    element,
                    style,
                    stylesheets,
                    Some(child_boxes),
                    table_fragment,
                    block_style,
                    output,
                );
                continue;
            }
            if let Some((element, signature, style, _)) = child.element_parts()
                && style.float != Float::None
            {
                output.push(InlineItem::Float(Box::new(InlineFloat::new(
                    element.clone(),
                    signature.clone(),
                    style.clone(),
                    active_float_containing_block_style.cloned(),
                ))));
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                // CSS Pseudo-Elements tree-abiding generated content can
                // generate block-level boxes. Even empty block pseudos create
                // block boundaries in an inline formatting context, such as
                // `dt::before { content: ""; display: block }`.
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                self.collect_inline_box_items_with_float_containing_block(
                    &box_.children,
                    stylesheets,
                    inherited_link.clone(),
                    baseline_shift,
                    visual_offset,
                    block_style,
                    box_.style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration),
                    active_float_containing_block_style,
                    output,
                );
                if formatting_box_has_inline_content(&box_.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration);
                    let text = if child_index + 1 == children.len() {
                        trim_terminal_preserved_segment_breaks(&box_.text, &text_style)
                    } else {
                        box_.text.as_str()
                    };
                    self.push_inline_words(
                        text,
                        &text_style,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    if box_.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.element.clone(),
                            box_.signature.clone(),
                            (*box_.style).clone(),
                            active_float_containing_block_style.cloned(),
                        ))));
                        continue;
                    }
                    let mut inline_style = box_tree::owned_style(&box_.style);
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration);
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement = InlinePlacement::new(baseline_shift, visual_offset)
                        .with_added_baseline_shift(
                            self.vertical_align_baseline_shift_for_inline_style(
                                &inline_style,
                                block_style,
                            ),
                        )
                        .with_added_visual_offset(
                            self.inline_visual_offset_for_style(&inline_style),
                        );
                    let scope = self.begin_inline_element_scope(
                        box_.element,
                        &inline_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::BOX_PAINT
                            .with_fragment_edges(box_.fragment_edges),
                        output,
                    );
                    let next_float_containing_block_style =
                        if inline_style_establishes_positioning_containing_block(&inline_style) {
                            Some(&inline_style)
                        } else {
                            active_float_containing_block_style
                        };
                    if inline_style.content.is_generated() {
                        let start_len = output.len();
                        self.push_element_content_items_from_boxes(
                            box_.element,
                            &inline_style,
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            block_style,
                            inline_style.text_decoration,
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.element,
                            clear,
                            output,
                            start_len,
                        );
                    } else {
                        self.collect_inline_box_items_with_float_containing_block(
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            block_style,
                            inline_style.text_decoration,
                            next_float_containing_block_style,
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    if box_.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat::new(
                            box_.element.clone(),
                            box_.signature.clone(),
                            (*box_.style).clone(),
                            active_float_containing_block_style.cloned(),
                        ))));
                        continue;
                    }
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset =
                        visual_offset.plus(self.inline_visual_offset_for_style(&box_.style));
                    if let Some(mut atom) = self.inline_atom_for_element(
                        box_.element,
                        &box_.signature,
                        &box_.style,
                        &box_.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        baseline_shift,
                        atom_visual_offset,
                        link.clone(),
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.element, &box_.style);
                        self.push_inline_words(
                            &text,
                            &box_.style,
                            link,
                            baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_inline_box_items_with_float_containing_block(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        visual_offset,
                        block_style,
                        box_.style
                            .text_decoration
                            .with_propagated_lines(propagated_decoration),
                        active_float_containing_block_style,
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_positioned_inline_descendant(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) {
        let source_was_inline_level =
            style.abspos_static_source_was_inline_level || style.display.is_inline_level();
        if source_was_inline_level {
            let mut positioned_style = style.clone();
            positioned_style.abspos_static_source_was_inline_level = true;
            positioned_style.abspos_static_source_was_atomic_inline =
                style.abspos_static_source_was_atomic_inline || style.display.is_atomic_inline();
            let static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                output,
            );
            self.layout_positioned_block_with_inline_static_position(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_position,
            );
            return;
        }

        let static_y_offset = self.block_static_position_y_offset_from_buffer(output, block_style);
        self.layout_positioned_block_with_block_static_y_offset(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
            static_y_offset,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_from_hypothetical_placeholder(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) -> InlineStaticPosition {
        let placeholder = self.inline_static_position_placeholder_atom(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
        );
        let mut hypothetical_items = Vec::with_capacity(output.len() + 1);
        hypothetical_items.extend_from_slice(output);
        hypothetical_items.push(InlineItem::Atom(Box::new(placeholder)));
        let available_width = self.current_content_logical_inline_size().max(1.0);
        // CSS Positioned Layout defines the static-position rectangle as the
        // box's hypothetical normal-flow position. Carrying a non-painting
        // placeholder through ordinary inline line selection keeps forced
        // breaks, wrapping, and line metrics aligned with the same CSS Text
        // machinery used for real inline content:
        // https://www.w3.org/TR/css-position-3/#staticpos-rect
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.inline_static_position_from_placeholder_sequence(&sequence, block_style)
            .unwrap_or_else(|| InlineStaticPosition {
                start_x: self.content_left,
                end_x: self.content_right,
                top_y: self.cursor_y,
                baseline_y: self.inline_static_baseline_y_from_buffer(output, style),
                use_margin_box_top: false,
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_static_position_placeholder_atom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> InlineAtom {
        let available_width = (self.content_right - self.content_left).max(style.font_size);
        let mut placeholder_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(&mut placeholder_style, available_width);
        let horizontal_non_content = placeholder_style.padding.left
            + placeholder_style.padding.right
            + horizontal_border_width(&placeholder_style);
        let positioned_available_outer_width =
            (available_width - placeholder_style.margin.left - placeholder_style.margin.right)
                .max(placeholder_style.font_size);
        let content_width = self.used_intrinsic_or_shrink_to_fit_width(
            element,
            &placeholder_style,
            stylesheets,
            positioned_available_outer_width,
            horizontal_non_content,
            child_boxes,
            table_fragment,
        );
        let border_box_width = content_width + horizontal_non_content;
        let vertical_non_content = placeholder_style.padding.top
            + placeholder_style.padding.bottom
            + vertical_border_width(&placeholder_style);
        let containing_block_height = self.definite_block_size_stack.last().copied().flatten();
        let content_height = used_content_height_or_auto_with_optional_basis(
            &placeholder_style,
            containing_block_height,
            vertical_non_content,
        )
        .map(|height| constrain_height(&placeholder_style, height, available_width))
        .unwrap_or(placeholder_style.line_height);
        let border_box_height = content_height + vertical_non_content;
        let line_baseline_offset = if placeholder_style.display.is_atomic_inline()
            || placeholder_style.abspos_static_source_was_atomic_inline
        {
            Self::inline_block_baseline_offset(&placeholder_style, border_box_height, None)
        } else {
            self.font_system
                .rendered_first_line_baseline_offset(&placeholder_style)
        };

        InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            placeholder_style.clone(),
            None,
            border_box_width + placeholder_style.margin.left + placeholder_style.margin.right,
            border_box_height + placeholder_style.margin.top + placeholder_style.margin.bottom,
            line_baseline_offset,
            0.0,
            None,
            None,
        )
    }

    pub(in crate::layout) fn inline_static_position_from_placeholder_sequence(
        &mut self,
        sequence: &inline_layout::InlineLineSequence,
        block_style: &ComputedStyle,
    ) -> Option<InlineStaticPosition> {
        let saved_cursor_y = self.cursor_y;
        let context = sequence.context(block_style);
        let mut plaintext_direction_state = None;
        let mut line_top = self.cursor_y;
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        for record in &records {
            if let Some(fragment) = &record.fragment && fragment.items().iter().any(|item| {
                matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                )
            }) {
                let paint_line_top = line_top + record.block_start_trim;
                self.cursor_y = paint_line_top;
                let position = self
                    .prepare_inline_line_record(record, context, &mut plaintext_direction_state)
                    .and_then(|prepared| {
                        prepared.paint_items.iter().find_map(|item| {
                            let PreparedInlinePaintItem::Atom(atom) = item else {
                                return None;
                            };
                            matches!(
                                atom.atom.content(),
                                InlineAtomContent::StaticPositionPlaceholder
                            )
                            .then_some(InlineStaticPosition {
                                start_x: atom.content_rect.x(),
                                end_x: atom.content_rect.x() + atom.content_rect.width(),
                                top_y: atom.content_rect.y()
                                    + atom.content_rect.height()
                                    + atom.atom.style().margin.top,
                                baseline_y: paint_line_top - prepared.metrics.baseline_offset,
                                use_margin_box_top: atom.atom.style().display.is_atomic_inline()
                                    || atom.atom.style().abspos_static_source_was_atomic_inline,
                            })
                        })
                    });
                self.cursor_y = saved_cursor_y;
                return position;
            }
            line_top -= record.height();
        }
        self.cursor_y = saved_cursor_y;
        None
    }

    pub(in crate::layout) fn collect_intrinsic_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        context: IntrinsicInlineCollectionContext<'_>,
        output: &mut Vec<InlineItem>,
    ) {
        for (child_index, child) in children.iter().enumerate() {
            if let Some((_, _, style, _)) = child.element_parts()
                && (matches!(style.position, Position::Absolute | Position::Fixed)
                    || style.float != Float::None)
            {
                continue;
            }
            if let box_tree::FormattingBox::Block(box_) = child
                && matches!(&box_.source, box_tree::BoxSource::GeneratedPseudo(_))
            {
                if output
                    .last()
                    .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                self.collect_intrinsic_inline_box_items(
                    &box_.children,
                    stylesheets,
                    inherited_link.clone(),
                    context
                        .with_block_style(&box_.style)
                        .with_propagated_decoration(
                            box_.style
                                .text_decoration
                                .with_propagated_lines(context.propagated_decoration),
                        ),
                    output,
                );
                if formatting_box_has_inline_content(&box_.children)
                    && output
                        .last()
                        .is_some_and(|item| !matches!(item, InlineItem::Break(_)))
                {
                    trim_trailing_inline_spaces(output);
                    output.push(InlineItem::Break(InlineBreak::default()));
                }
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_tree::owned_style(&box_.style);
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration);
                    let text = if child_index + 1 == children.len() {
                        trim_terminal_preserved_segment_breaks(&box_.text, &text_style)
                    } else {
                        box_.text.as_str()
                    };
                    self.push_inline_words(
                        text,
                        &text_style,
                        inherited_link.clone(),
                        context.baseline_shift,
                        context.visual_offset,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let mut inline_style = box_tree::owned_style(&box_.style);
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration);
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_placement =
                        InlinePlacement::new(context.baseline_shift, context.visual_offset)
                            .with_added_baseline_shift(
                                self.vertical_align_baseline_shift_for_inline_style(
                                    &inline_style,
                                    context.block_style,
                                ),
                            )
                            .with_added_visual_offset(
                                self.inline_visual_offset_for_style(&inline_style),
                            );
                    let scope = self.begin_inline_element_scope(
                        box_.element,
                        &inline_style,
                        link.clone(),
                        child_placement,
                        InlineElementScopeOptions::BOX_INTRINSIC
                            .with_fragment_edges(box_.fragment_edges),
                        output,
                    );
                    if inline_style.content.is_generated() {
                        let start_len = output.len();
                        self.push_intrinsic_element_content_items_from_boxes(
                            box_.element,
                            &inline_style,
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_placement.baseline_shift,
                            child_placement.visual_offset,
                            inline_style.text_decoration,
                            output,
                        );
                        let clear = generated_content_originating_clear(&box_.source)
                            .unwrap_or(inline_style.clear);
                        annotate_line_break_element_breaks_with_clear(
                            box_.element,
                            clear,
                            output,
                            start_len,
                        );
                    } else {
                        self.collect_intrinsic_inline_box_items(
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            context
                                .with_baseline_shift(child_placement.baseline_shift)
                                .with_visual_offset(child_placement.visual_offset)
                                .with_block_style(&inline_style)
                                .with_propagated_decoration(inline_style.text_decoration),
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let atom_visual_offset = context
                        .visual_offset
                        .plus(self.inline_visual_offset_for_style(&box_.style));
                    if let Some(mut atom) = self.intrinsic_inline_atom_for_element(
                        box_.element,
                        &box_.style,
                        &box_.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        context.baseline_shift,
                        atom_visual_offset,
                        link,
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, context.block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.element, &box_.style);
                        self.push_inline_words(
                            &text,
                            &box_.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            atom_visual_offset,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_intrinsic_inline_box_items(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        context
                            .with_block_style(&box_.style)
                            .with_propagated_decoration(
                                box_.style
                                    .text_decoration
                                    .with_propagated_lines(context.propagated_decoration),
                            ),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
            }
        }
    }
}

pub(in crate::layout) fn annotate_line_break_element_breaks(
    element: &Element,
    style: &ComputedStyle,
    output: &mut [InlineItem],
    start_len: usize,
) {
    annotate_line_break_element_breaks_with_clear(element, style.clear, output, start_len);
}

pub(in crate::layout) fn annotate_line_break_element_breaks_with_clear(
    element: &Element,
    clear: Clear,
    output: &mut [InlineItem],
    start_len: usize,
) {
    if !is_line_break_element(element) || clear == Clear::None {
        return;
    }
    for item in output.iter_mut().skip(start_len) {
        match item {
            InlineItem::Break(break_) => break_.clear = clear,
            InlineItem::Word(word) if word.source == InlineTextSource::Generated => {
                std::rc::Rc::make_mut(&mut word.style).clear = clear;
            }
            _ => {}
        }
    }
}

fn generated_content_originating_clear(source: &box_tree::BoxSource<'_>) -> Option<Clear> {
    match source {
        box_tree::BoxSource::GeneratedPseudo(pseudo) => Some(pseudo.originating_clear),
        box_tree::BoxSource::Principal => None,
    }
}

fn inline_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || !style.transform.is_empty()
}
