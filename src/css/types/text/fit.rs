/// CSS Text Level 5's `text-fit` direction.
///
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextFitDirection {
    Grow,
    Shrink,
}

/// The relationship between the scaling factors selected for a block's
/// formatted lines.
///
/// `PerLine` and `PerLineAll` are retained in the computed value even while
/// the first used-value implementation only enables `Consistent`.
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextFitStrategy {
    Consistent,
    PerLine,
    PerLineAll,
}

/// Computed CSS `text-fit` value.
///
/// The optional percentage is a scale limit expressed as a factor: `75%` is
/// stored as `0.75`. It is only a limit in the direction specified by
/// [`TextFitDirection`].
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextFit {
    None,
    Fit {
        direction: TextFitDirection,
        strategy: TextFitStrategy,
        limit: Option<f32>,
    },
}

impl TextFit {
    pub(crate) const NONE: Self = Self::None;

    /// Parse the complete unordered `text-fit` grammar without accepting a
    /// partial declaration.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let parts = crate::css::component_values::try_split_css_component_values(value)?;
        if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
            return Some(Self::None);
        }

        let mut direction = None;
        let mut strategy = None;
        let mut limit = None;
        for part in parts {
            let lower = part.to_ascii_lowercase();
            match lower.as_str() {
                "grow" if direction.is_none() => direction = Some(TextFitDirection::Grow),
                "shrink" if direction.is_none() => direction = Some(TextFitDirection::Shrink),
                "consistent" if strategy.is_none() => strategy = Some(TextFitStrategy::Consistent),
                "per-line" if strategy.is_none() => strategy = Some(TextFitStrategy::PerLine),
                "per-line-all" if strategy.is_none() => {
                    strategy = Some(TextFitStrategy::PerLineAll)
                }
                _ if limit.is_none() => {
                    let percentage = crate::css::values::parse_percentage(part)?;
                    if !percentage.is_finite() {
                        return None;
                    }
                    limit = Some(percentage);
                }
                _ => return None,
            }
        }
        Some(Self::Fit {
            direction: direction?,
            strategy: strategy.unwrap_or(TextFitStrategy::Consistent),
            limit,
        })
    }
}
