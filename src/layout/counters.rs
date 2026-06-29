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
    ) -> Vec<page_generated::PageMarginContentItem> {
        let mut output = Vec::new();
        for part in &set.parts {
            match part {
                NamedStringPart::String(text) => {
                    push_named_string_text_part(&mut output, text);
                }
                NamedStringPart::ContentText => {
                    push_named_string_text_part(&mut output, &inline_text(element));
                }
                NamedStringPart::BeforeContent => {
                    let items = self.evaluate_generated_pseudo_items_rollback(
                        element,
                        style.before_style.as_deref(),
                    );
                    push_named_string_items(&mut output, items);
                }
                NamedStringPart::AfterContent => {
                    let items = self.evaluate_generated_pseudo_items_rollback(
                        element,
                        style.after_style.as_deref(),
                    );
                    push_named_string_items(&mut output, items);
                }
                NamedStringPart::Attr(name) => {
                    if let Some(value) = element.attrs.get(name) {
                        push_named_string_text_part(&mut output, value);
                    }
                }
                NamedStringPart::Image(url) => output.push(
                    page_generated::PageMarginContentItem::Inline(GeneratedContentPart::Image {
                        url: url.clone(),
                        base_url: self.base_url.map(Path::to_path_buf),
                        root_url: self.root_url.map(Path::to_path_buf),
                    }),
                ),
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
                        push_named_string_text_part(&mut output, &counter);
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
                    push_named_string_text_part(&mut output, &counters.join(separator));
                }
            }
        }
        output
    }

    /// Evaluates generated pseudo content into page-margin items for `string-set`.
    ///
    /// CSS GCPM allows named strings to include generated content through
    /// `content(before)` and `content(after)`. Capturing typed items here keeps
    /// supported generated images and quote/leader tokens available for later
    /// page-margin layout while rolling back pseudo counter side effects:
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>.
    fn evaluate_generated_pseudo_items_rollback(
        &mut self,
        element: &Element,
        pseudo_style: Option<&ComputedStyle>,
    ) -> Vec<page_generated::PageMarginContentItem> {
        let Some(pseudo_style) = pseudo_style else {
            return Vec::new();
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return Vec::new();
        };
        let snapshot = self.counter_set.clone();
        let scope = self.begin_pseudo_counter_scope(pseudo_style);
        let mut output = Vec::new();
        for part in content {
            match part {
                GeneratedContentPart::Text(text) => push_named_string_text_part(&mut output, text),
                GeneratedContentPart::Contents => {
                    push_named_string_text_part(&mut output, &inline_text(element));
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
                    push_named_string_text_part(&mut output, &text);
                }
                GeneratedContentPart::Quote(_)
                | GeneratedContentPart::Leader(_)
                | GeneratedContentPart::Image { .. } => {
                    output.push(page_generated::PageMarginContentItem::Inline(part.clone()))
                }
            }
        }
        self.end_counter_scope(scope);
        self.counter_set = snapshot;
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

    pub(super) fn capture_named_strings(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Vec<AssignmentId> {
        let mut ids = Vec::new();
        for set in &style.string_sets {
            // CSS GCPM named strings capture generated text at element layout
            // time so page-margin `string()` can use the value for the page.
            // https://www.w3.org/TR/css-gcpm-3/#setting-named-strings
            let value = self.evaluate_named_string_set_with_counter_scopes(element, style, set);
            let placement = self.assignment_placement_for_current_page(style);
            let id = self.next_assignment_id();
            self.current_page_named_strings
                .entry(set.name.clone())
                .or_default()
                .push(NamedStringAssignment {
                    id,
                    value: PageAssignmentValue::GeneratedContent(value),
                    placement,
                });
            ids.push(id);
            self.record_captured_assignment_id(id);
        }
        ids
    }

    pub(super) fn capture_running_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        let Some(name) = &style.running_element_name else {
            return false;
        };
        let value = self.running_element_text_with_counter_scopes(element, style);
        let content_parts =
            running_element_content_parts(element, &value, self.base_url, self.root_url);
        let placement = self.running_element_source_marker_placement();
        let id = self.next_assignment_id();
        self.current_page_running_elements
            .entry(name.clone())
            .or_default()
            .push(NamedStringAssignment {
                id,
                value: PageAssignmentValue::RunningElement(Box::new(RunningElementCapture {
                    fallback_text: value,
                    content_parts,
                    element: element.clone(),
                    style: Box::new(style.clone()),
                    counter_set: self.counter_set.clone(),
                    quote_depth: self.quote_depth,
                })),
                placement,
            });
        self.record_captured_assignment_id(id);
        true
    }

    /// Starts collecting GCPM assignment ids emitted while a layout fragment is produced.
    ///
    /// Fragmented formatting contexts can move source boxes after assignment
    /// capture. The final emitted fragment then becomes the source of truth for
    /// `string(..., start)` and similar page-boundary lookups:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
    pub(super) fn begin_assignment_capture_frame(&mut self) {
        self.assignment_capture_stack.push(Vec::new());
    }

    pub(super) fn end_assignment_capture_frame(&mut self) -> Vec<AssignmentId> {
        self.assignment_capture_stack.pop().unwrap_or_default()
    }

    fn record_captured_assignment_id(&mut self, id: AssignmentId) {
        if let Some(frame) = self.assignment_capture_stack.last_mut() {
            frame.push(id);
        }
    }

    fn next_assignment_id(&mut self) -> AssignmentId {
        let id = AssignmentId(self.next_assignment_id);
        self.next_assignment_id += 1;
        id
    }

    /// Records the source-page position for GCPM named-string and running-element assignments.
    ///
    /// GCPM `string(..., start)` and `element(..., start)` are defined in terms
    /// of generated source fragments at the page boundary:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings> and
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>. The current
    /// capture point is before full child layout for normal-flow boxes, so this
    /// stores the fragment start estimate available at assignment time while
    /// preserving the exact page-start marker used by existing behavior.
    fn assignment_placement_for_current_page(&self, style: &ComputedStyle) -> AssignmentPlacement {
        let height = style.line_height.max(0.0);
        AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(
                PageTopRect::new(
                    self.content_left,
                    self.cursor_y,
                    self.content_right - self.content_left,
                    height,
                )
                .paint_clip(),
            ),
        }
    }

    /// Records the final source marker for `position: running()` assignments.
    ///
    /// Running elements are removed from normal flow, so there is no later
    /// source border box to observe. GCPM still resolves `element(..., start)`
    /// from the assignment's source position, so the post-break cursor is the
    /// durable zero-size marker for the removed source:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    fn running_element_source_marker_placement(&self) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(PaintClip::from_paint_rect(paint_space_rect(
                self.content_left,
                self.cursor_y,
                0.0,
                0.0,
            ))),
        }
    }

    /// Updates named-string assignments once the source box has produced its final first fragment.
    ///
    /// GCPM `string(..., start)` depends on whether the source assignment's
    /// generated fragment starts the page, so capture-time estimates are
    /// replaced after layout with the first page that actually received source
    /// paint:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
    pub(super) fn update_named_assignment_placements(
        &mut self,
        ids: &[AssignmentId],
        placement: AssignmentPlacement,
    ) {
        update_assignment_placements_for_maps(
            ids,
            placement,
            &mut self.page_named_strings,
            &mut self.current_page_named_strings,
            self.pages.len(),
        );
    }

    pub(super) fn update_running_assignment_placements(
        &mut self,
        ids: &[AssignmentId],
        placement: AssignmentPlacement,
    ) {
        update_assignment_placements_for_maps(
            ids,
            placement,
            &mut self.page_running_elements,
            &mut self.current_page_running_elements,
            self.pages.len(),
        );
    }
}

fn update_assignment_placements_for_maps(
    ids: &[AssignmentId],
    placement: AssignmentPlacement,
    pages: &mut [HashMap<String, Vec<NamedStringAssignment>>],
    current: &mut HashMap<String, Vec<NamedStringAssignment>>,
    current_page_index: usize,
) {
    for id in ids {
        let Some((name, mut assignment)) = take_assignment_by_id(current, *id)
            .or_else(|| take_assignment_by_id_from_pages(pages, *id))
        else {
            continue;
        };
        assignment.placement = placement;
        insert_assignment_for_page(
            pages,
            current,
            placement.page_index,
            name,
            assignment,
            current_page_index,
        );
    }
}

fn take_assignment_by_id_from_pages(
    pages: &mut [HashMap<String, Vec<NamedStringAssignment>>],
    id: AssignmentId,
) -> Option<(String, NamedStringAssignment)> {
    for page in pages {
        if let Some(assignment) = take_assignment_by_id(page, id) {
            return Some(assignment);
        }
    }
    None
}

fn take_assignment_by_id(
    assignments: &mut HashMap<String, Vec<NamedStringAssignment>>,
    id: AssignmentId,
) -> Option<(String, NamedStringAssignment)> {
    let name = assignments.iter().find_map(|(name, values)| {
        values
            .iter()
            .any(|assignment| assignment.id == id)
            .then(|| name.clone())
    })?;
    let values = assignments.get_mut(&name)?;
    let index = values.iter().position(|assignment| assignment.id == id)?;
    let assignment = values.remove(index);
    if values.is_empty() {
        assignments.remove(&name);
    }
    Some((name, assignment))
}

fn insert_assignment_for_page(
    pages: &mut [HashMap<String, Vec<NamedStringAssignment>>],
    current: &mut HashMap<String, Vec<NamedStringAssignment>>,
    page_index: usize,
    name: String,
    assignment: NamedStringAssignment,
    current_page_index: usize,
) {
    if page_index < current_page_index {
        if let Some(page) = pages.get_mut(page_index) {
            page.entry(name).or_default().push(assignment);
        }
    } else {
        current.entry(name).or_default().push(assignment);
    }
}

fn push_named_string_text_part(
    output: &mut Vec<page_generated::PageMarginContentItem>,
    value: &str,
) {
    if value.is_empty() {
        return;
    }
    match output.last_mut() {
        Some(page_generated::PageMarginContentItem::Inline(GeneratedContentPart::Text(
            previous,
        ))) => previous.push_str(value),
        _ => output.push(page_generated::PageMarginContentItem::Inline(
            GeneratedContentPart::Text(value.to_string()),
        )),
    }
}

fn push_named_string_items(
    output: &mut Vec<page_generated::PageMarginContentItem>,
    items: Vec<page_generated::PageMarginContentItem>,
) {
    for item in items {
        match item {
            page_generated::PageMarginContentItem::Inline(GeneratedContentPart::Text(text)) => {
                push_named_string_text_part(output, &text)
            }
            _ => output.push(item),
        }
    }
}

/// Captures the generated-content replay form for `position: running()`.
///
/// CSS GCPM defines `element()` as replaying a running element in generated
/// content: <https://www.w3.org/TR/css-gcpm-3/#running-elements>. This keeps
/// the capture typed so replaced images can flow through the normal generated
/// content image path while richer box-fragment replay is added.
fn running_element_content_parts(
    element: &Element,
    fallback_text: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Vec<GeneratedContentPart> {
    if element.tag.eq_ignore_ascii_case("img")
        && let Some(url) = element.attrs.get("src").filter(|value| !value.is_empty())
    {
        return vec![GeneratedContentPart::Image {
            url: url.clone(),
            base_url: base_url.map(Path::to_path_buf),
            root_url: root_url.map(Path::to_path_buf),
        }];
    }
    if fallback_text.is_empty() {
        Vec::new()
    } else {
        vec![GeneratedContentPart::Text(fallback_text.to_string())]
    }
}
