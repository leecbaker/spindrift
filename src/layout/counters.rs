use super::list::{list_container_counter_reset, list_item_value};
use super::*;

impl CounterSet {
    pub(super) fn new() -> Self {
        Self {
            values: HashMap::new(),
            frames: vec![CounterFrame {
                base_lengths: HashMap::new(),
            }],
        }
    }

    pub(super) fn stacks(&self) -> &HashMap<String, Vec<i32>> {
        &self.values
    }

    pub(super) fn current(&self, name: &str) -> Option<i32> {
        self.values
            .get(name)
            .and_then(|values| values.last().copied())
    }

    fn push_frame(&mut self) {
        self.frames.push(CounterFrame {
            base_lengths: HashMap::new(),
        });
    }

    fn pop_frame(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        for (name, base_len) in frame.base_lengths {
            if let Some(values) = self.values.get_mut(&name) {
                values.truncate(base_len);
                if values.is_empty() {
                    self.values.remove(&name);
                }
            }
        }
        if self.frames.is_empty() {
            self.frames.push(CounterFrame {
                base_lengths: HashMap::new(),
            });
        }
    }

    /// Applies `counter-reset` by instantiating a counter in the current scope.
    ///
    /// CSS Lists 3 scopes counters created by `counter-reset` to the element's
    /// descendants and following siblings, replacing earlier counters of the
    /// same name from the same parent scope:
    /// <https://www.w3.org/TR/css-lists-3/#inheriting-counters>.
    fn reset_counter(&mut self, name: &str, value: i32) {
        let base_len = self.current_frame_base_len(name);
        let values = self.values.entry(name.to_string()).or_default();
        values.truncate(base_len);
        values.push(value);
    }

    fn reset_counter_for_element(&mut self, name: &str, value: i32) -> bool {
        if self.counter_should_be_temporary_for_element(name) {
            self.values.entry(name.to_string()).or_default().push(value);
            true
        } else {
            self.reset_counter(name, value);
            false
        }
    }

    fn pop_temporary_counter(&mut self, name: &str) {
        if let Some(values) = self.values.get_mut(name) {
            values.pop();
            if values.is_empty() {
                self.values.remove(name);
            }
        }
    }

    /// Applies `counter-increment` to the innermost counter, instantiating a
    /// missing counter at `0` first as defined by CSS Lists 3:
    /// <https://www.w3.org/TR/css-lists-3/#propdef-counter-increment>.
    fn increment_counter(&mut self, name: &str, amount: i32) {
        self.ensure_counter(name);
        if let Some(value) = self
            .values
            .get_mut(name)
            .and_then(|values| values.last_mut())
        {
            *value += amount;
        }
    }

    /// Applies `counter-set` to the innermost counter, instantiating a missing
    /// counter at `0` first as defined by CSS Lists 3:
    /// <https://www.w3.org/TR/css-lists-3/#propdef-counter-set>.
    fn set_counter(&mut self, name: &str, value: i32) {
        self.ensure_counter(name);
        if let Some(current) = self
            .values
            .get_mut(name)
            .and_then(|values| values.last_mut())
        {
            *current = value;
        }
    }

    fn ensure_counter(&mut self, name: &str) {
        if self
            .values
            .get(name)
            .is_some_and(|values| !values.is_empty())
        {
            return;
        }
        let base_len = self.current_frame_base_len(name);
        let values = self.values.entry(name.to_string()).or_default();
        values.truncate(base_len);
        values.push(0);
    }

    fn current_frame_base_len(&mut self, name: &str) -> usize {
        let current_len = self.values.get(name).map_or(0, Vec::len);
        self.frames
            .last_mut()
            .expect("counter set always has a root frame")
            .base_lengths
            .entry(name.to_string())
            .or_insert(current_len)
            .to_owned()
    }

    fn counter_should_be_temporary_for_element(&self, name: &str) -> bool {
        let Some(values) = self.values.get(name) else {
            return false;
        };
        let Some(frame) = self.frames.last() else {
            return false;
        };
        let base_len = frame
            .base_lengths
            .get(name)
            .copied()
            .unwrap_or(values.len());
        values.len() == base_len && base_len > 0
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn begin_counter_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> CounterScopeState {
        let temporary_counters = self.apply_counter_effects(element, style);
        self.counter_set.push_frame();
        CounterScopeState { temporary_counters }
    }

    pub(super) fn end_counter_scope(&mut self, state: CounterScopeState) {
        self.counter_set.pop_frame();
        for name in state.temporary_counters.into_iter().rev() {
            self.counter_set.pop_temporary_counter(&name);
        }
    }

    pub(super) fn begin_pseudo_counter_scope(
        &mut self,
        style: &ComputedStyle,
    ) -> CounterScopeState {
        let temporary_counters = self.apply_pseudo_counter_effects(style);
        self.counter_set.push_frame();
        CounterScopeState { temporary_counters }
    }

    fn apply_counter_effects(&mut self, element: &Element, style: &ComputedStyle) -> Vec<String> {
        let mut temporary_counters = Vec::new();
        let resets = self.effective_counter_resets(element, style);
        for (name, value) in resets {
            if self.counter_set.reset_counter_for_element(&name, value) {
                temporary_counters.push(name);
            }
        }

        let increments = self.effective_counter_increments(element, style);
        if let Some(value) = list_item_value(element)
            && style.display.is_list_item()
        {
            let increment = increments
                .iter()
                .filter(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
                .map(|(_, value)| *value)
                .next()
                .unwrap_or_else(|| self.list_stack.last().map_or(1, |list| list.step));
            self.counter_set
                .set_counter(LIST_ITEM_COUNTER_NAME, value - increment);
        }
        for (name, amount) in increments {
            self.counter_set.increment_counter(&name, amount);
        }

        for (name, value) in &style.counter_sets {
            self.counter_set.set_counter(name, *value);
        }
        temporary_counters
    }

    fn apply_pseudo_counter_effects(&mut self, style: &ComputedStyle) -> Vec<String> {
        let mut temporary_counters = Vec::new();
        for (name, value) in &style.counter_resets {
            if self.counter_set.reset_counter_for_element(name, *value) {
                temporary_counters.push(name.clone());
            }
        }

        let increments = self.effective_pseudo_counter_increments(style);
        for (name, amount) in increments {
            self.counter_set.increment_counter(&name, amount);
        }

        for (name, value) in &style.counter_sets {
            self.counter_set.set_counter(name, *value);
        }
        temporary_counters
    }

    fn effective_counter_resets(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Vec<(String, i32)> {
        let mut resets = style.counter_resets.clone();
        let Some((value, should_override_zero)) = list_container_counter_reset(element) else {
            return resets;
        };
        match resets
            .iter_mut()
            .find(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
        {
            Some((_, existing)) if should_override_zero && *existing == 0 => *existing = value,
            Some(_) => {}
            None => resets.push((LIST_ITEM_COUNTER_NAME.to_string(), value)),
        }
        resets
    }

    fn effective_counter_increments(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Vec<(String, i32)> {
        let mut increments = style.counter_increments.clone();
        if style.display.is_list_item()
            && !increments
                .iter()
                .any(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
        {
            let amount = self.list_stack.last().map_or(1, |list| list.step);
            increments.push((LIST_ITEM_COUNTER_NAME.to_string(), amount));
        }
        if list_item_value(element).is_some()
            && !increments
                .iter()
                .any(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
        {
            let amount = self.list_stack.last().map_or(1, |list| list.step);
            increments.push((LIST_ITEM_COUNTER_NAME.to_string(), amount));
        }
        increments
    }

    fn effective_pseudo_counter_increments(&self, style: &ComputedStyle) -> Vec<(String, i32)> {
        let mut increments = style.counter_increments.clone();
        if style.display.is_list_item()
            && !increments
                .iter()
                .any(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
        {
            increments.push((LIST_ITEM_COUNTER_NAME.to_string(), 1));
        }
        increments
    }

    pub(super) fn evaluate_generated_pseudo_text_rollback(
        &mut self,
        element: &Element,
        pseudo_style: Option<&ComputedStyle>,
    ) -> String {
        let Some(pseudo_style) = pseudo_style else {
            return String::new();
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return String::new();
        };

        let snapshot = self.counter_set.clone();
        let scope = self.begin_pseudo_counter_scope(pseudo_style);
        let text = evaluate_generated_content_text(
            element,
            content,
            self.counter_set.stacks(),
            &self.counter_styles,
        );
        self.end_counter_scope(scope);
        self.counter_set = snapshot;
        text
    }

    fn evaluate_named_string_set_with_counter_scopes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        set: &crate::css::NamedStringSet,
    ) -> String {
        let mut output = String::new();
        for part in &set.parts {
            match part {
                NamedStringPart::String(text) => output.push_str(text),
                NamedStringPart::ContentText => output.push_str(&inline_text(element)),
                NamedStringPart::BeforeContent => {
                    output.push_str(&self.evaluate_generated_pseudo_text_rollback(
                        element,
                        style.before_style.as_deref(),
                    ))
                }
                NamedStringPart::AfterContent => {
                    output.push_str(&self.evaluate_generated_pseudo_text_rollback(
                        element,
                        style.after_style.as_deref(),
                    ))
                }
                NamedStringPart::Attr(name) => {
                    if let Some(value) = element.attrs.get(name) {
                        output.push_str(value);
                    }
                }
                NamedStringPart::Counter {
                    name,
                    style: counter_style,
                } => {
                    let value = self
                        .counter_set
                        .stacks()
                        .get(name)
                        .and_then(|values| values.last().copied())
                        .unwrap_or(0);
                    if let Some(counter) = list::counter_text(
                        counter_style.clone().unwrap_or(ListStyleType::Decimal),
                        value,
                        &self.counter_styles,
                    ) {
                        output.push_str(&counter);
                    }
                }
                NamedStringPart::Counters {
                    name,
                    separator,
                    style: counter_style,
                } => {
                    let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                    let counters = self
                        .counter_set
                        .stacks()
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| vec![0])
                        .into_iter()
                        .filter_map(|value| {
                            list::counter_text(style.clone(), value, &self.counter_styles)
                        })
                        .collect::<Vec<_>>();
                    output.push_str(&counters.join(separator));
                }
            }
        }
        output
    }

    fn running_element_text_with_counter_scopes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> String {
        let mut output =
            self.evaluate_generated_pseudo_text_rollback(element, style.before_style.as_deref());
        output.push_str(&inline_text(element));
        output.push_str(
            &self.evaluate_generated_pseudo_text_rollback(element, style.after_style.as_deref()),
        );
        output
    }

    pub(super) fn capture_named_strings(&mut self, element: &Element, style: &ComputedStyle) {
        for set in &style.string_sets {
            // CSS GCPM named strings capture generated text at element layout
            // time so page-margin `string()` can use the value for the page.
            // https://www.w3.org/TR/css-gcpm-3/#setting-named-strings
            let at_page_start = !self.current_page_has_content();
            let value = self.evaluate_named_string_set_with_counter_scopes(element, style, set);
            self.current_page_named_strings
                .entry(set.name.clone())
                .or_default()
                .push(NamedStringAssignment {
                    value,
                    at_page_start,
                });
        }
    }

    pub(super) fn capture_running_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        let Some(name) = &style.running_element_name else {
            return false;
        };
        let at_page_start = !self.current_page_has_content();
        let value = self.running_element_text_with_counter_scopes(element, style);
        self.current_page_running_elements
            .entry(name.clone())
            .or_default()
            .push(NamedStringAssignment {
                value,
                at_page_start,
            });
        true
    }
}
