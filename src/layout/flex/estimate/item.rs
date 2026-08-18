use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Measures the content-size suggestion used by a flex item's automatic
    /// main-axis minimum.
    ///
    /// A definite preferred main size supplies the flex base, but Flexbox
    /// measures the content-size suggestion with that preferred size
    /// suppressed. This keeps the automatic-minimum input distinct from a
    /// definite flex basis in both intrinsic contribution sizing and final
    /// flexible-length resolution:
    /// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    pub(in crate::layout::flex) fn estimate_flex_item_automatic_main_min_content(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        item_available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> Option<ContentBoxLength> {
        let specified_min = if physical_direction.is_row_axis() {
            child.style.box_values.min_width.clone()
        } else {
            child.style.box_values.min_height.clone()
        };
        flex_main_axis_content_based_minimum_kind(&specified_min, &child.style, physical_direction)
            .is_some()
            .then(|| {
                let mut content_child = child.clone();
                let mut content_item_available = item_available;
                if physical_direction.is_row_axis() {
                    content_child.style.box_values.width =
                        css::ComputedLengthPercentageOrAuto::Auto;
                } else {
                    content_child
                        .style
                        .box_values
                        .height
                        .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
                    // The probe has removed the item's preferred physical main
                    // height. Its retained numeric constraint is still useful to
                    // the flex estimator, but it must no longer make descendant
                    // percentage heights definite.
                    content_item_available.make_height_percentage_basis_indefinite();
                }
                let content_estimate = self.estimate_flex_item_size(
                    &content_child,
                    stylesheets,
                    content_item_available,
                    physical_direction,
                );
                if physical_direction.is_row_axis() {
                    content_estimate.min_width
                } else {
                    content_estimate.min_height
                }
            })
    }

    /// Estimates the hypothetical content size of a flex item.
    ///
    /// CSS Flexbox defines flex base sizes and intrinsic contributions before
    /// the flex formatting algorithm distributes free space. Inline
    /// contributions are measured through `InlineOpportunityGraph` so flex
    /// estimates use the same CSS Text break opportunities as inline layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>,
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout::flex) fn estimate_flex_item_size(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> FlexItemEstimate {
        let vertical_non_content = non_content_pt(
            child.style.padding.top
                + child.style.padding.bottom
                + vertical_border_width(&child.style),
        );
        let percentage_height_basis = flex_item_estimate_percentage_height_basis(
            &child.style,
            available,
            vertical_non_content,
        );
        let mut estimate =
            self.with_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
                layout.estimate_flex_item_size_with_percentage_basis(
                    child,
                    stylesheets,
                    available,
                    physical_direction,
                )
            });
        if child
            .element_parts()
            .is_some_and(|(element, _, _)| used_property_containment(element, &child.style).layout)
        {
            // A layout-contained principal box exports no first/last baseline;
            // its flex/grid parent must use the synthesized fallback from the
            // border box instead.
            // <https://www.w3.org/TR/css-contain-1/#containment-layout>
            estimate.metrics.clear_block_baselines();
            estimate.baselines = FlexItemBaselineEstimate::default();
        }
        estimate
    }

    pub(in crate::layout::flex) fn estimate_flex_item_size_with_percentage_basis(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> FlexItemEstimate {
        // Flex keeps the child's source style for descendant cascade. This
        // intrinsic multicol probe is a layout consumer, so use a separate
        // normalized multicol style for its tracks and balancing geometry.
        let multicol_style = self.multicol_used_style(&child.style);
        let style = &multicol_style;
        let context = super::item_special_cases::FlexItemEstimateContext::new(
            style,
            available,
            physical_direction,
        );
        let style = context.style;
        let available = context.available;
        if let Some(children) = child.anonymous_content() {
            return self.estimate_anonymous_flex_item(children, stylesheets, context);
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(0.0)),
                PhysicalContentHeight::new(content_box_pt(0.0)),
            );
        };
        if used_property_containment(element, style).size
            && replaced_element_kind(element).is_none()
        {
            return self.estimate_size_contained_flex_item(
                element,
                signature,
                child_boxes,
                stylesheets,
                context,
            );
        }
        if style.display.is_flex()
            && let Some(estimate) = self.estimate_nested_flex_container_item(
                element,
                signature,
                child_boxes,
                stylesheets,
                context,
            )
        {
            return estimate;
        }

        if style.display.inner == DisplayInner::Grid
            && let Some(estimate) =
                self.estimate_grid_flex_item(element, signature, child_boxes, stylesheets, context)
        {
            return estimate;
        }

        let replaced_intrinsic = match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => Some(if element.tag == "iframe" {
                intrinsic_iframe_size(element)
            } else {
                intrinsic_canvas_size(element)
            }),
            Some(ReplacedElementKind::Image) => intrinsic_image_size(
                element,
                style,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| image.replaced_size())
            // A `<video>` without a poster still has a CSS replaced box. Its
            // media frame is unavailable to this static PDF renderer, but
            // Flexbox must use the HTML default object size for flex base and
            // automatic-minimum sizing rather than treating it as an empty
            // ordinary block.
            // <https://html.spec.whatwg.org/multipage/media.html#the-video-element>
            // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
            .or_else(|| (element.tag == "video").then(intrinsic_unavailable_image_size)),
            Some(ReplacedElementKind::Svg) => intrinsic_svg_size(element),
            None => None,
        };
        if let Some(intrinsic) = replaced_intrinsic
            && let Some(size) = estimate_replaced_flex_item(
                intrinsic,
                style,
                used_property_containment(element, style).size,
                available.width,
                available,
            )
        {
            return size;
        }

        if has_direct_inline_replaced_child(element)
            && !has_direct_flow_child_with_font_metrics(
                element,
                style,
                stylesheets,
                &mut self.font_system,
            )
        {
            return self.estimate_inline_replaced_row_flex_item(element, stylesheets, context);
        }

        self.estimate_normal_flow_flex_item(
            child,
            element,
            signature,
            child_boxes,
            stylesheets,
            context,
        )
    }
}
