use super::*;
use std::rc::Rc;

/// Computed CSS value for `row-gap` and `column-gap`.
///
/// CSS Box Alignment defines gap properties as `normal | <length-percentage>`,
/// and CSS Cascade keeps `normal` as a computed keyword until the relevant
/// layout mode computes used values:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedGap {
    Normal,
    LengthPercentage(ComputedLengthPercentage),
}

impl ComputedGap {
    pub(crate) const NORMAL: Self = Self::Normal;

    /// Scale fixed gap components at the CSS `zoom` used-value boundary.
    ///
    /// The percentage coefficient remains unchanged so it resolves against
    /// the already zoomed layout container.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-align-3/#gap-shorthand>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::LengthPercentage(value) = self {
            value.scale_fixed_length_components(factor);
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    /// Reduces CSS Math comparisons whose percentage basis is non-negative.
    ///
    /// Gap percentages resolve against a content-box size, which cannot be
    /// negative. This permits pure-percentage comparisons to reduce without
    /// prematurely choosing a branch for mixed length-percentage values.
    /// <https://www.w3.org/TR/css-align-3/#gaps> and
    /// <https://www.w3.org/TR/css-values-4/#comp-func>
    pub(crate) fn reduce_math_with_nonnegative_percentage_basis(&mut self) {
        if let Self::LengthPercentage(value) = self {
            value.reduce_math_with_nonnegative_percentage_basis();
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }
}

/// Computed CSS gap-decoration list with optional `repeat(auto, ...)`.
///
/// CSS Gaps Level 1 lets rule width/style/color properties accept comma lists
/// and one optional auto repeater. Layout assigns the resulting pattern to the
/// actual list of gaps after gutter collapsing and fragmentation decisions:
/// <https://drafts.csswg.org/css-gaps-1/#lists-repeat> and
/// <https://drafts.csswg.org/css-gaps-1/#assigning>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GapRuleList<T> {
    pub(crate) leading: Rc<[GapRuleListComponent<T>]>,
    pub(crate) auto: Option<Rc<[T]>>,
    pub(crate) trailing: Rc<[GapRuleListComponent<T>]>,
}

impl<T> GapRuleList<T> {
    pub(crate) fn single(value: T) -> Self {
        Self::from_parts(vec![GapRuleListComponent::Value(value)], None, Vec::new())
    }

    pub(crate) fn from_parts(
        leading: Vec<GapRuleListComponent<T>>,
        auto: Option<Vec<T>>,
        trailing: Vec<GapRuleListComponent<T>>,
    ) -> Self {
        Self {
            leading: Rc::from(leading.into_boxed_slice()),
            auto: auto.map(|values| Rc::from(values.into_boxed_slice())),
            trailing: Rc::from(trailing.into_boxed_slice()),
        }
    }
}

impl<T: Clone> GapRuleList<T> {
    pub(crate) fn value_for_index(&self, index: usize, count: usize) -> Option<T> {
        if index >= count {
            return None;
        }
        let leading_len = expanded_gap_rule_components_len(&self.leading);
        let trailing_len = expanded_gap_rule_components_len(&self.trailing);
        let Some(auto) = &self.auto else {
            if leading_len == 0 {
                return None;
            }
            return gap_rule_components_value_at(&self.leading, index % leading_len);
        };

        let leading_count = leading_len.min(count);
        if index < leading_count {
            return gap_rule_components_value_at(&self.leading, index);
        }

        let trailing_count = trailing_len.min(count.saturating_sub(leading_count));
        let auto_count = count.saturating_sub(leading_count + trailing_count);
        let auto_index = index - leading_count;
        if auto_index < auto_count {
            if auto.is_empty() {
                return None;
            }
            return Some(auto[auto_index % auto.len()].clone());
        }

        let trailing_index = auto_index - auto_count;
        gap_rule_components_value_at(&self.trailing, trailing_index)
    }

    #[cfg(test)]
    pub(crate) fn values_for_count(&self, count: usize) -> Vec<T> {
        (0..count)
            .filter_map(|index| self.value_for_index(index, count))
            .collect()
    }
}

fn expanded_gap_rule_components_len<T>(components: &[GapRuleListComponent<T>]) -> usize {
    components
        .iter()
        .map(|component| match component {
            GapRuleListComponent::Value(_) => 1,
            GapRuleListComponent::Repeat { count, values } => count.saturating_mul(values.len()),
        })
        .sum()
}

fn gap_rule_components_value_at<T: Clone>(
    components: &[GapRuleListComponent<T>],
    mut index: usize,
) -> Option<T> {
    for component in components {
        match component {
            GapRuleListComponent::Value(value) => {
                if index == 0 {
                    return Some(value.clone());
                }
                index -= 1;
            }
            GapRuleListComponent::Repeat { count, values } => {
                let len = count.saturating_mul(values.len());
                if index < len {
                    return Some(values[index % values.len()].clone());
                }
                index -= len;
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GapRuleListComponent<T> {
    Value(T),
    Repeat { count: usize, values: Vec<T> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GapRuleBreak {
    None,
    Normal,
    Intersection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GapRuleVisibilityItems {
    Normal,
    All,
    Around,
    Between,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GapRuleOverlap {
    RowOverColumn,
    ColumnOverRow,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GapRuleInsetValue {
    LengthPercentage(ComputedLengthPercentage),
    OverlapJoin,
}

impl GapRuleInsetValue {
    pub(crate) const ZERO: Self = Self::LengthPercentage(ComputedLengthPercentage::ZERO);

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::LengthPercentage(value) = self {
            value.scale_fixed_length_components(factor);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GapRuleAxis {
    pub(crate) widths: GapRuleList<ComputedLengthPercentage>,
    pub(crate) styles: GapRuleList<BorderStyle>,
    pub(crate) colors: GapRuleList<CssColor>,
    pub(crate) rule_break: GapRuleBreak,
    pub(crate) visibility_items: GapRuleVisibilityItems,
    pub(crate) inset_cap_start: GapRuleInsetValue,
    pub(crate) inset_cap_end: GapRuleInsetValue,
    pub(crate) inset_junction_start: GapRuleInsetValue,
    pub(crate) inset_junction_end: GapRuleInsetValue,
}

impl GapRuleAxis {
    pub(crate) fn initial() -> Self {
        Self {
            widths: GapRuleList::single(ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT)),
            styles: GapRuleList::single(BorderStyle::None),
            colors: GapRuleList::single(CssColor::BLACK),
            rule_break: GapRuleBreak::Normal,
            visibility_items: GapRuleVisibilityItems::Normal,
            inset_cap_start: GapRuleInsetValue::ZERO,
            inset_cap_end: GapRuleInsetValue::ZERO,
            inset_junction_start: GapRuleInsetValue::ZERO,
            inset_junction_end: GapRuleInsetValue::ZERO,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        resolve_gap_rule_width_list_font_lengths(&mut self.widths, ch_advance);
        self.inset_cap_start.resolve_font_metric_lengths(ch_advance);
        self.inset_cap_end.resolve_font_metric_lengths(ch_advance);
        self.inset_junction_start
            .resolve_font_metric_lengths(ch_advance);
        self.inset_junction_end
            .resolve_font_metric_lengths(ch_advance);
    }

    /// Scale rule widths and fixed endpoint insets at the CSS `zoom`
    /// used-value boundary. Percentage components remain relative to the
    /// already zoomed decorated gap geometry.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-properties>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        scale_gap_rule_width_list_fixed_components(&mut self.widths, factor);
        self.inset_cap_start.scale_fixed_length_components(factor);
        self.inset_cap_end.scale_fixed_length_components(factor);
        self.inset_junction_start
            .scale_fixed_length_components(factor);
        self.inset_junction_end
            .scale_fixed_length_components(factor);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        gap_rule_width_list_requires_ch_advance(&self.widths)
            || self.inset_cap_start.requires_ch_advance()
            || self.inset_cap_end.requires_ch_advance()
            || self.inset_junction_start.requires_ch_advance()
            || self.inset_junction_end.requires_ch_advance()
    }
}

fn scale_gap_rule_width_list_fixed_components(
    list: &mut GapRuleList<ComputedLengthPercentage>,
    factor: f32,
) {
    for component in Rc::make_mut(&mut list.leading)
        .iter_mut()
        .chain(Rc::make_mut(&mut list.trailing).iter_mut())
    {
        match component {
            GapRuleListComponent::Value(value) => value.scale_fixed_length_components(factor),
            GapRuleListComponent::Repeat { values, .. } => {
                for value in values {
                    value.scale_fixed_length_components(factor);
                }
            }
        }
    }
    if let Some(values) = &mut list.auto {
        for value in Rc::make_mut(values) {
            value.scale_fixed_length_components(factor);
        }
    }
}

fn resolve_gap_rule_width_list_font_lengths(
    list: &mut GapRuleList<ComputedLengthPercentage>,
    ch_advance: LayoutLength,
) {
    for component in Rc::make_mut(&mut list.leading)
        .iter_mut()
        .chain(Rc::make_mut(&mut list.trailing).iter_mut())
    {
        match component {
            GapRuleListComponent::Value(value) => value.resolve_font_metric_lengths(ch_advance),
            GapRuleListComponent::Repeat { values, .. } => {
                for value in values {
                    value.resolve_font_metric_lengths(ch_advance);
                }
            }
        }
    }
    if let Some(values) = &mut list.auto {
        for value in Rc::make_mut(values) {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }
}

fn gap_rule_width_list_requires_ch_advance(list: &GapRuleList<ComputedLengthPercentage>) -> bool {
    list.leading
        .iter()
        .chain(list.trailing.iter())
        .any(gap_rule_component_requires_ch_advance)
        || list.auto.as_deref().is_some_and(|values| {
            values
                .iter()
                .any(ComputedLengthPercentage::requires_ch_advance)
        })
}

fn gap_rule_component_requires_ch_advance(
    component: &GapRuleListComponent<ComputedLengthPercentage>,
) -> bool {
    match component {
        GapRuleListComponent::Value(value) => value.requires_ch_advance(),
        GapRuleListComponent::Repeat { values, .. } => values
            .iter()
            .any(ComputedLengthPercentage::requires_ch_advance),
    }
}

fn resolve_gap_rule_width_list_viewport_lengths(
    list: &mut GapRuleList<ComputedLengthPercentage>,
    basis: ViewportLengthBasis,
) {
    for component in Rc::make_mut(&mut list.leading)
        .iter_mut()
        .chain(Rc::make_mut(&mut list.trailing).iter_mut())
    {
        match component {
            GapRuleListComponent::Value(value) => value.resolve_viewport_lengths(basis),
            GapRuleListComponent::Repeat { values, .. } => {
                for value in values {
                    value.resolve_viewport_lengths(basis);
                }
            }
        }
    }
    if let Some(values) = &mut list.auto {
        for value in Rc::make_mut(values) {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for ComputedGap {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GapRuleInsetValue {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GapRuleAxis {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        resolve_gap_rule_width_list_viewport_lengths(&mut self.widths, basis);
        self.inset_cap_start.resolve_viewport_lengths(basis);
        self.inset_cap_end.resolve_viewport_lengths(basis);
        self.inset_junction_start.resolve_viewport_lengths(basis);
        self.inset_junction_end.resolve_viewport_lengths(basis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_gap_rule_list_truncates_excess_trailing_values_at_the_end() {
        let list = GapRuleList::from_parts(
            vec![
                GapRuleListComponent::Value(1),
                GapRuleListComponent::Value(2),
            ],
            Some(vec![3, 4]),
            vec![
                GapRuleListComponent::Value(5),
                GapRuleListComponent::Value(6),
                GapRuleListComponent::Value(7),
            ],
        );

        assert_eq!(list.values_for_count(4), [1, 2, 5, 6]);
        assert_eq!(list.values_for_count(7), [1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn auto_gap_rule_list_truncates_fixed_trailing_repeat_in_authored_order() {
        let list = GapRuleList::from_parts(
            vec![GapRuleListComponent::Value(1)],
            Some(vec![2]),
            vec![GapRuleListComponent::Repeat {
                count: 2,
                values: vec![3, 4],
            }],
        );

        assert_eq!(list.values_for_count(3), [1, 3, 4]);
        assert_eq!(list.values_for_count(6), [1, 2, 3, 4, 3, 4]);
    }

    #[test]
    fn gap_rule_list_without_auto_repeater_keeps_cycling_leading_values() {
        let list = GapRuleList::from_parts(
            vec![
                GapRuleListComponent::Value(1),
                GapRuleListComponent::Repeat {
                    count: 2,
                    values: vec![2, 3],
                },
            ],
            None,
            Vec::new(),
        );

        assert_eq!(list.values_for_count(7), [1, 2, 3, 2, 3, 1, 2]);
    }
}
