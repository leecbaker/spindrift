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
                width: available_width.max(0.0),
                width_is_definite: used_content_width_or_auto(
                    style,
                    available_width.max(0.0),
                    style.padding.left + style.padding.right + horizontal_border_width(style),
                )
                .is_some(),
                height: used_length_percentage_or_auto(style.box_values.height, available_width),
                height_is_definite: !style.box_values.height.is_auto(),
            },
        );
        (
            intrinsic.min_width.points().max(0.0),
            intrinsic.width.points().max(0.0),
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
        basis: Option<f32>,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(basis) = basis else {
            return layout(self);
        };
        self.definite_block_size_stack.push(Some(basis));
        let result = layout(self);
        self.definite_block_size_stack.pop();
        result
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
) -> Option<f32> {
    Some(
        (border_box_height
            - style.padding.top
            - style.padding.bottom
            - vertical_border_width(style))
        .max(0.0),
    )
}

fn flex_item_estimate_percentage_height_basis(
    style: &ComputedStyle,
    available: FlexItemAvailableSpace,
) -> Option<f32> {
    available
        .stretched_height
        .and_then(|height| flex_item_content_height_percentage_basis(style, height))
}

fn flex_item_replay_percentage_height_basis(
    child: &StyledChild<'_>,
    style: &ComputedStyle,
    border_box_height: f32,
) -> Option<f32> {
    let has_direct_inline_replaced_descendant = child
        .element_parts()
        .is_some_and(|(element, _, _)| has_direct_inline_replaced_child(element));
    has_direct_inline_replaced_descendant
        .then(|| flex_item_content_height_percentage_basis(style, border_box_height))
        .flatten()
}
