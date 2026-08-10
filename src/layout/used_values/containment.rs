use super::*;
/// Whether CSS containment suppresses intrinsic contributions on a box's
/// logical inline axis.
///
/// `size` contains both axes; `inline-size` contains this axis only.
/// <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
pub(in crate::layout) fn intrinsic_inline_size_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size || style.contain.inline_size
}

/// Whether containment suppresses this box's physical-width contribution.
///
/// Physical `width` is logical inline size only in horizontal writing. In an
/// orthogonal flow it is logical block size, which `inline-size` containment
/// must leave available to ancestors.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
pub(in crate::layout) fn intrinsic_physical_width_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size
        || (style.contain.inline_size && style.writing_mode == WritingMode::HorizontalTb)
}

/// Whether containment suppresses this box's physical-height contribution.
///
/// This is the counterpart to [`intrinsic_physical_width_is_contained`]. An
/// inline-size-contained vertical writing-mode box has its physical height on
/// its logical inline axis, while its physical width remains a block-axis
/// contribution.
/// <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
pub(in crate::layout) fn intrinsic_physical_height_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size
        || (style.contain.inline_size && style.writing_mode != WritingMode::HorizontalTb)
}

/// Return the authored intrinsic fallback on the logical inline axis.
///
/// The computed `contain-intrinsic-size` longhands are physical, while
/// `inline-size` containment is logical, so the selected component follows
/// the element writing mode at this boundary.
pub(in crate::layout) fn contained_intrinsic_logical_inline_size(
    style: &ComputedStyle,
) -> Option<css::ComputedLengthPercentage> {
    match style.writing_mode {
        WritingMode::HorizontalTb => style.contain_intrinsic_size.width.clone(),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => style.contain_intrinsic_size.height.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn inline_size_containment_is_logical_for_physical_width_contributions() {
        let mut horizontal = ComputedStyle::initial();
        horizontal.contain.inline_size = true;
        assert!(intrinsic_inline_size_is_contained(&horizontal));
        assert!(intrinsic_physical_width_is_contained(&horizontal));

        let mut vertical = horizontal;
        vertical.writing_mode = WritingMode::VerticalRl;
        assert!(intrinsic_inline_size_is_contained(&vertical));
        assert!(!intrinsic_physical_width_is_contained(&vertical));
        assert!(intrinsic_physical_height_is_contained(&vertical));
    }
}
