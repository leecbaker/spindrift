use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Estimate a direct inline row for flex intrinsic sizing.
    ///
    /// CSS Flexbox asks each item for intrinsic size contributions before flex
    /// line placement, while CSS Inline and CSS Text define the line fragments
    /// and atomic inline boxes that determine row height:
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(super) fn measure_direct_inline_row(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> (f32, f32) {
        let measurement =
            self.with_intrinsic_inline_percentage_basis(PercentageBasis::indefinite(), |layout| {
                layout.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    None,
                    f32::MAX,
                )
            });
        (
            measurement.contribution.max_content.points(),
            measurement.height(),
        )
    }

    pub(super) fn push_ancestor_signature(&mut self, signature: ElementSignature) {
        self.ancestors.push(signature);
    }

    /// Run a layout query with one descendant signature on the selector ancestor stack.
    ///
    /// CSS selector matching for descendant layout estimates depends on the
    /// same ancestor chain used by normal box construction. This helper keeps
    /// recursive intrinsic estimates from leaking temporary ancestry into
    /// sibling estimates:
    /// <https://www.w3.org/TR/selectors-4/#overview> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    pub(super) fn with_ancestor_signature<R>(
        &mut self,
        signature: ElementSignature,
        estimate: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.ancestors.push(signature);
        let result = estimate(self);
        self.ancestors.pop();
        result
    }
}
