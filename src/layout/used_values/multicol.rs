use super::*;
/// Resolves a computed column gap for multi-column layout.
///
/// CSS Multi-column Layout defines `column-gap: normal` as `1em`; CSS Box
/// Alignment supplies the shared length-percentage gap syntax:
/// <https://www.w3.org/TR/css-multicol-1/#cgap> and
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout) fn used_multicol_column_gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    font_size: f32,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(font_size.max(0.0)),
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

/// Resolves the number of columns for the current multi-column formatting context.
///
/// CSS Multi-column Layout derives the used column count from `column-count`,
/// `column-width`, the available inline size, and the used column gap:
/// <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>.
pub(in crate::layout) fn used_multicol_column_count(
    style: &ComputedStyle,
    available_width: f32,
    gap: f32,
) -> Option<usize> {
    let specified_count = match style.column_count {
        css::ColumnCount::Auto => None,
        css::ColumnCount::Count(count) => Some(count.get()),
    };
    let specified_width = match &style.column_width {
        css::ComputedColumnWidth::Auto => None,
        css::ComputedColumnWidth::Length(width) => {
            width.length_if_no_percent().filter(|width| *width > 0.0)
        }
    };
    match (specified_count, specified_width) {
        (None, None) if matches!(style.column_height, css::ComputedColumnHeight::Length(_)) => {
            Some(1)
        }
        (None, None) => None,
        (Some(count), None) => Some(count),
        (count, Some(width)) => {
            let fitting_count = ((available_width + gap) / (width + gap)).floor().max(1.0) as usize;
            Some(count.map_or(fitting_count, |count| count.min(fitting_count)))
        }
    }
}

/// Return the intrinsic inline sizes contributed by a size-contained multicol.
///
/// Size containment ignores the contents of the principal box, but it does
/// not erase the multicol formatting context's authored column widths and
/// gaps. With no content contribution, a definite `column-width` and maximum
/// `column-count` form both intrinsic inline sizes. An automatic column width
/// contributes zero per column, but gaps between an authored number of
/// columns remain part of the formatting context's intrinsic geometry.
/// <https://www.w3.org/TR/css-contain-1/#containment-size>
/// <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>
pub(in crate::layout) fn size_contained_multicol_intrinsic_inline_sizes(
    style: &ComputedStyle,
) -> Option<(f32, f32)> {
    if !intrinsic_inline_size_is_contained(style) {
        return None;
    }
    let column_width = match &style.column_width {
        css::ComputedColumnWidth::Auto => 0.0,
        css::ComputedColumnWidth::Length(column_width) => column_width
            .length_if_no_percent()
            .filter(|width| *width > 0.0)
            .unwrap_or(0.0),
    };
    let count = match style.column_count {
        css::ColumnCount::Auto => 1,
        css::ColumnCount::Count(count) => count.get(),
    };
    let gap = used_multicol_column_gap(
        style.column_gap.clone(),
        PercentageBasis::definite(content_box_pt(0.0)),
        style.font_size,
    )
    .points();
    let inline_size = column_width * count as f32 + gap * count.saturating_sub(1) as f32;
    Some((inline_size, inline_size))
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn multicol_count_can_derive_from_computed_column_width() {
        let mut style = ComputedStyle::initial();
        style.column_width =
            css::ComputedColumnWidth::Length(css::ComputedLengthPercentage::from_points(40.0));

        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(3));

        style.column_count = css::ColumnCount::Count(std::num::NonZeroUsize::new(2).unwrap());
        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(2));
        assert_eq!(used_multicol_column_count(&style, 40.0, 10.0), Some(1));

        style.column_width = css::ComputedColumnWidth::Auto;
        assert_eq!(used_multicol_column_count(&style, 1.0, 10.0), Some(2));
    }

    #[test]
    fn size_containment_preserves_authored_multicol_intrinsic_width() {
        let mut style = ComputedStyle::initial();
        style.contain.size = true;
        style.column_count = css::ColumnCount::Count(std::num::NonZeroUsize::new(3).unwrap());
        style.column_width =
            css::ComputedColumnWidth::Length(css::ComputedLengthPercentage::from_points(20.0));
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            size_contained_multicol_intrinsic_inline_sizes(&style),
            Some((70.0, 70.0))
        );
    }

    #[test]
    fn size_contained_auto_width_multicol_preserves_authored_gaps() {
        let mut style = ComputedStyle::initial();
        style.contain.size = true;
        style.column_count = css::ColumnCount::Count(std::num::NonZeroUsize::new(3).unwrap());
        style.column_width = css::ComputedColumnWidth::Auto;
        style.font_size = 12.0;
        style.column_gap = css::ComputedGap::Normal;

        assert_eq!(
            size_contained_multicol_intrinsic_inline_sizes(&style),
            Some((24.0, 24.0))
        );
    }
}
