use super::*;
use crate::units::IntoLayoutLength;

/// A replaced flex item's physical content-box dimensions before aspect-ratio
/// constraint resolution. Keeping the axes together prevents a width value
/// being re-used as the height half of a later constraint pass.
#[derive(Debug, Clone, Copy)]
struct FlexReplacedContentSize {
    width: ContentBoxLength,
    height: ContentBoxLength,
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn replaced_flex_estimate_resolves_percentage_max_height_from_block_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );
        let available = FlexItemAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(200.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(200.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            stretched_width: None,
            stretched_height: None,
        };
        let estimate = estimate_replaced_flex_item(
            IntrinsicReplacedSize {
                width: content_box_pt(200.0),
                height: content_box_pt(200.0),
                preferred_aspect_ratio: Some(1.0),
                has_intrinsic_size: true,
                attr_width: None,
                attr_height: None,
            },
            &style,
            false,
            PhysicalContentWidth::new(content_box_pt(200.0)),
            available,
        )
        .expect("a square intrinsic image is estimable");

        assert_eq!(estimate.width.points(), 100.0);
        assert_eq!(estimate.height.points(), 100.0);
    }

    #[test]
    fn ratio_only_svg_keeps_default_object_size_separate_from_flex_stretch() {
        let style = ComputedStyle::initial();
        let available = FlexItemAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(600.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(600.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
            stretched_width: Some(PhysicalContentWidth::new(content_box_pt(600.0))),
            stretched_height: None,
        };
        let estimate = estimate_replaced_flex_item(
            IntrinsicReplacedSize {
                width: content_box_pt(225.0),
                height: content_box_pt(112.5),
                preferred_aspect_ratio: Some(2.0),
                has_intrinsic_size: false,
                attr_width: None,
                attr_height: None,
            },
            &style,
            false,
            PhysicalContentWidth::new(content_box_pt(600.0)),
            available,
        )
        .expect("a ratio-only SVG is estimable");

        assert_eq!(estimate.width.points(), 225.0);
        assert_eq!(estimate.height.points(), 112.5);
        let automatic = estimate
            .automatic_preferred_physical_size
            .expect("the CSS default object size is retained");
        assert_eq!(automatic.width.points(), 225.0);
        assert_eq!(automatic.height.points(), 112.5);
    }
}

impl FlexReplacedContentSize {
    fn new(width: ContentBoxLength, height: ContentBoxLength) -> Self {
        Self { width, height }
    }

    fn zero() -> Self {
        Self::new(content_box_pt(0.0), content_box_pt(0.0))
    }

    fn constrain_with_aspect_ratio(
        &mut self,
        aspect_ratio: f32,
        preferred_sizes: ReplacedPreferredSizeAxes,
        constraints: ReplacedSizeConstraints,
    ) {
        let constrained = resolve_replaced_size_with_aspect_ratio(
            content_box_size_pt(self.width.points(), self.height.points()),
            aspect_ratio,
            preferred_sizes,
            constraints,
        );
        *self = Self::new(
            content_box_pt(constrained.width),
            content_box_pt(constrained.height),
        );
    }

    fn width_at_ratio(width: ContentBoxLength, ratio: f32) -> Self {
        Self::new(width, content_box_pt(width.points() / ratio))
    }

    fn height_at_ratio(height: ContentBoxLength, ratio: f32) -> Self {
        Self::new(content_box_pt(height.points() * ratio), height)
    }
}

/// Estimates a replaced flex item without letting main-size constraints alter flex basis.
///
/// CSS Flexbox computes the flex base size from the item's used flex-basis
/// while ignoring min/max main-size constraints, but the hypothetical size and
/// cross-size contribution still reflect replaced-element aspect-ratio sizing.
/// For replaced elements with an intrinsic ratio, cross-axis min/max constraints
/// transfer through the ratio into the content-basis candidate used by
/// `flex-basis:auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>, and
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>.
pub(in crate::layout::flex) fn estimate_replaced_flex_item(
    intrinsic: IntrinsicReplacedSize,
    style: &ComputedStyle,
    has_size_containment: bool,
    containing_width: PhysicalContentWidth,
    available: FlexItemAvailableSpace,
) -> Option<FlexItemEstimate> {
    let attribute_aspect_ratio = intrinsic.attribute_aspect_ratio();
    let aspect_ratio = style.aspect_ratio.preferred_ratio(
        true,
        if has_size_containment {
            attribute_aspect_ratio
        } else {
            intrinsic.natural_aspect_ratio()
        },
    );
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    // A flex item's percentage block-size constraints resolve against the
    // containing block's block-size basis, which is independent from its
    // physical inline measurement width. In particular, a row flex item can
    // have a definite stretched height while its width is larger.
    // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    let block_constraints = ReplacedSizeConstraints {
        min_width: used_min_width(
            style,
            PercentageBasis::definite(containing_width.content_box_length()),
        )
        .map(|width| width.max(content_box_pt(0.0))),
        max_width: used_max_width(
            style,
            PercentageBasis::definite(containing_width.content_box_length()),
        )
        .map(|width| width.max(content_box_pt(0.0))),
        min_height: used_length_percentage_or_auto_with_basis(
            style.box_values.min_height.clone(),
            available.height_basis,
        )
        .map(|height| content_box_pt(height.points().max(0.0))),
        max_height: used_length_percentage_or_auto_with_basis(
            style.box_values.max_height.clone(),
            available.height_basis,
        )
        .map(|height| content_box_pt(height.points().max(0.0))),
    };
    // A viewBox-only SVG has a preferred ratio but no intrinsic dimensions.
    // Its automatic preferred size remains CSS Images' default object size;
    // flex stretch must not masquerade as an authored definite width or
    // height while establishing that size.
    let ratio_only_automatic_size = !intrinsic.has_intrinsic_size
        && aspect_ratio.is_some()
        && intrinsic.attr_width.is_none()
        && intrinsic.attr_height.is_none()
        && style.box_values.width.is_auto()
        && style.box_values.height.is_auto();
    let specified_width = used_content_box_width_or_auto(
        style,
        containing_width.content_box_length().into_layout_length(),
        non_content_pt(horizontal_non_content),
    )
    .or(intrinsic.attr_width)
    .or_else(|| {
        (!ratio_only_automatic_size)
            .then(|| {
                available
                    .stretched_width
                    .map(|width| content_box_pt((width.points() - horizontal_non_content).max(0.0)))
            })
            .flatten()
    });
    let specified_height = used_content_box_height_or_auto_with_basis(
        style,
        available.height_basis,
        non_content_pt(vertical_non_content),
    )
    .or(intrinsic.attr_height)
    .or_else(|| {
        (!ratio_only_automatic_size)
            .then(|| {
                available
                    .stretched_height
                    .map(|height| content_box_pt((height.points() - vertical_non_content).max(0.0)))
            })
            .flatten()
    });
    let width_is_auto = specified_width.is_none();
    let height_is_auto = specified_height.is_none();
    let contained_intrinsic_width = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.width.clone().map(|width| {
                used_length_percentage(
                    width,
                    PercentageBasis::definite(containing_width.content_box_length()),
                )
                .cast_unit()
            })
        })
        .flatten()
        .unwrap_or_else(|| content_box_pt(0.0));
    let contained_intrinsic_height = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.height.clone().map(|height| {
                used_length_percentage(
                    height,
                    PercentageBasis::definite(containing_width.content_box_length()),
                )
                .cast_unit()
            })
        })
        .flatten()
        .unwrap_or_else(|| content_box_pt(0.0));
    let base_size = match (specified_width, specified_height, aspect_ratio) {
        (Some(width), None, Some(ratio)) => FlexReplacedContentSize::width_at_ratio(width, ratio),
        (None, Some(height), Some(ratio)) => {
            FlexReplacedContentSize::height_at_ratio(height, ratio)
        }
        // An SVG image with only a `viewBox` provides a preferred aspect
        // ratio but no intrinsic dimensions. CSS Images still supplies its
        // default object size as the automatic preferred physical size.
        // Preserve that size rather than replacing it with the flex
        // container's available cross space; the automatic-minimum phase
        // later transfers the applicable physical axis through the ratio.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
        (None, None, Some(ratio)) if !intrinsic.has_intrinsic_size => {
            let width = intrinsic.width.max(content_box_pt(0.0));
            let height = intrinsic.height.max(content_box_pt(0.0));
            if width > content_box_pt(0.0) {
                FlexReplacedContentSize::width_at_ratio(width, ratio)
            } else {
                FlexReplacedContentSize::height_at_ratio(height, ratio)
            }
        }
        // Size containment's fallback is an intrinsic size, not a preferred
        // aspect ratio.  When an authored dimension leaves the other axis
        // auto, retain that axis's fallback intrinsic dimension rather than
        // scaling it from the specified one.
        // <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>
        (Some(width), None, None) if has_size_containment => {
            FlexReplacedContentSize::new(width, contained_intrinsic_height)
        }
        (None, Some(height), None) if has_size_containment => {
            FlexReplacedContentSize::new(contained_intrinsic_width, height)
        }
        (Some(width), None, None) => FlexReplacedContentSize::new(width, content_box_pt(0.0)),
        (None, Some(height), None) => FlexReplacedContentSize::new(content_box_pt(0.0), height),
        (None, None, _) if has_size_containment => {
            FlexReplacedContentSize::new(contained_intrinsic_width, contained_intrinsic_height)
        }
        (None, None, _) => FlexReplacedContentSize::new(intrinsic.width, intrinsic.height),
        (Some(width), Some(height), _) => FlexReplacedContentSize::new(width, height),
    };
    let Some(aspect_ratio) = aspect_ratio else {
        let width = constrain_content_width(
            style,
            base_size.width,
            PercentageBasis::definite(content_box_pt(containing_width.points().max(1.0))),
        );
        let height = super::sizing::constrain_flex_item_estimated_height(
            style,
            base_size.height,
            base_size.height,
            base_size.height,
            available.height_basis,
            non_content_pt(vertical_non_content),
        )
        .max(content_box_pt(1.0));
        return Some(FlexItemEstimate::from_physical_intrinsic_metrics(
            FlexPhysicalIntrinsicMetrics {
                width: PhysicalContentWidth::new(width),
                height: PhysicalContentHeight::new(height),
                min_width: PhysicalContentWidth::new(width),
                min_height: PhysicalContentHeight::new(height),
                content_width: PhysicalContentWidth::new(width),
                content_height: PhysicalContentHeight::new(height),
            },
            None,
            FlexItemBaselineEstimate::default(),
        ));
    };
    let mut constrained_size = base_size;
    constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedPreferredSizeAxes {
            width: ReplacedPreferredSize::from_is_automatic(width_is_auto),
            height: ReplacedPreferredSize::from_is_automatic(height_is_auto),
        },
        block_constraints,
    );

    let mut width_constrained_size = base_size;
    width_constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedPreferredSizeAxes {
            width: ReplacedPreferredSize::from_is_automatic(width_is_auto),
            height: ReplacedPreferredSize::from_is_automatic(height_is_auto),
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(
                style,
                PercentageBasis::definite(containing_width.content_box_length()),
            )
            .map(|width| width.max(content_box_pt(0.0))),
            max_width: used_max_width(
                style,
                PercentageBasis::definite(containing_width.content_box_length()),
            )
            .map(|width| width.max(content_box_pt(0.0))),
            min_height: None,
            max_height: None,
        },
    );

    let mut height_constrained_size = base_size;
    height_constrained_size.constrain_with_aspect_ratio(
        aspect_ratio,
        ReplacedPreferredSizeAxes {
            width: ReplacedPreferredSize::from_is_automatic(width_is_auto),
            height: ReplacedPreferredSize::from_is_automatic(height_is_auto),
        },
        ReplacedSizeConstraints {
            min_width: None,
            max_width: None,
            min_height: block_constraints.min_height,
            max_height: block_constraints.max_height,
        },
    );

    // An SVG with only a preferred ratio has no intrinsic content-size
    // suggestion for Flexbox's automatic minimum. It still uses the default
    // object size to establish an auto flex base size, but treating that
    // fallback as min-content would prevent normal flex shrinking and make a
    // ratio-only SVG overflow a definite flex container.
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
    // <https://www.w3.org/TR/css-images-3/#default-sizing>
    let min_size = if intrinsic.has_intrinsic_size {
        FlexReplacedContentSize::new(
            constrained_size.width.max(content_box_pt(1.0)),
            constrained_size.height.max(content_box_pt(1.0)),
        )
    } else {
        FlexReplacedContentSize::zero()
    };
    let mut estimate = FlexItemEstimate::from_physical_intrinsic_metrics(
        FlexPhysicalIntrinsicMetrics {
            width: PhysicalContentWidth::new(constrained_size.width.max(content_box_pt(1.0))),
            height: PhysicalContentHeight::new(constrained_size.height.max(content_box_pt(1.0))),
            min_width: PhysicalContentWidth::new(min_size.width),
            min_height: PhysicalContentHeight::new(min_size.height),
            content_width: PhysicalContentWidth::new(
                height_constrained_size.width.max(content_box_pt(1.0)),
            ),
            content_height: PhysicalContentHeight::new(
                width_constrained_size.height.max(content_box_pt(1.0)),
            ),
        },
        Some(aspect_ratio),
        FlexItemBaselineEstimate::default(),
    );
    if ratio_only_automatic_size {
        estimate.set_automatic_preferred_physical_size(FlexAutomaticPreferredPhysicalSize {
            width: PhysicalContentWidth::new(base_size.width.max(content_box_pt(0.0))),
            height: PhysicalContentHeight::new(base_size.height.max(content_box_pt(0.0))),
        });
    }
    Some(estimate)
}
