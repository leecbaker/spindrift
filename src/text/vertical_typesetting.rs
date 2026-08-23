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

    /// Returns the resolved typographic unit that fully contains `range`.
    ///
    /// CSS Writing Modes resolves vertical orientation per typographic
    /// character unit. A shaped glyph can therefore be assigned to a unit
    /// only when its source provenance is wholly contained by that unit.
    ///
    /// <https://www.w3.org/TR/css-writing-modes-4/#vertical-orientations>
    pub(crate) fn resolved_unit_for_range(
        &self,
        range: &Range<usize>,
    ) -> Option<&ResolvedVerticalUnit> {
        self.units()
            .iter()
            .find(|unit| unit.range.start <= range.start && range.end <= unit.range.end)
    }

    pub(crate) fn typesetting_for_range(
        &self,
        range: &Range<usize>,
    ) -> Option<VerticalUnitTypesetting> {
        self.resolved_unit_for_range(range)
            .map(|unit| unit.typesetting)
    }

    /// Return one mode when `range` is fully covered by resolved units that
    /// all agree on their vertical typesetting.
    ///
    /// A shaping cluster can cover several CSS typographic character units.
    /// The cluster has no single unit identity in that case, but its font
    /// metrics and paint matrix are still unambiguous when each covered unit
    /// resolves to the same mode. A range crossing upright and sideways units
    /// deliberately remains unresolved.
    /// <https://www.w3.org/TR/css-writing-modes-4/#vertical-orientations>
    pub(crate) fn unanimous_typesetting_for_range(
        &self,
        range: &Range<usize>,
    ) -> Option<VerticalUnitTypesetting> {
        if let Some(typesetting) = self.typesetting_for_range(range) {
            return Some(typesetting);
        }
        if range.start >= range.end {
            return None;
        }
        let mut covered_until = range.start;
        let mut typesetting = None;
        for unit in self.units() {
            if unit.range.end <= range.start {
                continue;
            }
            if range.end <= unit.range.start {
                break;
            }
            let overlap_start = unit.range.start.max(range.start);
            let overlap_end = unit.range.end.min(range.end);
            if overlap_start != covered_until {
                return None;
            }
            match typesetting {
                Some(previous) if previous != unit.typesetting => return None,
                Some(_) => {}
                None => typesetting = Some(unit.typesetting),
            }
            covered_until = overlap_end;
        }
        (covered_until == range.end)
            .then_some(typesetting)
            .flatten()
    }

    /// Returns the common typesetting mode when every resolved unit agrees.
    ///
    /// This is a fallback for a rendered run with no source provenance.
    /// PDF-facing Unicode summaries cannot safely reconstruct CSS
    /// typographic character-unit boundaries.
    pub(crate) fn uniform_typesetting(&self) -> Option<VerticalUnitTypesetting> {
        let first = self.units().first()?.typesetting;
        self.units()
            .iter()
            .all(|unit| unit.typesetting == first)
            .then_some(first)
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
                    let cuts_unit = unit.range.start < range.start || range.end < unit.range.end;
                    if cuts_unit
                        && !matches!(
                            unit.typesetting,
                            VerticalUnitTypesetting::SidewaysHorizontal
                        )
                    {
                        return None;
                    }
                    selected.push(ResolvedVerticalUnit {
                        // Native vertical cursive shaping may establish one
                        // sideways-horizontal unit across several authored
                        // inline fragments. A source slice must retain that
                        // unit's already-resolved mode for its selected
                        // intersection rather than re-shaping without its
                        // synthetic context.
                        range: unit.range.start.max(range.start) - range.start
                            ..unit.range.end.min(range.end) - range.start,
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
    fn unanimous_typesetting_handles_multi_unit_clusters_without_crossing_modes() {
        let upright = vertical_style(TextOrientation::Upright);
        let upright_plan = TextTypesettingPlan::resolve("AB", &upright);
        assert_eq!(
            upright_plan.unanimous_typesetting_for_range(&(0..2)),
            Some(VerticalUnitTypesetting::UprightVertical)
        );

        let mixed = vertical_style(TextOrientation::Mixed);
        let mixed_plan = TextTypesettingPlan::resolve("a、", &mixed);
        assert_eq!(
            mixed_plan.unanimous_typesetting_for_range(&(0..4)),
            None,
            "a sideways and an upright ideographic comma are ambiguous together"
        );
    }

    #[test]
    fn source_slice_projects_a_contextual_native_vertical_unit() {
        let text = "ᠨᠨᠨ";
        let plan = TextTypesettingPlan::resolve(text, &vertical_style(TextOrientation::Upright));
        let slice = plan
            .source_slice('ᠨ'.len_utf8()..'ᠨ'.len_utf8() * 2)
            .expect("a source fragment may reuse its contextual vertical mode");

        assert_eq!(
            slice.units(),
            [ResolvedVerticalUnit {
                range: 0..'ᠨ'.len_utf8(),
                typesetting: VerticalUnitTypesetting::SidewaysHorizontal,
            }]
        );
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
