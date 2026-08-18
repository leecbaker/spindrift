//! Resolved per-unit typography for CSS vertical lines.
//!
//! CSS `text-orientation` is an authored policy, not a shaping instruction.
//! Resolve it once into a complete unit mode before shaping so that vertical
//! font features, vertical metrics, and PDF placement cannot disagree.

use super::*;

/// One complete shaping, metrics, and placement mode for a typographic unit.
///
/// The variants deliberately own all three decisions: code outside this
/// module cannot construct a sideways unit that requests vertical forms or
/// vertical advances.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerticalUnitTypesetting {
    /// Individual vertical units using vertical OpenType features and metrics.
    UprightVertical,
    /// A horizontally composed run rotated onto a vertical line.
    SidewaysHorizontal,
}

impl VerticalUnitTypesetting {
    pub(crate) const fn uses_vertical_font_metrics(self) -> bool {
        matches!(self, Self::UprightVertical)
    }
}

/// One source range and its fully resolved vertical typesetting mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedVerticalUnit {
    pub(crate) range: Range<usize>,
    pub(crate) typesetting: VerticalUnitTypesetting,
}

/// The resolved typography plan for one shaped text line.
///
/// Horizontal text has no per-unit vertical choices. Vertical plans cover the
/// entire source string with ordered CSS typographic units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextTypesettingPlan {
    Horizontal,
    Vertical(Rc<[ResolvedVerticalUnit]>),
}

impl TextTypesettingPlan {
    pub(crate) fn resolve(text: &str, style: &ComputedStyle) -> Self {
        match style.text_layout_policy() {
            TextLayoutPolicy::Horizontal => Self::Horizontal,
            TextLayoutPolicy::Sideways(_) => {
                Self::vertical_single(text, VerticalUnitTypesetting::SidewaysHorizontal)
            }
            TextLayoutPolicy::Vertical(orientation) => {
                let mut units = CursiveProtectedUnitRanges::new(text)
                    .into_iter()
                    .map(|range| {
                        let unit_text = &text[range.clone()];
                        ResolvedVerticalUnit {
                            typesetting: resolve_vertical_unit(orientation, unit_text),
                            range,
                        }
                    })
                    .collect::<Vec<_>>();
                // Default-ignorables inherit the following visible unit's
                // orientation; coalesce equal neighboring decisions so font
                // shaping receives one contiguous feature range.
                for index in (0..units.len()).rev() {
                    if text[units[index].range.clone()]
                        .chars()
                        .all(character_inherits_vertical_orientation)
                        && let Some(next) = units.get(index + 1)
                    {
                        units[index].typesetting = next.typesetting;
                    }
                }
                Self::Vertical(units.into())
            }
        }
    }

    fn vertical_single(text: &str, typesetting: VerticalUnitTypesetting) -> Self {
        if text.is_empty() {
            Self::Horizontal
        } else {
            Self::Vertical(
                vec![ResolvedVerticalUnit {
                    range: 0..text.len(),
                    typesetting,
                }]
                .into(),
            )
        }
    }

    pub(crate) fn units(&self) -> &[ResolvedVerticalUnit] {
        match self {
            Self::Horizontal => &[],
            Self::Vertical(units) => units,
        }
    }

    pub(crate) fn typesetting_for_range(
        &self,
        range: &Range<usize>,
    ) -> Option<VerticalUnitTypesetting> {
        self.units()
            .iter()
            .find(|unit| unit.range.start <= range.start && range.end <= unit.range.end)
            .map(|unit| unit.typesetting)
    }

    pub(crate) fn upright_vertical_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = self
            .units()
            .iter()
            .filter(|unit| unit.typesetting.uses_vertical_font_metrics())
            .map(|unit| unit.range.clone())
            .collect::<Vec<_>>();
        let mut coalesced = Vec::<Range<usize>>::with_capacity(ranges.len());
        for range in ranges.drain(..) {
            if let Some(previous) = coalesced.last_mut()
                && previous.end == range.start
            {
                previous.end = range.end;
            } else {
                coalesced.push(range);
            }
        }
        coalesced
    }

    pub(crate) fn source_slice(&self, range: Range<usize>) -> Option<Self> {
        match self {
            Self::Horizontal => Some(Self::Horizontal),
            Self::Vertical(units) => {
                let mut selected = Vec::new();
                for unit in units.iter() {
                    if unit.range.end <= range.start || range.end <= unit.range.start {
                        continue;
                    }
                    if unit.range.start < range.start || range.end < unit.range.end {
                        return None;
                    }
                    selected.push(ResolvedVerticalUnit {
                        range: unit.range.start - range.start..unit.range.end - range.start,
                        typesetting: unit.typesetting,
                    });
                }
                (!selected.is_empty())
                    .then(|| Self::Vertical(selected.into()))
                    .or(Some(Self::Horizontal))
            }
        }
    }
}

fn resolve_vertical_unit(orientation: TextOrientation, text: &str) -> VerticalUnitTypesetting {
    match orientation {
        TextOrientation::Sideways => VerticalUnitTypesetting::SidewaysHorizontal,
        TextOrientation::Mixed => match typographic_unit_mixed_orientation(text) {
            MixedTextOrientation::Upright => VerticalUnitTypesetting::UprightVertical,
            MixedTextOrientation::Sideways => VerticalUnitTypesetting::SidewaysHorizontal,
        },
        TextOrientation::Upright => {
            // Mongolian and Phags-pa are intrinsically vertical, but their
            // normal horizontal composition is counter-clockwise from that
            // intrinsic presentation. CSS's sideways-horizontal path supplies
            // the clockwise transform without applying vertical substitutions
            // or metrics; this is the behavior asserted by the WPTs.
            if text
                .chars()
                .find(|character| !character_inherits_vertical_orientation(*character))
                .is_some_and(character_is_native_vertical_script)
            {
                VerticalUnitTypesetting::SidewaysHorizontal
            } else {
                VerticalUnitTypesetting::UprightVertical
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::WritingMode;

    fn vertical_style(orientation: TextOrientation) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.text_orientation = orientation;
        style
    }

    fn modes(text: &str, orientation: TextOrientation) -> Vec<VerticalUnitTypesetting> {
        TextTypesettingPlan::resolve(text, &vertical_style(orientation))
            .units()
            .iter()
            .map(|unit| unit.typesetting)
            .collect()
    }

    #[test]
    fn mixed_resolves_u_tu_and_tr_upright_but_r_sideways() {
        assert_eq!(
            modes("a§、〈", TextOrientation::Mixed),
            [
                VerticalUnitTypesetting::SidewaysHorizontal,
                VerticalUnitTypesetting::UprightVertical,
                VerticalUnitTypesetting::UprightVertical,
                VerticalUnitTypesetting::UprightVertical,
            ]
        );
    }

    #[test]
    fn native_vertical_scripts_never_request_vertical_metrics() {
        for orientation in [TextOrientation::Mixed, TextOrientation::Upright] {
            for text in ["ᠮ", "ꡀ"] {
                assert_eq!(
                    modes(text, orientation),
                    [VerticalUnitTypesetting::SidewaysHorizontal],
                );
            }
        }
    }

    #[test]
    fn sideways_and_horizontal_plans_have_no_upright_ranges() {
        let sideways =
            TextTypesettingPlan::resolve("中文", &vertical_style(TextOrientation::Sideways));
        assert!(sideways.upright_vertical_ranges().is_empty());
        assert!(
            TextTypesettingPlan::resolve("中文", &ComputedStyle::initial())
                .upright_vertical_ranges()
                .is_empty()
        );
    }
}
