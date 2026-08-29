use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CounterSet {
    pub(in crate::layout) values: HashMap<String, Vec<CounterInstance>>,
    pub(in crate::layout) frames: Vec<CounterFrame>,
    pub(in crate::layout) next_scope_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct CounterInstance {
    pub(in crate::layout) value: CounterValue,
    pub(in crate::layout) reversed: bool,
    /// The element or tree-abiding pseudo-element that instantiated this
    /// counter, when the counter set originates from source-order planning.
    ///
    /// CSS Lists defines counter identity in terms of both its name and
    /// originating element. See <https://drafts.csswg.org/css-lists-3/#creating-counters>.
    pub(in crate::layout) creator: Option<CounterOriginKey>,
    /// The traversal scope that owns this counter instance for style
    /// containment. This is deliberately distinct from `creator`: traversal
    /// scopes are not CSS counter identity.
    pub(in crate::layout) creator_scope: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct CounterOriginKey {
    pub(in crate::layout) element_id: crate::dom::ElementId,
    pub(in crate::layout) source: box_tree::CounterEventSource,
}

impl CounterOriginKey {
    pub(in crate::layout) fn new(element: &Element, source: box_tree::CounterEventSource) -> Self {
        Self {
            element_id: element.id,
            source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct CounterResetKey {
    pub(in crate::layout) origin: CounterOriginKey,
    pub(in crate::layout) declaration_index: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::layout) struct CounterPlan {
    pub(in crate::layout) reversed_initial_values: HashMap<CounterResetKey, CounterValue>,
    pub(in crate::layout) values_at_origin: HashMap<CounterOriginKey, HashMap<String, Vec<i32>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct CounterFrame {
    pub(in crate::layout) base_lengths: HashMap<String, usize>,
    pub(in crate::layout) scope_id: usize,
    /// The innermost style-containment boundary visible to this scope.
    ///
    /// `counter-increment` and `counter-set` must not mutate instances
    /// created outside this scope. See CSS Containment 2 § 3.3:
    /// <https://drafts.csswg.org/css-contain-2/#containment-style>.
    pub(in crate::layout) counter_mutation_floor: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct CounterScopeState {
    /// Generated pseudo-content is evaluated against the durable source-order
    /// counter snapshot and restores the caller's transient layout state when
    /// complete.
    pub(in crate::layout) previous_counter_set: Option<CounterSet>,
    /// Style containment also scopes generated-quote nesting.  The enclosing
    /// quote depth remains observable inside the scope, but quote operations
    /// performed there must not affect following siblings outside it.
    /// <https://drafts.csswg.org/css-contain-2/#containment-style>
    pub(in crate::layout) previous_quote_depth: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GeneratedPseudoCounterMode {
    Commit,
    Rollback,
}

#[derive(Debug, Clone, Copy)]
struct PlannedCounterInstance {
    id: usize,
    reversed: bool,
    creator: CounterOriginKey,
    creator_scope: usize,
}

/// Counter membership at a source-tree position, before numeric values are
/// materialized.  CSS Lists counter inheritance takes membership from an
/// element's parent or preceding sibling, but values from the preceding
/// element in tree order.  Keeping this separate from traversal frames avoids
/// treating an element's subtree as the scope of a counter it creates.
/// <https://drafts.csswg.org/css-lists-3/#inheriting-counters>
#[derive(Debug, Clone, Default)]
struct PlannedCounterState {
    values: HashMap<String, Vec<PlannedCounterInstance>>,
}

impl PlannedCounterState {
    fn inherit(membership_source: &Self, value_source: &Self) -> Self {
        let values = value_source
            .values
            .iter()
            .filter_map(|(name, value_instances)| {
                let membership_instances = membership_source.values.get(name)?;
                let inherited = value_instances
                    .iter()
                    .copied()
                    .filter(|value| {
                        membership_instances
                            .iter()
                            .any(|membership| membership.id == value.id)
                    })
                    .collect::<Vec<_>>();
                (!inherited.is_empty()).then_some((name.clone(), inherited))
            })
            .collect();
        Self { values }
    }

    fn current(&self, name: &str, mutation_floor: usize) -> Option<PlannedCounterInstance> {
        self.values
            .get(name)
            .and_then(|values| values.last())
            .filter(|instance| instance.creator_scope >= mutation_floor)
            .copied()
    }

    fn reset(&mut self, name: &str, instance: PlannedCounterInstance, current_scope: usize) {
        let values = self.values.entry(name.to_string()).or_default();
        if values.last().is_some_and(|previous| {
            previous.creator == instance.creator || previous.creator_scope == current_scope
        }) {
            values.pop();
        }
        values.push(instance);
    }
}

#[derive(Debug, Default)]
struct ReversedAccumulator {
    key: Option<CounterResetKey>,
    value: CounterValue,
    last_nonzero_increment_negated: CounterValue,
    stopped_by_set: bool,
}

/// Counter scope used exclusively while determining an omitted `reversed()`
/// start value.
///
/// CSS counter values continue to use [`PlannedCounterState`]: a counter
/// created by a nested list can be inherited by following siblings.  The
/// initial-value algorithm has a different question, though: which source
/// mutations belong to the reset declaration's own scope.  In particular, a
/// nested reset temporarily masks its ancestor while its subtree is visited,
/// then the ancestor resumes collecting contributions for following siblings.
///
/// <https://drafts.csswg.org/css-lists-3/#instantiating-counters>
#[derive(Debug, Default)]
struct ReversedStartScopeState {
    values: HashMap<String, Vec<PlannedCounterInstance>>,
    frames: Vec<ReversedStartScopeFrame>,
}

#[derive(Debug, Default)]
struct ReversedStartScopeFrame {
    base_lengths: HashMap<String, usize>,
}

impl ReversedStartScopeState {
    fn new() -> Self {
        Self {
            values: HashMap::new(),
            frames: vec![ReversedStartScopeFrame::default()],
        }
    }

    fn current(&self, name: &str) -> Option<PlannedCounterInstance> {
        self.values
            .get(name)
            .and_then(|values| values.last())
            .copied()
    }

    /// Makes a reset visible while its descendants are visited.
    ///
    /// A reset at the start of an ancestor's child scope is temporary: it
    /// owns that subtree's reversed-start contributions, but does not own
    /// contributions from following siblings in the enclosing scope.  A
    /// reset that appears after another sibling counter instead shadows that
    /// counter for the rest of the sibling scope.
    fn reset_for_node(&mut self, name: &str, instance: PlannedCounterInstance) -> bool {
        let base_length = self.current_frame_base_length(name);
        let values = self.values.entry(name.to_string()).or_default();
        let temporary = values.len() == base_length && base_length > 0;
        if !temporary {
            values.truncate(base_length);
        }
        values.push(instance);
        temporary
    }

    fn ensure(&mut self, name: &str, instance: PlannedCounterInstance) -> PlannedCounterInstance {
        self.current(name).unwrap_or_else(|| {
            self.reset_for_node(name, instance);
            instance
        })
    }

    fn push_frame(&mut self) {
        self.frames.push(ReversedStartScopeFrame::default());
    }

    fn pop_frame(&mut self) {
        let frame = self
            .frames
            .pop()
            .expect("reversed-start scope never pops its root frame");
        for (name, base_length) in frame.base_lengths {
            if let Some(values) = self.values.get_mut(&name) {
                values.truncate(base_length);
                if values.is_empty() {
                    self.values.remove(&name);
                }
            }
        }
    }

    fn pop_temporary(&mut self, name: &str) {
        if let Some(values) = self.values.get_mut(name) {
            values.pop();
            if values.is_empty() {
                self.values.remove(name);
            }
        }
    }

    fn current_frame_base_length(&mut self, name: &str) -> usize {
        let current_length = self.values.get(name).map_or(0, Vec::len);
        *self
            .frames
            .last_mut()
            .expect("reversed-start scope always has a root frame")
            .base_lengths
            .entry(name.to_string())
            .or_insert(current_length)
    }
}

#[derive(Debug)]
struct CounterPlanBuilder {
    next_scope_id: usize,
    accumulators: Vec<ReversedAccumulator>,
}

impl CounterPlanBuilder {
    fn new() -> Self {
        Self {
            next_scope_id: 1,
            accumulators: Vec::new(),
        }
    }

    fn build(events: &[box_tree::CounterEventNode<'_>]) -> CounterPlan {
        let mut builder = Self::new();
        let mut reversed_start_scopes = ReversedStartScopeState::new();
        builder.visit_siblings(
            events,
            &PlannedCounterState::default(),
            0,
            0,
            &mut reversed_start_scopes,
        );
        let reversed_initial_values = builder
            .accumulators
            .into_iter()
            .filter_map(|accumulator| {
                accumulator.key.map(|key| {
                    (
                        key,
                        accumulator
                            .value
                            .add(accumulator.last_nonzero_increment_negated),
                    )
                })
            })
            .collect();
        let values_at_origin = CounterSnapshotPlanner::build(events, &reversed_initial_values);
        CounterPlan {
            reversed_initial_values,
            values_at_origin,
        }
    }

    fn visit_siblings(
        &mut self,
        events: &[box_tree::CounterEventNode<'_>],
        parent_state: &PlannedCounterState,
        current_scope: usize,
        mutation_floor: usize,
        reversed_start_scopes: &mut ReversedStartScopeState,
    ) -> PlannedCounterState {
        let mut preceding_sibling = None;
        let mut preceding_tree_order = parent_state.clone();
        for event in events {
            let membership_source = preceding_sibling.as_ref().unwrap_or(parent_state);
            let state = PlannedCounterState::inherit(membership_source, &preceding_tree_order);
            let (node_state, last_tree_order) = self.visit(
                event,
                state,
                current_scope,
                mutation_floor,
                reversed_start_scopes,
            );
            preceding_sibling = Some(node_state);
            preceding_tree_order = last_tree_order;
        }
        preceding_tree_order
    }

    fn visit(
        &mut self,
        event: &box_tree::CounterEventNode<'_>,
        mut state: PlannedCounterState,
        current_scope: usize,
        mutation_floor: usize,
        reversed_start_scopes: &mut ReversedStartScopeState,
    ) -> (PlannedCounterState, PlannedCounterState) {
        let origin = CounterOriginKey::new(event.element, event.source);
        let mut temporary_reversed_start_resets = Vec::new();
        for (declaration_index, reset) in event.counter_style.counter_resets.iter().enumerate() {
            let accumulator_id = self.accumulators.len();
            self.accumulators.push(ReversedAccumulator {
                key: matches!(reset.kind, CounterResetKind::Reversed(None)).then_some(
                    CounterResetKey {
                        origin,
                        declaration_index,
                    },
                ),
                ..ReversedAccumulator::default()
            });
            let instance = PlannedCounterInstance {
                id: accumulator_id,
                reversed: reset.kind.is_reversed(),
                creator: origin,
                creator_scope: current_scope,
            };
            state.reset(&reset.name, instance, current_scope);
            if reversed_start_scopes.reset_for_node(&reset.name, instance) {
                temporary_reversed_start_resets.push(reset.name.clone());
            }
        }

        let mut increments = Vec::<(String, CounterValue)>::new();
        for change in &event.counter_style.counter_increments {
            if let Some((_, value)) = increments.iter_mut().find(|(name, _)| name == &change.name) {
                *value = value.add(change.value);
            } else {
                increments.push((change.name.clone(), change.value));
            }
        }
        if event.counter_style.is_list_item
            && !increments
                .iter()
                .any(|(name, _)| name == LIST_ITEM_COUNTER_NAME)
        {
            let amount = if state
                .current(LIST_ITEM_COUNTER_NAME, mutation_floor)
                .is_some_and(|instance| instance.reversed)
            {
                -1
            } else {
                1
            };
            increments.push((
                LIST_ITEM_COUNTER_NAME.to_string(),
                CounterValue::new(amount),
            ));
        }

        let mut observed_names = increments
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        for change in &event.counter_style.counter_sets {
            if !observed_names.contains(&change.name) {
                observed_names.push(change.name.clone());
            }
        }
        for name in observed_names {
            let increment = increments
                .iter()
                .find(|(candidate, _)| candidate == &name)
                .map_or(CounterValue::ZERO, |(_, value)| *value);
            let set = event
                .counter_style
                .counter_sets
                .iter()
                .find(|change| change.name == name)
                .map(|change| change.value);
            let had_visible_instance = state.current(&name, mutation_floor).is_some();
            let instance = self.ensure(&mut state, &name, origin, current_scope, mutation_floor);
            let reversed_start_instance = if !had_visible_instance && mutation_floor != 0 {
                // Style containment makes the enclosing counter invisible to
                // this subtree. Its first local mutation must therefore own a
                // separate reversed-start accumulator, while ordinary nested
                // reversed resets retain the shared-scope behavior below.
                // <https://drafts.csswg.org/css-contain-2/#containment-style>
                reversed_start_scopes.reset_for_node(&name, instance);
                instance
            } else {
                reversed_start_scopes.ensure(&name, instance)
            };
            self.observe(reversed_start_instance.id, increment, set);
        }

        let node_state = state.clone();
        let child_scope = self.next_scope_id;
        self.next_scope_id = self.next_scope_id.saturating_add(1);
        let child_mutation_floor = if event.counter_style.style_containment {
            child_scope
        } else {
            mutation_floor
        };
        reversed_start_scopes.push_frame();
        let last_tree_order = self.visit_siblings(
            &event.children,
            &node_state,
            child_scope,
            child_mutation_floor,
            reversed_start_scopes,
        );
        reversed_start_scopes.pop_frame();
        for name in temporary_reversed_start_resets.into_iter().rev() {
            reversed_start_scopes.pop_temporary(&name);
        }
        (node_state, last_tree_order)
    }

    fn observe(&mut self, id: usize, increment: CounterValue, set: Option<CounterValue>) {
        let Some(accumulator) = self.accumulators.get_mut(id) else {
            return;
        };
        if accumulator.key.is_none() || accumulator.stopped_by_set {
            return;
        }
        let increment_negated = increment.negated();
        if !increment_negated.is_zero() {
            accumulator.last_nonzero_increment_negated = increment_negated;
        }
        if let Some(set) = set {
            accumulator.value = accumulator.value.add(set);
            accumulator.stopped_by_set = true;
        } else {
            accumulator.value = accumulator.value.add(increment_negated);
        }
    }

    fn ensure(
        &mut self,
        state: &mut PlannedCounterState,
        name: &str,
        creator: CounterOriginKey,
        current_scope: usize,
        mutation_floor: usize,
    ) -> PlannedCounterInstance {
        if let Some(instance) = state.current(name, mutation_floor) {
            return instance;
        }
        let id = self.accumulators.len();
        self.accumulators.push(ReversedAccumulator::default());
        let instance = PlannedCounterInstance {
            id,
            reversed: false,
            creator,
            creator_scope: current_scope,
        };
        state.reset(name, instance, current_scope);
        instance
    }
}

struct CounterSnapshotPlanner<'a> {
    reversed_initial_values: &'a HashMap<CounterResetKey, CounterValue>,
    values_at_origin: HashMap<CounterOriginKey, HashMap<String, Vec<i32>>>,
    next_scope_id: usize,
}

impl<'a> CounterSnapshotPlanner<'a> {
    fn build(
        events: &[box_tree::CounterEventNode<'_>],
        reversed_initial_values: &'a HashMap<CounterResetKey, CounterValue>,
    ) -> HashMap<CounterOriginKey, HashMap<String, Vec<i32>>> {
        let mut planner = Self {
            reversed_initial_values,
            values_at_origin: HashMap::new(),
            next_scope_id: 1,
        };
        planner.visit_siblings(events, &CounterSet::new(), 0, 0);
        planner.values_at_origin
    }

    fn inherit(
        membership_source: &CounterSet,
        value_source: &CounterSet,
        current_scope: usize,
        mutation_floor: usize,
    ) -> CounterSet {
        let values = value_source
            .values
            .iter()
            .filter_map(|(name, value_instances)| {
                let membership_instances = membership_source.values.get(name)?;
                let inherited = value_instances
                    .iter()
                    .filter(|value| {
                        membership_instances
                            .iter()
                            .any(|membership| membership.creator == value.creator)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                (!inherited.is_empty()).then_some((name.clone(), inherited))
            })
            .collect();
        CounterSet {
            values,
            frames: vec![CounterFrame {
                base_lengths: HashMap::new(),
                scope_id: current_scope,
                counter_mutation_floor: mutation_floor,
            }],
            next_scope_id: current_scope.saturating_add(1),
        }
    }

    fn visit_siblings(
        &mut self,
        events: &[box_tree::CounterEventNode<'_>],
        parent_state: &CounterSet,
        current_scope: usize,
        mutation_floor: usize,
    ) -> CounterSet {
        let mut preceding_sibling = None;
        let mut preceding_tree_order = parent_state.clone();
        for event in events {
            let membership_source = preceding_sibling.as_ref().unwrap_or(parent_state);
            let state = Self::inherit(
                membership_source,
                &preceding_tree_order,
                current_scope,
                mutation_floor,
            );
            let (node_state, last_tree_order) =
                self.visit(event, state, current_scope, mutation_floor);
            preceding_sibling = Some(node_state);
            preceding_tree_order = last_tree_order;
        }
        preceding_tree_order
    }

    fn visit(
        &mut self,
        event: &box_tree::CounterEventNode<'_>,
        mut counters: CounterSet,
        _current_scope: usize,
        mutation_floor: usize,
    ) -> (CounterSet, CounterSet) {
        let origin = CounterOriginKey::new(event.element, event.source);
        for (declaration_index, reset) in event.counter_style.counter_resets.iter().enumerate() {
            let value = reset.kind.explicit_value().unwrap_or_else(|| {
                self.reversed_initial_values
                    .get(&CounterResetKey {
                        origin,
                        declaration_index,
                    })
                    .cloned()
                    .unwrap_or(CounterValue::ZERO)
            });
            counters.reset_counter(reset, value, origin);
        }

        let mut increments = Vec::<(String, CounterValue)>::new();
        for change in &event.counter_style.counter_increments {
            if let Some((_, value)) = increments.iter_mut().find(|(name, _)| name == &change.name) {
                *value = value.add(change.value);
            } else {
                increments.push((change.name.clone(), change.value));
            }
        }
        if event.counter_style.is_list_item
            && !increments
                .iter()
                .any(|(name, _)| name == LIST_ITEM_COUNTER_NAME)
        {
            let amount = if counters.current_is_reversed(LIST_ITEM_COUNTER_NAME) {
                -1
            } else {
                1
            };
            increments.push((
                LIST_ITEM_COUNTER_NAME.to_string(),
                CounterValue::new(amount),
            ));
        }
        for (name, amount) in increments {
            counters.increment_counter(&name, amount, origin);
        }
        for change in &event.counter_style.counter_sets {
            counters.set_counter(&change.name, change.value, origin);
        }

        self.values_at_origin.insert(origin, counters.stacks());
        let node_state = counters.clone();
        let child_scope = self.next_scope_id;
        self.next_scope_id = self.next_scope_id.saturating_add(1);
        let child_mutation_floor = if event.counter_style.style_containment {
            child_scope
        } else {
            mutation_floor
        };
        let last_tree_order = self.visit_siblings(
            &event.children,
            &node_state,
            child_scope,
            child_mutation_floor,
        );
        (node_state, last_tree_order)
    }
}

impl CounterSet {
    pub(super) fn new() -> Self {
        Self {
            values: HashMap::new(),
            frames: vec![CounterFrame {
                base_lengths: HashMap::new(),
                scope_id: 0,
                counter_mutation_floor: 0,
            }],
            next_scope_id: 1,
        }
    }

    pub(super) fn stacks(&self) -> HashMap<String, Vec<i32>> {
        self.values
            .iter()
            .map(|(name, instances)| {
                (
                    name.clone(),
                    instances
                        .iter()
                        .map(|instance| instance.value.get())
                        .collect(),
                )
            })
            .collect()
    }

    fn from_stacks(stacks: HashMap<String, Vec<i32>>) -> Self {
        let values = stacks
            .into_iter()
            .map(|(name, values)| {
                let values = values
                    .into_iter()
                    .map(|value| CounterInstance {
                        value: CounterValue::new(value),
                        reversed: false,
                        creator: None,
                        creator_scope: 0,
                    })
                    .collect();
                (name, values)
            })
            .collect();
        Self {
            values,
            frames: vec![CounterFrame {
                base_lengths: HashMap::new(),
                scope_id: 0,
                counter_mutation_floor: 0,
            }],
            next_scope_id: 1,
        }
    }

    pub(super) fn current(&self, name: &str) -> Option<i32> {
        self.values
            .get(name)
            .and_then(|values| values.last())
            .map(|instance| instance.value.get())
    }

    fn current_is_reversed(&self, name: &str) -> bool {
        self.values
            .get(name)
            .and_then(|values| values.last())
            .filter(|instance| instance.creator_scope >= self.counter_mutation_floor())
            .is_some_and(|instance| instance.reversed)
    }

    fn push_frame(&mut self, establishes_style_containment: bool) {
        let scope_id = self.next_scope_id;
        self.next_scope_id = self.next_scope_id.saturating_add(1);
        let inherited_floor = self
            .frames
            .last()
            .map_or(0, |frame| frame.counter_mutation_floor);
        self.frames.push(CounterFrame {
            base_lengths: HashMap::new(),
            scope_id,
            counter_mutation_floor: if establishes_style_containment {
                scope_id
            } else {
                inherited_floor
            },
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
                scope_id: 0,
                counter_mutation_floor: 0,
            });
        }
    }

    /// Applies `counter-reset` by instantiating a counter in the current scope.
    ///
    /// CSS Lists 3 scopes counters created by `counter-reset` to the element's
    /// descendants and following siblings, replacing earlier counters of the
    /// same name from the same parent scope:
    /// <https://www.w3.org/TR/css-lists-3/#inheriting-counters>.
    fn reset_counter(
        &mut self,
        reset: &CounterReset,
        value: CounterValue,
        creator: CounterOriginKey,
    ) {
        let name = reset.name.as_str();
        let current_scope = self.frames.last().map_or(0, |frame| frame.scope_id);
        let replaces_previous_sibling = self
            .values
            .get(name)
            .and_then(|values| values.last())
            .is_some_and(|previous| {
                previous.creator == Some(creator) || previous.creator_scope == current_scope
            });
        let base_len = self.current_frame_base_len(name, replaces_previous_sibling);
        let creator_scope = self.frames.last().map_or(0, |frame| frame.scope_id);
        let values = self.values.entry(name.to_string()).or_default();
        if replaces_previous_sibling {
            values.truncate(base_len);
        } else {
            debug_assert_eq!(
                values.len(),
                base_len,
                "only counters created by preceding siblings may be replaced"
            );
        }
        values.push(CounterInstance {
            value,
            reversed: reset.kind.is_reversed(),
            creator: Some(creator),
            creator_scope,
        });
    }

    /// Applies `counter-increment` to the innermost counter, instantiating a
    /// missing counter at `0` first as defined by CSS Lists 3:
    /// <https://www.w3.org/TR/css-lists-3/#propdef-counter-increment>.
    fn increment_counter(&mut self, name: &str, amount: CounterValue, creator: CounterOriginKey) {
        self.ensure_counter(name, creator);
        if let Some(instance) = self
            .values
            .get_mut(name)
            .and_then(|values| values.last_mut())
        {
            instance.value = instance.value.add(amount);
        }
    }

    /// Applies `counter-set` to the innermost counter, instantiating a missing
    /// counter at `0` first as defined by CSS Lists 3:
    /// <https://www.w3.org/TR/css-lists-3/#propdef-counter-set>.
    fn set_counter(&mut self, name: &str, value: CounterValue, creator: CounterOriginKey) {
        self.ensure_counter(name, creator);
        if let Some(instance) = self
            .values
            .get_mut(name)
            .and_then(|values| values.last_mut())
        {
            instance.value = value;
        }
    }

    fn ensure_counter(&mut self, name: &str, creator: CounterOriginKey) {
        if self
            .values
            .get(name)
            .and_then(|values| values.last())
            .is_some_and(|instance| instance.creator_scope >= self.counter_mutation_floor())
        {
            return;
        }
        let base_len = self.current_frame_base_len(name, false);
        let creator_scope = self.frames.last().map_or(0, |frame| frame.scope_id);
        let values = self.values.entry(name.to_string()).or_default();
        values.truncate(base_len);
        values.push(CounterInstance {
            value: CounterValue::ZERO,
            reversed: false,
            creator: Some(creator),
            creator_scope,
        });
    }

    fn current_frame_base_len(&mut self, name: &str, replaces_previous_sibling: bool) -> usize {
        let current_len = self.values.get(name).map_or(0, Vec::len);
        self.frames
            .last_mut()
            .expect("counter set always has a root frame")
            .base_lengths
            .entry(name.to_string())
            .or_insert_with(|| current_len.saturating_sub(replaces_previous_sibling as usize))
            .to_owned()
    }

    fn counter_mutation_floor(&self) -> usize {
        self.frames
            .last()
            .map_or(0, |frame| frame.counter_mutation_floor)
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Emits `string-set` assignments from a `display: none` source boundary.
    ///
    /// GCPM assigns these strings where the source content box would have
    /// been created. The source has no generated box, counter effects, page
    /// break, or paint; its zero-size marker is therefore the active layout
    /// cursor supplied by the adjacent visible source boundary.
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>
    pub(in crate::layout) fn capture_suppressed_named_strings_before(
        &mut self,
        element: ElementId,
    ) {
        let events = self
            .suppressed_named_strings_before
            .remove(&element)
            .unwrap_or_default();
        self.capture_suppressed_named_string_events(events);
    }

    pub(in crate::layout) fn capture_suppressed_named_strings_after(&mut self, element: ElementId) {
        let events = self
            .suppressed_named_strings_after
            .remove(&element)
            .unwrap_or_default();
        self.capture_suppressed_named_string_events(events);
    }

    fn capture_suppressed_named_string_events(
        &mut self,
        events: Vec<box_tree::SuppressedNamedStringEvent>,
    ) {
        for event in events {
            let ids = self.capture_named_strings(&event.element, &event.style);
            let placement = AssignmentPlacement {
                page_index: self.pages.len(),
                starts_page_fragment: !self.current_page_has_content(),
                border_box: Some(PaintClip::from_paint_rect(paint_space_rect(
                    self.content_left,
                    self.cursor_y,
                    0.0,
                    0.0,
                ))),
            };
            self.update_named_assignment_placements(&ids, placement);
        }
    }

    pub(super) fn prepare_counter_plan(&mut self, events: &[box_tree::CounterEventNode<'_>]) {
        self.counter_plan = CounterPlanBuilder::build(events);
        log::trace!(
            target: "quire::layout::counters",
            "prepared {} unresolved reversed counter starts",
            self.counter_plan.reversed_initial_values.len()
        );
    }

    pub(super) fn begin_counter_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> CounterScopeState {
        self.apply_counter_effects(element, box_tree::CounterEventSource::Principal, style);
        self.counter_set.push_frame(style.contain.style);
        CounterScopeState {
            previous_counter_set: None,
            previous_quote_depth: style.contain.style.then_some(self.quote_depth),
        }
    }

    pub(super) fn end_counter_scope(&mut self, state: CounterScopeState) {
        if let Some(previous_quote_depth) = state.previous_quote_depth {
            self.quote_depth = previous_quote_depth;
        }
        if let Some(previous_counter_set) = state.previous_counter_set {
            self.counter_set = previous_counter_set;
            return;
        }
        self.counter_set.pop_frame();
    }

    pub(super) fn begin_pseudo_counter_scope(
        &mut self,
        element: &Element,
        source: box_tree::CounterEventSource,
        style: &ComputedStyle,
    ) -> CounterScopeState {
        let counter_stacks = self.counter_stacks_at_origin(element, source);
        let previous_counter_set = std::mem::replace(
            &mut self.counter_set,
            CounterSet::from_stacks(counter_stacks),
        );
        self.counter_set.push_frame(style.contain.style);
        CounterScopeState {
            previous_counter_set: Some(previous_counter_set),
            previous_quote_depth: style.contain.style.then_some(self.quote_depth),
        }
    }

    fn apply_counter_effects(
        &mut self,
        element: &Element,
        source: box_tree::CounterEventSource,
        style: &ComputedStyle,
    ) {
        let origin = CounterOriginKey::new(element, source);
        for (declaration_index, reset) in style.counter_resets.iter().enumerate() {
            let value = self.counter_reset_initial_value(origin, declaration_index, reset);
            self.counter_set.reset_counter(reset, value, origin);
        }

        let increments = self.effective_counter_increments(style);
        for (name, amount) in increments {
            self.counter_set.increment_counter(&name, amount, origin);
        }

        for change in &style.counter_sets {
            self.counter_set
                .set_counter(&change.name, change.value, origin);
        }
    }

    fn counter_reset_initial_value(
        &self,
        origin: CounterOriginKey,
        declaration_index: usize,
        reset: &CounterReset,
    ) -> CounterValue {
        reset.kind.explicit_value().unwrap_or_else(|| {
            let key = CounterResetKey {
                origin,
                declaration_index,
            };
            let value = self
                .counter_plan
                .reversed_initial_values
                .get(&key)
                .cloned()
                .unwrap_or(CounterValue::ZERO);
            log::trace!(
                target: "quire::layout::counters",
                "resolved reversed counter {} at {:?} to {}",
                reset.name,
                key,
                value.get()
            );
            value
        })
    }

    fn effective_counter_increments(&self, style: &ComputedStyle) -> Vec<(String, CounterValue)> {
        let mut increments = style
            .counter_increments
            .iter()
            .map(|change| (change.name.clone(), change.value))
            .collect::<Vec<_>>();
        if style.display.is_list_item()
            && !increments
                .iter()
                .any(|(name, _)| name.as_str() == LIST_ITEM_COUNTER_NAME)
        {
            let amount = if self.counter_set.current_is_reversed(LIST_ITEM_COUNTER_NAME) {
                -1
            } else {
                1
            };
            increments.push((
                LIST_ITEM_COUNTER_NAME.to_string(),
                CounterValue::new(amount),
            ));
        }
        increments
    }

    /// Returns the immutable post-event counter snapshot for a generated box.
    ///
    /// Counter values are planned from logical source order before pagination,
    /// so speculative layout and fragment replay cannot apply the same event a
    /// second time. The runtime state is retained only for layout entry points
    /// that do not originate in the durable formatting tree.
    /// <https://drafts.csswg.org/css-lists-3/#creating-counters>
    pub(in crate::layout) fn counter_stacks_at_origin(
        &self,
        element: &Element,
        source: box_tree::CounterEventSource,
    ) -> HashMap<String, Vec<i32>> {
        self.counter_plan
            .values_at_origin
            .get(&CounterOriginKey::new(element, source))
            .cloned()
            .unwrap_or_else(|| self.counter_set.stacks())
    }

    pub(in crate::layout) fn evaluate_generated_pseudo_text_rollback(
        &mut self,
        element: &Element,
        source: box_tree::CounterEventSource,
        pseudo_style: Option<&ComputedStyle>,
    ) -> String {
        let Some(pseudo_style) = pseudo_style else {
            return String::new();
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return String::new();
        };

        let counter_stacks = self.counter_stacks_at_origin(element, source);
        evaluate_generated_content_text(
            element,
            content,
            &counter_stacks,
            &self.counter_styles,
            counter_styles::CounterStyleRenderContext::for_style(pseudo_style),
        )
    }

    fn evaluate_named_string_set_with_counter_scopes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        set: &crate::css::NamedStringSet,
    ) -> Vec<page_generated::PageMarginContentItem> {
        let mut output = Vec::new();
        let counter_stacks =
            self.counter_stacks_at_origin(element, box_tree::CounterEventSource::Principal);
        for part in &set.parts {
            match part {
                NamedStringPart::String(text) => {
                    push_named_string_text_part(&mut output, text);
                }
                NamedStringPart::ContentText => {
                    push_named_string_text_part(&mut output, &inline_text(element));
                }
                NamedStringPart::ContentFirstLetter => {
                    let text = inline_text(element);
                    if let Some(range) = first_letter_byte_range(&text) {
                        push_named_string_text_part(&mut output, &text[range]);
                    }
                }
                NamedStringPart::ContentMarker => {
                    if let Some(marker) =
                        self.marker_for_list_item(element, style, self.containing_block_direction)
                        && !marker.text.is_empty()
                    {
                        push_named_string_text_part(&mut output, &marker.text);
                    }
                }
                NamedStringPart::BeforeContent => {
                    let items = self.evaluate_generated_pseudo_items_rollback(
                        element,
                        box_tree::CounterEventSource::Before,
                        style.before_style.as_deref(),
                    );
                    push_named_string_items(&mut output, items);
                }
                NamedStringPart::AfterContent => {
                    let items = self.evaluate_generated_pseudo_items_rollback(
                        element,
                        box_tree::CounterEventSource::After,
                        style.after_style.as_deref(),
                    );
                    push_named_string_items(&mut output, items);
                }
                NamedStringPart::Attr { name, fallback } => {
                    if let Some(value) = element.unprefixed_css_attr(name) {
                        push_named_string_text_part(&mut output, value);
                    } else if let Some(fallback) = fallback {
                        push_named_string_text_part(&mut output, fallback);
                    }
                }
                NamedStringPart::Image(image) => output.push(
                    page_generated::PageMarginContentItem::Inline(GeneratedContentPart::Image {
                        image: image_with_context_urls(image.clone(), self.base_url, self.root_url),
                    }),
                ),
                NamedStringPart::Quote(quote) => {
                    output.push(page_generated::PageMarginContentItem::Inline(
                        GeneratedContentPart::Quote(*quote),
                    ))
                }
                NamedStringPart::Leader(text) => {
                    output.push(page_generated::PageMarginContentItem::Inline(
                        GeneratedContentPart::Leader(text.clone()),
                    ))
                }
                NamedStringPart::Counter {
                    name,
                    style: counter_style,
                } => {
                    if name.eq_ignore_ascii_case("page") || name.eq_ignore_ascii_case("pages") {
                        output.push(
                            page_generated::PageMarginContentItem::NamedStringPageCounter {
                                name: name.clone(),
                                separator: None,
                                style: counter_style.clone(),
                            },
                        );
                        continue;
                    }
                    let value = counter_stacks
                        .get(name)
                        .and_then(|values| values.last().cloned())
                        .unwrap_or(0);
                    if let Some(counter) = counter_styles::counter_text_with_context(
                        counter_style.clone().unwrap_or(ListStyleType::Decimal),
                        value,
                        &self.counter_styles,
                        counter_styles::CounterStyleRenderContext::for_style(style),
                    ) {
                        push_named_string_text_part(&mut output, &counter);
                    }
                }
                NamedStringPart::Counters {
                    name,
                    separator,
                    style: counter_style,
                } => {
                    if name.eq_ignore_ascii_case("page") || name.eq_ignore_ascii_case("pages") {
                        output.push(
                            page_generated::PageMarginContentItem::NamedStringPageCounter {
                                name: name.clone(),
                                separator: Some(separator.clone()),
                                style: counter_style.clone(),
                            },
                        );
                        continue;
                    }
                    let counter_style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                    let counters = counter_stacks
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| vec![0])
                        .into_iter()
                        .filter_map(|value| {
                            counter_styles::counter_text_with_context(
                                counter_style.clone(),
                                value,
                                &self.counter_styles,
                                counter_styles::CounterStyleRenderContext::for_style(style),
                            )
                        })
                        .collect::<Vec<_>>();
                    push_named_string_text_part(&mut output, &counters.join(separator));
                }
                NamedStringPart::TargetCounter {
                    target,
                    name,
                    style,
                } => output.push(page_generated::PageMarginContentItem::TargetCounter {
                    target: target.clone(),
                    name: name.clone(),
                    style: style.clone(),
                }),
                NamedStringPart::TargetText { target, keyword } => {
                    output.push(page_generated::PageMarginContentItem::TargetText {
                        target: target.clone(),
                        keyword: *keyword,
                    })
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
        source: box_tree::CounterEventSource,
        pseudo_style: Option<&ComputedStyle>,
    ) -> Vec<page_generated::PageMarginContentItem> {
        let Some(pseudo_style) = pseudo_style else {
            return Vec::new();
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return Vec::new();
        };
        let counter_stacks = self.counter_stacks_at_origin(element, source);
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
                        &counter_stacks,
                        &self.counter_styles,
                        counter_styles::CounterStyleRenderContext::for_style(pseudo_style),
                    );
                    push_named_string_text_part(&mut output, &text);
                }
                GeneratedContentPart::TargetCounter {
                    target,
                    name,
                    style,
                } => output.push(page_generated::PageMarginContentItem::TargetCounter {
                    target: target.clone(),
                    name: name.clone(),
                    style: style.clone(),
                }),
                GeneratedContentPart::TargetText { target, keyword } => {
                    output.push(page_generated::PageMarginContentItem::TargetText {
                        target: target.clone(),
                        keyword: *keyword,
                    })
                }
                GeneratedContentPart::Quote(_)
                | GeneratedContentPart::Leader(_)
                | GeneratedContentPart::Image { .. } => {
                    output.push(page_generated::PageMarginContentItem::Inline(part.clone()))
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
        let mut output = self.evaluate_generated_pseudo_text_rollback(
            element,
            box_tree::CounterEventSource::Before,
            style.before_style.as_deref(),
        );
        output.push_str(&inline_text(element));
        output.push_str(&self.evaluate_generated_pseudo_text_rollback(
            element,
            box_tree::CounterEventSource::After,
            style.after_style.as_deref(),
        ));
        output
    }

    pub(super) fn capture_named_strings(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Vec<AssignmentId> {
        if self.element_side_effect_suppression_depth > 0 {
            return Vec::new();
        }
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
        let css::Position::Running(name) = &style.position else {
            return false;
        };
        if self.element_side_effect_suppression_depth > 0 {
            return true;
        }
        let value = self.running_element_text_with_counter_scopes(element, style);
        let content_parts =
            running_element_content_parts(element, &value, self.base_url, self.root_url);
        let placement = self.running_element_source_marker_placement();
        let id = self.next_assignment_id();
        self.current_page_running_elements
            .entry(name.as_str().to_owned())
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

    /// Captures GCPM assignments for a source element represented by an already-planned fragment.
    ///
    /// Split table-cell replay can paint a source fragment without passing
    /// through the normal element layout wrapper. GCPM `string(..., start)` and
    /// `element(..., start)` still resolve from the final source fragment, so
    /// callers provide the fragment-backed placement directly:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings> and
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    pub(in crate::layout) fn capture_assignments_for_fragment_source(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        placement: AssignmentPlacement,
    ) -> bool {
        self.capture_assignments_for_fragment_source_with_ids(element, style, placement)
            .0
    }

    /// Captures assignments for a committed fragment source and returns their
    /// durable ids for the fragment plan that owns the source paint.
    ///
    /// Speculative replay restores page-side-effect maps, so its assignment
    /// ids cannot be used after the snapshot is discarded. Fragment planners
    /// call this on the committed source fragment instead.
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>
    pub(in crate::layout) fn capture_assignments_for_fragment_source_with_ids(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        placement: AssignmentPlacement,
    ) -> (bool, Vec<AssignmentId>) {
        if style.display.is_none() {
            return (false, Vec::new());
        }
        let counter_scope = self.begin_counter_scope(element, style);
        self.begin_assignment_capture_frame();
        let named_assignment_ids = self.capture_named_strings(element, style);
        let captured_running_element = self.capture_running_element(element, style);
        let assignment_ids = self.end_assignment_capture_frame();
        self.update_named_assignment_placements(&named_assignment_ids, placement);
        self.update_running_assignment_placements(&assignment_ids, placement);
        self.end_counter_scope(counter_scope);
        (captured_running_element, assignment_ids)
    }

    /// Captures named strings for a source element represented by a final fragment.
    ///
    /// Table row fragments do not pass through the normal block element wrapper,
    /// but CSS GCPM still sets named strings when the source element is laid out.
    /// Running table rows require removal from table layout, so this helper is
    /// deliberately limited to `string-set` and updates placement from the final
    /// visible row fragment:
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>.
    pub(in crate::layout) fn capture_named_strings_for_fragment_source(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        placement: AssignmentPlacement,
    ) {
        if style.display.is_none() || style.string_sets.is_empty() {
            return;
        }
        let counter_scope = self.begin_counter_scope(element, style);
        let named_assignment_ids = self.capture_named_strings(element, style);
        self.update_named_assignment_placements(&named_assignment_ids, placement);
        self.end_counter_scope(counter_scope);
    }

    /// Copies GCPM assignment values emitted in an isolated fragment layout.
    ///
    /// Split table-cell nested table/flex replay lays descendants out on a
    /// temporary page and restores the caller state afterwards. Capture the
    /// values before restore, then re-emit them against the real page fragment:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings> and
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    pub(in crate::layout) fn captured_current_page_assignment_values(
        &self,
    ) -> Vec<CapturedPageAssignment> {
        let mut output = Vec::new();
        push_captured_assignment_values(&mut output, &self.current_page_named_strings);
        push_captured_assignment_values(&mut output, &self.current_page_running_elements);
        output
    }

    pub(in crate::layout) fn replay_captured_page_assignments(
        &mut self,
        assignments: &[CapturedPageAssignment],
        placement: AssignmentPlacement,
    ) {
        for assignment in assignments {
            let id = self.next_assignment_id();
            let page_assignment = NamedStringAssignment {
                id,
                value: assignment.value.clone(),
                placement,
            };
            match &assignment.value {
                PageAssignmentValue::GeneratedContent(_) => {
                    insert_assignment_for_page(
                        &mut self.page_named_strings,
                        &mut self.current_page_named_strings,
                        placement.page_index,
                        assignment.name.clone(),
                        page_assignment,
                        self.pages.len(),
                    );
                }
                PageAssignmentValue::RunningElement(_) => {
                    insert_assignment_for_page(
                        &mut self.page_running_elements,
                        &mut self.current_page_running_elements,
                        placement.page_index,
                        assignment.name.clone(),
                        page_assignment,
                        self.pages.len(),
                    );
                }
            }
        }
    }
}

fn push_captured_assignment_values(
    output: &mut Vec<CapturedPageAssignment>,
    assignments: &HashMap<String, Vec<NamedStringAssignment>>,
) {
    output.extend(assignments.iter().flat_map(|(name, values)| {
        values.iter().map(move |assignment| CapturedPageAssignment {
            name: name.clone(),
            value: assignment.value.clone(),
        })
    }));
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

fn image_with_context_urls(
    mut image: css::ComputedImage,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> css::ComputedImage {
    let Some(mut selected) = image.as_image_mut() else {
        return image;
    };
    while let css::BackgroundImage::SelectedImageSet { image, .. } = selected {
        selected = image;
    }
    if let css::BackgroundImage::Url(css::ImageUrl {
        base_url: image_base_url,
        root_url: image_root_url,
        ..
    }) = selected
    {
        if image_base_url.is_none() {
            *image_base_url = base_url.cloned();
        }
        if image_root_url.is_none() {
            *image_root_url = root_url.cloned();
        }
    }
    image
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
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Vec<GeneratedContentPart> {
    if element.tag.eq_ignore_ascii_case("img")
        && let Some(url) = element.attrs.get("src").filter(|value| !value.is_empty())
    {
        return vec![GeneratedContentPart::Image {
            image: css::ComputedImage::image(css::BackgroundImage::Url(css::ImageUrl {
                href: url.clone(),
                base_url: base_url.cloned(),
                root_url: root_url.cloned(),
                request_modifiers: css::RequestUrlModifiers::default(),
            })),
        }];
    }
    if fallback_text.is_empty() {
        Vec::new()
    } else {
        vec![GeneratedContentPart::Text(fallback_text.to_string())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter_event<'a>(
        element: &'a Element,
        resets: Vec<CounterReset>,
        increments: Vec<css::CounterChange>,
        sets: Vec<css::CounterChange>,
        children: Vec<box_tree::CounterEventNode<'a>>,
    ) -> box_tree::CounterEventNode<'a> {
        box_tree::CounterEventNode {
            element,
            source: box_tree::CounterEventSource::Principal,
            counter_style: box_tree::CounterEventStyle {
                counter_resets: resets,
                counter_increments: increments,
                counter_sets: sets,
                is_list_item: false,
                style_containment: false,
            },
            children,
        }
    }

    fn counter_reset(name: &str, kind: CounterResetKind) -> CounterReset {
        CounterReset {
            name: name.to_string(),
            kind,
        }
    }

    fn counter_change(name: &str, value: i32) -> css::CounterChange {
        css::CounterChange {
            name: name.to_string(),
            value: CounterValue::new(value),
        }
    }

    fn element(tag: &str) -> Element {
        let NodeKind::Element(element) = Node::element(tag).kind else {
            unreachable!("Node::element always creates an element")
        };
        element
    }

    fn principal_origin(element: &Element) -> CounterOriginKey {
        CounterOriginKey::new(element, box_tree::CounterEventSource::Principal)
    }

    fn test_counter_origin() -> CounterOriginKey {
        CounterOriginKey {
            element_id: ElementId::next(),
            source: box_tree::CounterEventSource::Principal,
        }
    }

    fn seeded_counter(value: i32) -> CounterSet {
        let mut counters = CounterSet::new();
        counters.values.insert(
            "section".to_string(),
            vec![CounterInstance {
                value: CounterValue::new(value),
                reversed: false,
                creator: None,
                creator_scope: 0,
            }],
        );
        counters
    }

    #[test]
    fn style_containment_shadows_outer_counter_for_direct_children() {
        let mut counters = seeded_counter(17);
        counters.push_frame(true);
        for _ in 0..5 {
            counters.increment_counter("section", CounterValue::new(4), test_counter_origin());
        }

        assert_eq!(counters.current("section"), Some(20));
        assert_eq!(counters.values["section"].len(), 2);
    }

    #[test]
    fn nested_style_containment_shadow_does_not_escape_parent() {
        let mut counters = seeded_counter(13);
        counters.push_frame(true);
        counters.push_frame(false);
        for _ in 0..4 {
            counters.increment_counter("section", CounterValue::new(5), test_counter_origin());
        }
        assert_eq!(counters.current("section"), Some(20));

        counters.pop_frame();
        assert_eq!(counters.current("section"), Some(13));
    }

    #[test]
    fn reversed_start_includes_following_sibling_descendants() {
        let creator = element("creator");
        let following_sibling = element("following-sibling");
        let descendant = element("descendant");
        let events = vec![
            counter_event(
                &creator,
                vec![counter_reset("foo", CounterResetKind::Reversed(None))],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &following_sibling,
                Vec::new(),
                vec![counter_change("foo", -2)],
                Vec::new(),
                vec![counter_event(
                    &descendant,
                    Vec::new(),
                    vec![counter_change("foo", -3)],
                    Vec::new(),
                    Vec::new(),
                )],
            ),
        ];

        let plan = CounterPlanBuilder::build(&events);
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&creator),
                declaration_index: 0,
            }]
                .get(),
            8
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&following_sibling)]["foo"],
            [6]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&descendant)]["foo"],
            [3]
        );
    }

    #[test]
    fn later_sibling_reset_shadows_reversed_counter_scope() {
        let creator = element("creator");
        let incrementer = element("incrementer");
        let shadow = element("shadow");
        let after_shadow = element("after-shadow");
        let events = vec![
            counter_event(
                &creator,
                vec![counter_reset("foo", CounterResetKind::Reversed(None))],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &incrementer,
                Vec::new(),
                vec![counter_change("foo", -2)],
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &shadow,
                vec![counter_reset(
                    "foo",
                    CounterResetKind::Forward(CounterValue::ZERO),
                )],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &after_shadow,
                Vec::new(),
                vec![counter_change("foo", -5)],
                Vec::new(),
                Vec::new(),
            ),
        ];

        let plan = CounterPlanBuilder::build(&events);
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&creator),
                declaration_index: 0,
            }]
                .get(),
            4
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&after_shadow)]["foo"],
            [-5]
        );
    }

    #[test]
    fn reversed_start_uses_last_nonzero_increment_before_first_set() {
        let creator = element("creator");
        let first = element("first");
        let zero = element("zero");
        let setter = element("setter");
        let events = vec![
            counter_event(
                &creator,
                vec![counter_reset("foo", CounterResetKind::Reversed(None))],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &first,
                Vec::new(),
                vec![counter_change("foo", -2)],
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &zero,
                Vec::new(),
                vec![counter_change("foo", 0)],
                Vec::new(),
                Vec::new(),
            ),
            counter_event(
                &setter,
                Vec::new(),
                vec![counter_change("foo", -7)],
                vec![counter_change("foo", 5)],
                Vec::new(),
            ),
        ];

        let plan = CounterPlanBuilder::build(&events);
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&creator),
                declaration_index: 0,
            }]
                .get(),
            14
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&setter)]["foo"],
            [5]
        );
    }

    #[test]
    fn nested_reversed_counter_is_a_distinct_scope() {
        let outer = element("outer");
        let first_outer = element("first-outer");
        let inner = element("inner");
        let inner_item = element("inner-item");
        let final_outer = element("final-outer");
        let events = vec![counter_event(
            &outer,
            vec![counter_reset("foo", CounterResetKind::Reversed(None))],
            Vec::new(),
            Vec::new(),
            vec![
                counter_event(
                    &first_outer,
                    Vec::new(),
                    vec![counter_change("foo", -2)],
                    Vec::new(),
                    vec![counter_event(
                        &inner,
                        vec![counter_reset("foo", CounterResetKind::Reversed(None))],
                        Vec::new(),
                        Vec::new(),
                        vec![counter_event(
                            &inner_item,
                            Vec::new(),
                            vec![counter_change("foo", -3)],
                            Vec::new(),
                            Vec::new(),
                        )],
                    )],
                ),
                counter_event(
                    &final_outer,
                    Vec::new(),
                    vec![counter_change("foo", -2)],
                    Vec::new(),
                    Vec::new(),
                ),
            ],
        )];

        let plan = CounterPlanBuilder::build(&events);
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&outer),
                declaration_index: 0,
            }]
                .get(),
            6
        );
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&inner),
                declaration_index: 0,
            }]
                .get(),
            6
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&inner_item)]["foo"],
            [4, 3]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&final_outer)]["foo"],
            [2]
        );
    }

    #[test]
    fn nested_reversed_starts_resume_the_enclosing_sibling_scope() {
        let outer = element("outer");
        let inner = element("inner");
        let inner_zero = element("inner-zero");
        let inner_first = element("inner-first");
        let inner_second = element("inner-second");
        let inner_third = element("inner-third");
        let outer_zero = element("outer-zero");
        let outer_first = element("outer-first");
        let outer_second = element("outer-second");
        let outer_third = element("outer-third");
        let events = vec![counter_event(
            &outer,
            vec![counter_reset("foo", CounterResetKind::Reversed(None))],
            Vec::new(),
            Vec::new(),
            vec![
                counter_event(
                    &inner,
                    vec![counter_reset("foo", CounterResetKind::Reversed(None))],
                    Vec::new(),
                    Vec::new(),
                    vec![
                        counter_event(
                            &inner_zero,
                            Vec::new(),
                            vec![counter_change("foo", 0)],
                            Vec::new(),
                            Vec::new(),
                        ),
                        counter_event(
                            &inner_first,
                            Vec::new(),
                            vec![counter_change("foo", 3)],
                            Vec::new(),
                            Vec::new(),
                        ),
                        counter_event(
                            &inner_second,
                            Vec::new(),
                            vec![counter_change("foo", 1)],
                            Vec::new(),
                            Vec::new(),
                        ),
                        counter_event(
                            &inner_third,
                            Vec::new(),
                            vec![counter_change("foo", 8)],
                            Vec::new(),
                            Vec::new(),
                        ),
                    ],
                ),
                counter_event(
                    &outer_zero,
                    Vec::new(),
                    vec![counter_change("foo", 0)],
                    Vec::new(),
                    Vec::new(),
                ),
                counter_event(
                    &outer_first,
                    Vec::new(),
                    vec![counter_change("foo", 3)],
                    Vec::new(),
                    Vec::new(),
                ),
                counter_event(
                    &outer_second,
                    Vec::new(),
                    vec![counter_change("foo", 1)],
                    Vec::new(),
                    Vec::new(),
                ),
                counter_event(
                    &outer_third,
                    Vec::new(),
                    vec![counter_change("foo", 8)],
                    Vec::new(),
                    Vec::new(),
                ),
            ],
        )];

        let plan = CounterPlanBuilder::build(&events);
        for element in [&outer, &inner] {
            assert_eq!(
                plan.reversed_initial_values[&CounterResetKey {
                    origin: principal_origin(element),
                    declaration_index: 0,
                }]
                    .get(),
                -20
            );
        }
        assert_eq!(
            plan.values_at_origin[&principal_origin(&inner_third)]["foo"],
            [-20, -8]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&outer_first)]["foo"],
            [-20, -5]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&outer_third)]["foo"],
            [-20, 4]
        );
    }

    #[test]
    fn reversed_list_item_counter_uses_implicit_decrements() {
        let list = element("list");
        let first = element("first");
        let second = element("second");
        let third = element("third");
        let mut first_event = counter_event(&first, Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut second_event =
            counter_event(&second, Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut third_event = counter_event(&third, Vec::new(), Vec::new(), Vec::new(), Vec::new());
        first_event.counter_style.is_list_item = true;
        second_event.counter_style.is_list_item = true;
        third_event.counter_style.is_list_item = true;
        let events = vec![counter_event(
            &list,
            vec![counter_reset(
                LIST_ITEM_COUNTER_NAME,
                CounterResetKind::Reversed(None),
            )],
            Vec::new(),
            Vec::new(),
            vec![first_event, second_event, third_event],
        )];

        let plan = CounterPlanBuilder::build(&events);
        assert_eq!(
            plan.reversed_initial_values[&CounterResetKey {
                origin: principal_origin(&list),
                declaration_index: 0,
            }]
                .get(),
            4
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&first)][LIST_ITEM_COUNTER_NAME],
            [3]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&second)][LIST_ITEM_COUNTER_NAME],
            [2]
        );
        assert_eq!(
            plan.values_at_origin[&principal_origin(&third)][LIST_ITEM_COUNTER_NAME],
            [1]
        );
    }
}
