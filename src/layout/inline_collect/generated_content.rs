use super::*;

impl<'a> LayoutBuilder<'a> {
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
        let (source, footnote_call) = if originating_style
            .before_style
            .as_deref()
            .is_some_and(|before| std::ptr::eq(before, pseudo_style))
        {
            (box_tree::CounterEventSource::Before, None)
        } else if originating_style
            .footnote_call_style
            .as_deref()
            .is_some_and(|call| std::ptr::eq(call, pseudo_style))
        {
            (box_tree::CounterEventSource::FootnoteCall, Some(element.id))
        } else if originating_style
            .scroll_marker_style
            .as_deref()
            .is_some_and(|marker| std::ptr::eq(marker, pseudo_style))
        {
            // Automatic markers inherit the target element's counter scope;
            // their external group is only a formatting parent.
            (box_tree::CounterEventSource::Principal, None)
        } else {
            (box_tree::CounterEventSource::After, None)
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
        if let Some(element) = footnote_call {
            for item in &mut output[start_len..] {
                if let InlineItem::Word(word) = item {
                    word.source = InlineTextSource::FootnoteCall(element);
                }
            }
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
                let source = if element.tag.eq_ignore_ascii_case("wbr") {
                    InlineTextSource::GeneratedWbr
                } else {
                    InlineTextSource::Generated
                };
                push_generated_inline_words_for_style_with_source(
                    text,
                    style,
                    link_target,
                    baseline_shift,
                    visual_offset,
                    source,
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
                    counter_styles::CounterStyleRenderContext::for_style(style),
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
            GeneratedContentPart::TargetCounter {
                target,
                name,
                style: counter_style,
            } => {
                self.has_normal_flow_target_references = true;
                if let Some(text) = self.resolve_generated_target_counter(
                    element,
                    target,
                    name,
                    counter_style.clone(),
                ) {
                    push_generated_inline_words_for_style(
                        &text,
                        style,
                        link_target,
                        baseline_shift,
                        visual_offset,
                        output,
                    );
                }
            }
            GeneratedContentPart::TargetText { target, keyword } => {
                self.has_normal_flow_target_references = true;
                if let Some(text) = self.resolve_generated_target_text(element, target, *keyword) {
                    push_generated_inline_words_for_style(
                        &text,
                        style,
                        link_target,
                        baseline_shift,
                        visual_offset,
                        output,
                    );
                }
            }
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
                        true,
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
        preserve_intrinsic_image_size: bool,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Option<InlineAtom> {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
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
        let content = image.into_inline_atom_content();
        let mut atom_style = style.clone();
        // `used_generated_image_value` resolves the pseudo's own used
        // border-box size. The atom retains that footprint, while
        // `object-fit: none` below selects the intrinsic payload size within
        // it.
        let atom_border_box_width = border_box_width;
        let atom_border_box_height = border_box_height;
        if preserve_intrinsic_image_size
            && matches!(
                content,
                InlineAtomContent::Image(_) | InlineAtomContent::Svg { .. }
            )
        {
            // Browser interoperability treats an image supplied as the sole
            // content of ::before/::after as anonymous inline content inside
            // the pseudo box. Its payload keeps its intrinsic dimensions;
            // the pseudo's width and height still establish the decorated
            // outer box.
            // <https://www.w3.org/TR/css-content-3/#content-property>
            atom_style = atom_style.map_used_values(|style| {
                style.object_fit = css::ObjectFit::None;
                // The anonymous image is laid out at the pseudo-element's
                // content start, rather than using the replaced-element
                // default centered `object-position`.
                style.object_position = css::BackgroundPosition::INITIAL;
            });
        }
        Some(
            InlineAtom::new(
                content,
                atom_style,
                None,
                InlineSize::new(
                    atom_border_box_width + style.margin.left + style.margin.right,
                    atom_border_box_height + style.margin.top + style.margin.bottom,
                ),
                atom_border_box_height,
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
}

pub(in crate::layout) fn quote_pair(style: &ComputedStyle, depth: usize) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(default_quote_pair),
        Quotes::Auto(_) => {
            let (open, close) = style.quotes.auto_quote_pair(depth);
            (open.to_string(), close.to_string())
        }
    }
}

pub(in crate::layout) fn default_quote_pair() -> (String, String) {
    ("“".to_string(), "”".to_string())
}

pub(super) fn generated_pseudo_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut content_style = style.clone();
    content_style.margin = css::Edges::ZERO;
    content_style.ua_margin_em = css::OptionalEdges::NONE;
    content_style.box_values.margin =
        css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
    content_style.padding = css::Edges::ZERO;
    content_style.box_values.padding = css::PhysicalEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_width = 0.0;
    content_style.border_widths = css::Edges::ZERO;
    content_style.border_width_values =
        css::PhysicalEdges::all(css::ComputedLengthPercentage::ZERO);
    content_style.border_styles = css::BorderStyles::NONE;
    content_style.border_radius = css::BorderRadius::ZERO;
    content_style.corner_shapes = css::CornerShapes::ROUND;
    content_style.border_image = css::BorderImage::initial();
    content_style
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
            InlineItem::Word(word) if word.source.is_generated() => {
                std::rc::Rc::make_mut(&mut word.style).clear = clear;
            }
            _ => {}
        }
    }
}

pub(super) fn generated_content_originating_clear(
    source: &box_tree::BoxSource<'_>,
) -> Option<Clear> {
    match source {
        box_tree::BoxSource::GeneratedPseudo(pseudo) => Some(pseudo.originating_clear),
        box_tree::BoxSource::Principal => None,
    }
}

/// Keep a generated float's tree-abiding pseudo identity through inline
/// collection so its replay uses the matching counter scope.
/// <https://www.w3.org/TR/css-pseudo-4/#generated-content>
pub(super) fn generated_pseudo_counter_source(
    source: &box_tree::BoxSource<'_>,
) -> Option<box_tree::CounterEventSource> {
    match source {
        box_tree::BoxSource::GeneratedPseudo(pseudo) => Some(pseudo.kind.counter_event_source()),
        box_tree::BoxSource::Principal => None,
    }
}

pub(super) fn inline_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
}
