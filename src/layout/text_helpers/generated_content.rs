use super::*;

pub(in crate::layout) fn evaluate_bookmark_label(
    element: &Element,
    style: &ComputedStyle,
) -> String {
    let mut output = String::new();
    for part in &style.bookmark_label.parts {
        match part {
            BookmarkLabelPart::String(text) => output.push_str(text),
            BookmarkLabelPart::ContentText => output.push_str(&inline_text(element)),
            BookmarkLabelPart::Attr(name) => {
                if let Some(value) = element.unprefixed_css_attr(name) {
                    output.push_str(value);
                }
            }
        }
    }
    output
}

impl<'a> LayoutBuilder<'a> {
    /// Resolves a same-document target reference against the preceding fresh
    /// layout pass. The text is inserted before inline line selection, so it
    /// participates in ordinary wrapping and pagination rather than paint
    /// replay. CSS Generated Content Level 3 defines target values from the
    /// target end of a link: <https://www.w3.org/TR/css-content-3/#cross-references>.
    pub(in crate::layout) fn resolve_generated_target_counter(
        &self,
        origin: &Element,
        target: &TargetReference,
        name: &str,
        style: Option<ListStyleType>,
    ) -> Option<String> {
        let anchor = self.target_anchor_for_reference(origin, target)?;
        let value = if name.eq_ignore_ascii_case("page") {
            i32::try_from(anchor.page_index.saturating_add(1)).unwrap_or(i32::MAX)
        } else if name.eq_ignore_ascii_case("pages") {
            i32::try_from(self.target_references.total_pages).unwrap_or(i32::MAX)
        } else {
            anchor.counters.get(name)?.last().copied()?
        };
        counter_styles::counter_text(
            style.unwrap_or(ListStyleType::Decimal),
            value,
            &self.counter_styles,
        )
    }

    pub(in crate::layout) fn resolve_generated_target_text(
        &self,
        origin: &Element,
        target: &TargetReference,
        keyword: css::NamedStringTargetTextKeyword,
    ) -> Option<String> {
        let anchor = self.target_anchor_for_reference(origin, target)?;
        Some(match keyword {
            css::NamedStringTargetTextKeyword::Content => anchor.text.content,
            css::NamedStringTargetTextKeyword::Before => anchor.text.before,
            css::NamedStringTargetTextKeyword::After => anchor.text.after,
            css::NamedStringTargetTextKeyword::FirstLetter => anchor
                .text
                .content
                .chars()
                .next()
                .map(|character| character.to_string())
                .unwrap_or_default(),
        })
    }

    fn target_anchor_for_reference(
        &self,
        origin: &Element,
        target: &TargetReference,
    ) -> Option<TargetAnchor> {
        let target = match target {
            TargetReference::Fragment(_) => target.literal_fragment_id()?.to_string(),
            TargetReference::Attribute(name) => origin
                .unprefixed_css_attr(name)
                .and_then(|value| value.strip_prefix('#'))
                .filter(|value| !value.is_empty())?
                .to_string(),
        };
        self.target_references
            .anchors
            .get(&target)
            .cloned()
            .or_else(|| {
                Some(TargetAnchor {
                    page_index: *self.page_anchors.get(&target)?,
                    text: self.page_anchor_text.get(&target)?.clone(),
                    counters: self.page_anchor_counters.get(&target)?.clone(),
                })
            })
    }
}

pub(in crate::layout) fn evaluate_generated_content_text(
    element: &Element,
    content: &[GeneratedContentPart],
    counter_stack: &HashMap<String, Vec<i32>>,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: counter_styles::CounterStyleRenderContext,
) -> String {
    let mut output = String::new();
    for part in content {
        match part {
            GeneratedContentPart::Text(text) => output.push_str(text),
            GeneratedContentPart::Contents => output.push_str(&inline_text(element)),
            GeneratedContentPart::Attr { name, fallback } => {
                if let Some(value) = element.unprefixed_css_attr(name) {
                    output.push_str(value);
                } else if let Some(fallback) = fallback {
                    output.push_str(fallback);
                }
            }
            GeneratedContentPart::Counter {
                name,
                style: counter_style,
            } => {
                let value = counter_stack
                    .get(name)
                    .and_then(|values| values.last().cloned())
                    .unwrap_or(0);
                if let Some(counter) = counter_styles::counter_text_with_context(
                    counter_style.clone().unwrap_or(ListStyleType::Decimal),
                    value,
                    counter_styles,
                    render_context,
                ) {
                    output.push_str(&counter);
                }
            }
            GeneratedContentPart::Counters {
                name,
                separator,
                style: counter_style,
            } => {
                let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                let counters = counter_stack
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| vec![0])
                    .into_iter()
                    .filter_map(|value| {
                        counter_styles::counter_text_with_context(
                            style.clone(),
                            value,
                            counter_styles,
                            render_context,
                        )
                    })
                    .collect::<Vec<_>>();
                output.push_str(&counters.join(separator));
            }
            GeneratedContentPart::Image { .. } => {}
            GeneratedContentPart::TargetCounter { .. } => {}
            GeneratedContentPart::TargetText { .. } => {}
            GeneratedContentPart::Quote(_) => {}
            GeneratedContentPart::Leader(text) => output.push_str(text),
        }
    }
    output
}

pub(in crate::layout) fn evaluate_generated_alt_text(
    element: &Element,
    content: &[GeneratedAltTextPart],
    counter_stack: &HashMap<String, Vec<i32>>,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> String {
    let mut output = String::new();
    for part in content {
        match part {
            GeneratedAltTextPart::Text(text) => output.push_str(text),
            GeneratedAltTextPart::Attr { name, fallback } => {
                if let Some(value) = element.unprefixed_css_attr(name) {
                    output.push_str(value);
                } else if let Some(fallback) = fallback {
                    output.push_str(fallback);
                }
            }
            GeneratedAltTextPart::Counter {
                name,
                style: counter_style,
            } => {
                let value = counter_stack
                    .get(name)
                    .and_then(|values| values.last().cloned())
                    .unwrap_or(0);
                if let Some(counter) = counter_styles::counter_text(
                    counter_style.clone().unwrap_or(ListStyleType::Decimal),
                    value,
                    counter_styles,
                ) {
                    output.push_str(&counter);
                }
            }
            GeneratedAltTextPart::Counters {
                name,
                separator,
                style: counter_style,
            } => {
                let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                let counters = counter_stack
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| vec![0])
                    .into_iter()
                    .filter_map(|value| {
                        counter_styles::counter_text(style.clone(), value, counter_styles)
                    })
                    .collect::<Vec<_>>();
                output.push_str(&counters.join(separator));
            }
        }
    }
    output
}
