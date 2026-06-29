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
                box_tree::build_child_boxes(element, stylesheets, style, &self.ancestors);
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
        (intrinsic.min_width.max(0.0), intrinsic.width.max(0.0))
    }

    fn flex_container_signature(&self, element: &Element) -> ElementSignature {
        self.ancestors
            .last()
            .cloned()
            .unwrap_or_else(|| ElementSignature::new(element.tag.clone(), element.attrs.clone()))
    }
}
