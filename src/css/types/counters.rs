/// A CSS counter's implementation-limited integer value.
///
/// CSS Lists permits implementations to impose finite limits on counter
/// values, but requires values outside those limits to be clamped rather than
/// allowed to overflow:
/// <https://drafts.csswg.org/css-lists-3/#counter-properties>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CounterValue(i32);

impl CounterValue {
    // A symmetric range keeps negation well-defined for reversed-counter
    // planning while comfortably exceeding the values required by CSS Lists.
    pub(crate) const MIN: i32 = -2_100_000_000;
    pub(crate) const MAX: i32 = 2_100_000_000;
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) fn new(value: i32) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    pub(crate) const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn add(self, other: Self) -> Self {
        let sum = self.0 as i64 + other.0 as i64;
        Self(sum.clamp(Self::MIN as i64, Self::MAX as i64) as i32)
    }

    pub(crate) fn negated(self) -> Self {
        Self::new(-self.0)
    }
}

impl From<i32> for CounterValue {
    fn from(value: i32) -> Self {
        Self::new(value)
    }
}

/// The direction and initial value specified by one `counter-reset` entry.
///
/// Only reversed counters may omit an initial value. Their missing value is
/// resolved from the counter's scope at layout time:
/// <https://drafts.csswg.org/css-lists-3/#instantiating-counters>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterResetKind {
    Forward(CounterValue),
    Reversed(Option<CounterValue>),
}

impl CounterResetKind {
    pub(crate) const fn is_reversed(self) -> bool {
        matches!(self, Self::Reversed(_))
    }

    pub(crate) const fn explicit_value(self) -> Option<CounterValue> {
        match self {
            Self::Forward(value) | Self::Reversed(Some(value)) => Some(value),
            Self::Reversed(None) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterReset {
    pub(crate) name: String,
    pub(crate) kind: CounterResetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterChange {
    pub(crate) name: String,
    pub(crate) value: CounterValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_values_clamp_construction_and_arithmetic() {
        assert_eq!(CounterValue::new(i32::MAX).get(), CounterValue::MAX);
        assert_eq!(CounterValue::new(i32::MIN).get(), CounterValue::MIN);
        assert_eq!(
            CounterValue::new(CounterValue::MAX)
                .add(CounterValue::new(1))
                .get(),
            CounterValue::MAX
        );
        assert_eq!(
            CounterValue::new(CounterValue::MIN)
                .add(CounterValue::new(-1))
                .get(),
            CounterValue::MIN
        );
    }

    #[test]
    fn counter_value_negation_stays_in_the_supported_range() {
        assert_eq!(
            CounterValue::new(CounterValue::MIN).negated().get(),
            CounterValue::MAX
        );
        assert_eq!(
            CounterValue::new(CounterValue::MAX).negated().get(),
            CounterValue::MIN
        );
    }
}
