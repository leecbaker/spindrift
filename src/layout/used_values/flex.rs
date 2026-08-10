use super::*;
/// Resolves a computed gap for flex layout.
///
/// CSS Box Alignment defines `normal` gaps as zero for flex containers and
/// resolves percentage gaps against the corresponding content box dimension:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout) fn used_flex_gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> LayoutLength {
    used_flex_gap_with_basis(value, percentage_basis)
}

/// Resolves a flex gap against a definite or indefinite percentage basis.
///
/// CSS Box Alignment treats the percentage component of a cyclic gap as zero
/// when the relevant flex axis is indefinite, while preserving any
/// non-percentage length component:
/// <https://www.w3.org/TR/css-align-3/#gap-percent>.
pub(in crate::layout) fn used_flex_gap_with_basis<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => percentage_basis
            .points()
            .map(|basis| {
                used_length_percentage(
                    value.clone(),
                    PercentageBasis::definite(layout_pt(basis.max(0.0))),
                )
            })
            .unwrap_or_else(|| value.length_max_zero()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn gap_resolvers_preserve_typed_fixed_components_when_basis_is_indefinite() {
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(4.0), 0.5, true);
        let gap = css::ComputedGap::LengthPercentage(mixed);

        let flex_gap: LayoutLength = used_flex_gap(
            gap.clone(),
            PercentageBasis::<ContentBoxLength>::indefinite(),
        );
        let multicol_gap: LayoutLength =
            used_multicol_column_gap(gap, PercentageBasis::<ContentBoxLength>::indefinite(), 16.0);

        assert_eq!(flex_gap.points(), 4.0);
        assert_eq!(multicol_gap.points(), 4.0);
    }
}
