use super::*;
use crate::units::{
    IntoLayoutLength, content_box_to_margin_box_length, layout_to_content_box_length,
};

mod children;
mod compute;
mod estimate;
mod layout;
mod model;
mod taffy;

pub(in crate::layout) use children::flex_container_fragment_boundary_breaks;
use children::*;
use model::*;
use taffy::*;

/// The physical content-box width contributions of a flex formatting context.
///
/// This is intentionally a composite rather than a `(f32, f32)`: callers
/// query Flex from physical-width sizing algorithms, whereas the estimator
/// itself retains logical sizes until its writing-mode projection boundary.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct FlexIntrinsicWidthContributions {
    pub(in crate::layout) min_content: PhysicalContentWidth,
    pub(in crate::layout) max_content: PhysicalContentWidth,
}

impl FlexIntrinsicWidthContributions {
    fn new(min_content: PhysicalContentWidth, max_content: PhysicalContentWidth) -> Self {
        let min_content = min_content.non_negative();
        let max_content = max_content.non_negative().max(min_content);
        Self {
            min_content,
            max_content,
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    fn flex_container_height_percentage_basis(&self) -> BlockSizePercentageBasis {
        let stack_basis = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        if stack_basis.is_definite() {
            return stack_basis;
        }
        match self
            .current_child_available_space()
            .physical_height_percentage_basis()
        {
            PercentageBasis::Definite { value: height, .. } => PercentageBasis::definite_from(
                height.content_box_length(),
                BlockSizeBasisSource::ContainingBlock,
            ),
            PercentageBasis::Indefinite => PercentageBasis::indefinite(),
        }
    }

    /// Estimate the min-content and max-content inline widths of a flex container.
    ///
    /// CSS Flexbox defines a flex container's intrinsic main and cross sizes
    /// from its flex items' intrinsic contributions. These widths are used by
    /// parent formatting contexts such as CSS 2.2 shrink-to-fit sizing for
    /// floats and absolutely/fixed positioned boxes:
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
    pub(in crate::layout) fn estimate_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: PhysicalContentWidth,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> FlexIntrinsicWidthContributions {
        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            &built_child_boxes
        };
        let container_signature = self.flex_container_signature(element);
        let children = flex_children_from_boxes(element, &container_signature, style, child_boxes);
        let intrinsic = self.estimate_intrinsic_flex_container_size(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: available_width.non_negative(),
                width_basis: flex_available_percentage_basis(
                    used_content_box_width_or_auto(
                        style,
                        available_width.content_box_length().into_layout_length(),
                        non_content_pt(
                            style.padding.left
                                + style.padding.right
                                + horizontal_border_width(style),
                        ),
                    )
                    .map(|_| available_width.content_box_length()),
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                ),
                height: used_length_percentage_or_auto(
                    style.box_values.height.value().clone(),
                    PercentageBasis::definite(
                        available_width.content_box_length().into_layout_length(),
                    ),
                )
                .map(|height| {
                    PhysicalContentHeight::new(crate::units::layout_to_content_box_length(height))
                }),
                height_basis: flex_available_percentage_basis(
                    used_length_percentage_or_auto(
                        style.box_values.height.value().clone(),
                        PercentageBasis::definite(
                            available_width.content_box_length().into_layout_length(),
                        ),
                    )
                    .map(crate::units::layout_to_content_box_length),
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                ),
            },
        );
        FlexIntrinsicWidthContributions::new(
            PhysicalContentWidth::new(intrinsic.min_width),
            PhysicalContentWidth::new(intrinsic.width),
        )
    }

    /// Estimate a floated flex container's margin-box height after the float
    /// algorithm has resolved its used inline size.
    ///
    /// Float placement needs the flex container's line-based block size.  A
    /// generic block-child walk would instead add every row flex item's block
    /// size, turning one flex line into a vertical stack and producing too
    /// much clearance for following floats.
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    pub(in crate::layout) fn estimate_floated_flex_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        margin_box_width: MarginBoxLength,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> MarginBoxLength {
        let border_widths = used_border_widths(style);
        // Computed style still stores used edge values as CSS scalars. Turn
        // those values into a box-model quantity at this boundary so the Flex
        // estimator cannot accidentally mix them with content or margin boxes.
        let horizontal_non_content = non_content_pt(
            border_widths.left + border_widths.right + style.padding.left + style.padding.right,
        );
        let vertical_non_content = non_content_pt(
            border_widths.top + border_widths.bottom + style.padding.top + style.padding.bottom,
        );
        let available_content_width = PhysicalContentWidth::new(layout_to_content_box_length(
            margin_box_width.into_layout_length() - horizontal_non_content.into_layout_length(),
        ))
        .non_negative()
        .content_box_length();
        let content_width = used_content_box_width_or_auto(
            style,
            margin_box_width.into_layout_length(),
            horizontal_non_content,
        )
        .unwrap_or(available_content_width);
        let height_basis = self.flex_container_height_percentage_basis();
        let explicit_content_height =
            used_content_box_height_or_auto_with_basis(style, height_basis, vertical_non_content);
        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            &built_child_boxes
        };
        let container_signature = self.flex_container_signature(element);
        let children = flex_children_from_boxes(element, &container_signature, style, child_boxes);
        let intrinsic = self.estimate_intrinsic_flex_container_size(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: PhysicalContentWidth::new(content_width).non_negative(),
                width_basis: flex_available_percentage_basis(
                    Some(content_width),
                    FlexAvailableSizeSource::ContainingBlock,
                ),
                height: explicit_content_height.map(PhysicalContentHeight::new),
                height_basis: flex_available_percentage_basis(
                    explicit_content_height,
                    FlexAvailableSizeSource::ContainingBlock,
                ),
            },
        );
        let content_height = explicit_content_height.unwrap_or(intrinsic.height);
        let percentage_height_basis = height_basis.value().unwrap_or(content_width);
        content_box_to_margin_box_length(
            constrain_content_height(
                style,
                content_height,
                PercentageBasis::definite(percentage_height_basis),
            ),
            vertical_non_content,
            // CSS used margins are signed. Keep their scalar addition at the
            // computed-style boundary, then carry the result as a typed layout
            // displacement through the explicit content-to-margin conversion.
            layout_pt(style.margin.top + style.margin.bottom),
        )
    }

    fn flex_container_signature(&self, element: &Element) -> ElementSignature {
        self.ancestors
            .last()
            .cloned()
            .unwrap_or_else(|| element_signature(element))
    }

    /// Scope a definite flex item height for percentage-height descendants.
    ///
    /// CSS Flexbox treats stretched cross sizes and post-flexing main sizes as
    /// definite for descendant layout, and CSS Sizing lets replaced elements
    /// transfer a resolved percentage height through their intrinsic aspect
    /// ratio:
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
    /// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>.
    fn with_flex_item_percentage_height_basis<R>(
        &mut self,
        basis: FlexPercentageBasis,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if !basis.is_definite() {
            return layout(self);
        };
        let Some(basis) = basis.value() else {
            return layout(self);
        };
        self.definite_block_size_stack
            .push(PercentageBasis::definite_from(
                basis,
                BlockSizeBasisSource::FlexItem,
            ));
        let result = layout(self);
        self.definite_block_size_stack.pop();
        result
    }

    /// Scope the semantic block-size basis of a replayed flex item.
    ///
    /// Replay materializes a final used height in a temporary style, but that
    /// representation must not itself grant definiteness to descendant
    /// percentages. The item's root formatting context consumes this one-shot
    /// basis before it starts ordinary descendant layout:
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    fn with_replayed_flex_item_percentage_height_basis<R>(
        &mut self,
        basis: FlexPercentageBasis,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let replay_basis = match basis {
            PercentageBasis::Definite { value, .. } => {
                PercentageBasis::definite_from(value, BlockSizeBasisSource::FlexItem)
            }
            PercentageBasis::Indefinite => PercentageBasis::indefinite(),
        };
        self.replayed_flex_item_percentage_height_bases
            .push(Some(replay_basis));
        let result = self.with_flex_item_percentage_height_basis(basis, layout);
        let consumed = self
            .replayed_flex_item_percentage_height_bases
            .pop()
            .flatten();
        debug_assert!(
            consumed.is_none(),
            "flex item replay basis must be consumed once"
        );
        result
    }

    /// Consume the pending basis for a replayed flex item's root formatting
    /// context. This is deliberately one-shot so nested ordinary blocks use
    /// their own CSS sizing results rather than the flex item's basis.
    pub(in crate::layout) fn take_replayed_flex_item_percentage_height_basis(
        &mut self,
    ) -> Option<BlockSizePercentageBasis> {
        self.replayed_flex_item_percentage_height_bases
            .last_mut()
            .and_then(Option::take)
    }
}

/// Convert a flex item's definite border-box height to a content-box basis.
///
/// Flex layout stores final item sizes as border-box sizes after margins have
/// been removed. Percentage `height` descendants resolve against the
/// containing block's content box, so padding and borders must be excluded:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch> and
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
fn flex_item_content_height_percentage_basis(
    style: &ComputedStyle,
    border_box_height: BorderBoxLength,
    source: FlexDefiniteSizeSource,
) -> FlexPercentageBasis {
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
    let content_height = border_box_to_content_box_length(border_box_height, vertical_non_content);
    PercentageBasis::definite_from(content_height, source)
}

fn flex_item_estimate_percentage_height_basis(
    available: FlexItemAvailableSpace,
) -> FlexPercentageBasis {
    // A definite preferred main size or flex base supplies the item's used
    // main-axis height while measuring descendants. Either therefore
    // establishes the percentage-height basis, while an ordinary definite
    // container constraint does not:
    // <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    if let PercentageBasis::Definite {
        value,
        source: FlexAvailableSizeSource::DefiniteFlexBase,
    } = available.height_basis
    {
        return PercentageBasis::definite_from(
            value,
            FlexDefiniteSizeSource::PostFlexingMainSizeFromDefiniteFlexBase,
        );
    }
    if let PercentageBasis::Definite {
        value,
        source:
            FlexAvailableSizeSource::DefinitePreferredMainSize
            | FlexAvailableSizeSource::DefinitePreferredCrossSize,
    } = available.height_basis
    {
        return PercentageBasis::definite_from(value, FlexDefiniteSizeSource::SpecifiedMainSize);
    }
    available
        .stretched_height
        .map(|height| {
            // `FlexItemAvailableSpace::stretched_height` is already a
            // physical content-box size. Re-labeling it as a border box here
            // would subtract padding and borders a second time before
            // descendant percentage resolution.
            PercentageBasis::definite_from(
                height.content_box_length(),
                FlexDefiniteSizeSource::StretchedCrossSizeFromDefiniteSingleLineContainer,
            )
        })
        .unwrap_or_else(PercentageBasis::indefinite)
}

fn flex_item_replay_percentage_height_basis(
    style: &ComputedStyle,
    border_box_height: BorderBoxLength,
    source: FlexDefiniteSizeSource,
) -> FlexPercentageBasis {
    flex_item_content_height_percentage_basis(style, border_box_height, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_width_contributions_preserve_non_negative_min_max_ordering() {
        let contributions = FlexIntrinsicWidthContributions::new(
            PhysicalContentWidth::new(content_box_pt(24.0)),
            PhysicalContentWidth::new(content_box_pt(12.0)),
        );
        assert_eq!(contributions.min_content.points(), 24.0);
        assert_eq!(contributions.max_content.points(), 24.0);

        let contributions = FlexIntrinsicWidthContributions::new(
            PhysicalContentWidth::new(content_box_pt(-4.0)),
            PhysicalContentWidth::new(content_box_pt(12.0)),
        );
        assert_eq!(contributions.min_content.points(), 0.0);
        assert_eq!(contributions.max_content.points(), 12.0);
    }

    #[test]
    fn stretched_content_height_is_not_reconverted_from_a_border_box() {
        let mut style = ComputedStyle::initial();
        style.padding.top = 10.0;
        style.padding.bottom = 20.0;
        let available = FlexItemAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(200.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(200.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
            stretched_width: None,
            stretched_height: Some(PhysicalContentHeight::new(content_box_pt(50.0))),
        };

        assert_eq!(
            flex_item_estimate_percentage_height_basis(available).value(),
            Some(content_box_pt(50.0))
        );
    }
}
