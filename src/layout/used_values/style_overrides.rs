use super::*;
/// Resolves the physical `left` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_left(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_left.clone(),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `right` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_right(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_right.clone(),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `top` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_top(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_top.clone(),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `bottom` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_bottom(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_bottom.clone(),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .map(SemanticLengthExt::points)
}

/// Replaces computed width with a definite used width for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_width(style: &mut ComputedStyle, width: f32) {
    let width = width.max(0.0);
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(width),
    );
}

/// Replaces computed height with a definite used height for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_height(style: &mut ComputedStyle, height: f32) {
    let height = height.max(0.0);
    style.box_values.height.replace_with_used(
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(height),
        ),
    );
}

/// Freeze a temporary content-box replay style to a resolved content width.
///
/// This adapter is paired with an explicit border-to-content conversion at
/// the flex replay boundary; the temporary style therefore remains in
/// `box-sizing: content-box` space throughout normal-flow reconstruction.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items>
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>
pub(in crate::layout) fn set_style_used_content_box_width_bounds(
    style: &mut ComputedStyle,
    width: ContentBoxLength,
) {
    let width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(width.points().max(0.0)),
    );
    style.box_values.min_width = width.clone();
    style.box_values.max_width = width;
}

/// See [`set_style_used_content_box_width_bounds`].
pub(in crate::layout) fn set_style_used_content_box_height_bounds(
    style: &mut ComputedStyle,
    height: ContentBoxLength,
) {
    let height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(height.points().max(0.0)),
    );
    style.box_values.min_height = height.clone();
    style.box_values.max_height = height;
}

/// Restores `width: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial width value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-width-property>.
pub(in crate::layout) fn set_style_auto_width(style: &mut ComputedStyle) {
    style.box_values.width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Restores `height: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial height value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
pub(in crate::layout) fn set_style_auto_height(style: &mut ComputedStyle) {
    style
        .box_values
        .height
        .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
}

/// Clears physical positioned offsets on a temporary layout style.
///
/// CSS 2.2 defines physical inset properties for positioned boxes:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn clear_style_insets(style: &mut ComputedStyle) {
    style.box_values.inset_left = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_top = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_right = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_bottom = css::ComputedLengthPercentageOrAuto::Auto;
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn length(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(points),
        )
    }
    #[test]
    fn mutating_used_width_replaces_typed_percentage_with_used_length() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );

        set_style_used_width(&mut style, 42.0);

        assert_eq!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(42.0)
            )
        );
    }

    #[test]
    fn substituting_a_used_or_auto_height_clears_deferred_font_metric_provenance() {
        let mut style = ComputedStyle::initial();
        style.box_values.height = css::PhysicalHeight::DeferredFontMetric(length(12.0));

        set_style_used_height(&mut style, 20.0);
        assert!(!style.box_values.height.is_deferred_font_metric());
        assert_eq!(
            style
                .box_values
                .height
                .length_if_no_percent()
                .expect("used height is a definite length"),
            20.0
        );

        style.box_values.height = css::PhysicalHeight::DeferredFontMetric(length(12.0));
        set_style_auto_height(&mut style);
        assert!(!style.box_values.height.is_deferred_font_metric());
        assert!(style.box_values.height.is_auto());
    }
}
