use super::*;

mod children;
mod compute;
mod estimate;
mod layout;
mod model;
mod taffy;

use children::*;
use model::*;
use taffy::*;

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
        stylesheets: &[Stylesheet],
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> (f32, f32) {
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
                width: PhysicalContentWidth::new(content_box_pt(available_width.max(0.0))),
                width_basis: flex_available_percentage_basis_from_points(
                    used_content_box_width_or_auto(
                        style,
                        layout_pt(available_width.max(0.0)),
                        non_content_pt(
                            style.padding.left
                                + style.padding.right
                                + horizontal_border_width(style),
                        ),
                    )
                    .map(|_| available_width.max(0.0)),
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                ),
                height: used_length_percentage_or_auto(
                    style.box_values.height.clone(),
                    PercentageBasis::definite(layout_pt(available_width)),
                )
                .map(|height| {
                    PhysicalContentHeight::new(crate::units::layout_to_content_box_length(height))
                }),
                height_basis: flex_available_percentage_basis_from_points(
                    used_length_percentage_or_auto(
                        style.box_values.height.clone(),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .map(|height| height.points()),
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                ),
            },
        );
        (
            intrinsic.min_width.points().max(0.0),
            intrinsic.width.points().max(0.0),
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
        stylesheets: &[Stylesheet],
        margin_box_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        let border_widths = used_border_widths(style);
        let horizontal_extras =
            border_widths.left + border_widths.right + style.padding.left + style.padding.right;
        let vertical_extras =
            border_widths.top + border_widths.bottom + style.padding.top + style.padding.bottom;
        let content_width = used_content_box_width_or_auto(
            style,
            layout_pt(margin_box_width),
            non_content_pt(horizontal_extras),
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| (margin_box_width - horizontal_extras).max(0.0));
        let height_basis = self.flex_container_height_percentage_basis();
        let explicit_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_basis,
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points);
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
                width: PhysicalContentWidth::new(content_box_pt(content_width.max(0.0))),
                width_basis: flex_available_percentage_basis_from_points(
                    Some(content_width.max(0.0)),
                    FlexAvailableSizeSource::ContainingBlock,
                ),
                height: explicit_content_height
                    .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                height_basis: flex_available_percentage_basis_from_points(
                    explicit_content_height,
                    FlexAvailableSizeSource::ContainingBlock,
                ),
            },
        );
        let content_height = explicit_content_height.unwrap_or(intrinsic.height.points());
        let height_basis = height_basis.points().unwrap_or(content_width);
        style.margin.top
            + vertical_extras
            + constrain_content_height(
                style,
                content_box_pt(content_height),
                PercentageBasis::definite(layout_pt(height_basis.max(0.0))),
            )
            .points()
            + style.margin.bottom
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
    border_box_height: f32,
    source: FlexDefiniteSizeSource,
) -> FlexPercentageBasis {
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
    let content_height =
        border_box_to_content_box_length(border_box_pt(border_box_height), vertical_non_content);
    PercentageBasis::definite_from(content_height, source)
}

fn flex_item_estimate_percentage_height_basis(
    style: &ComputedStyle,
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
            flex_item_content_height_percentage_basis(
                style,
                height.points(),
                FlexDefiniteSizeSource::StretchedCrossSizeFromDefiniteSingleLineContainer,
            )
        })
        .unwrap_or_else(PercentageBasis::indefinite)
}

fn flex_item_replay_percentage_height_basis(
    style: &ComputedStyle,
    border_box_height: f32,
    source: FlexDefiniteSizeSource,
) -> FlexPercentageBasis {
    flex_item_content_height_percentage_basis(style, border_box_height, source)
}
