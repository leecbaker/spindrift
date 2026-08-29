use super::*;
use crate::css::ContainerType;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use crate::units::LayoutSize;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn resolve_style_viewport_lengths(
        style: &mut ComputedStyle,
        viewport: LayoutSize,
        container_physical: LayoutSize,
    ) {
        style.resolve_viewport_lengths_for_viewport_and_container(viewport, container_physical);
        if let Some(style) = &mut style.marker_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
        if let Some(style) = &mut style.before_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
        if let Some(style) = &mut style.after_style {
            Self::resolve_style_viewport_lengths(style, viewport, container_physical);
        }
    }

    pub(in crate::layout) fn style_with_current_viewport_lengths(
        &self,
        style: &impl css::CascadedStyleSource,
    ) -> css::ZoomedLayoutStyle {
        let mut style = css::LayoutStyle::from_computed(style);
        self.resolve_style_current_viewport_lengths(&mut style);
        style.into_zoomed()
    }

    pub(in crate::layout) fn style_with_current_used_lengths(
        &mut self,
        style: &impl css::CascadedStyleSource,
    ) -> css::ZoomedLayoutStyle {
        let mut style = css::LayoutStyle::from_computed(style);
        self.resolve_style_current_viewport_lengths(&mut style);
        // A frozen box tree can retain an `em`/`rem` expression until it is
        // replayed for intrinsic sizing or an isolated formatting context.
        // These units are computed from the element's resolved font sizes,
        // independently of any containing-block percentage basis.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        style.finalize_computed_font_relative_lengths();
        self.resolve_style_font_metric_lengths(&mut style);
        let mut style = style.into_zoomed();
        // Viewport and font-relative units can turn an authored box edge into
        // a fixed computed length after cascading. Keep the legacy used-edge
        // cache synchronized so its fixed-edge fast path does not retain the
        // pre-resolution value.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-cascade-5/#computed>
        synchronize_resolved_fixed_box_edge_cache(&mut style);
        style
    }

    pub(in crate::layout) fn resolve_style_current_viewport_lengths(
        &self,
        style: &mut ComputedStyle,
    ) {
        // Document viewport-relative lengths resolve against the immutable
        // initial containing block. An embedded document instead has the
        // iframe's finite browsing-context viewport, even though its static
        // layout surface is deliberately made tall to avoid fragmentation.
        // A destination page may otherwise have a different used page area
        // through a named or spread `@page` rule, but that changes layout
        // geometry rather than the document viewport.
        // <https://www.w3.org/TR/css-page-3/#page-model>
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        let viewport = self
            .iframe_viewport
            .map(|context| context.viewport.layout_size())
            .unwrap_or_else(|| {
                LayoutSize::new(
                    self.initial_viewport_context.area_width(),
                    self.initial_viewport_context.area_height(),
                )
            });
        Self::resolve_style_viewport_lengths(
            style,
            viewport,
            self.current_container_unit_physical(viewport),
        );
    }

    /// Select the nearest eligible query container independently for the
    /// physical width and height axes. The CSS unit resolver maps those
    /// physical values to `cqi`/`cqb` using the consuming style's writing
    /// mode.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    fn current_container_unit_physical(&self, fallback: LayoutSize) -> LayoutSize {
        let mut width = None;
        let mut height = None;
        for context in self.container_unit_contexts.iter().rev().copied() {
            if width.is_none() && context.supplies_physical_width() {
                width = Some(context.physical_width.points());
            }
            if height.is_none() && context.supplies_physical_height() {
                height = Some(context.physical_height.points());
            }
            if width.is_some() && height.is_some() {
                break;
            }
        }
        LayoutSize::new(
            width.unwrap_or(fallback.width),
            height.unwrap_or(fallback.height),
        )
    }

    /// Enter one layout-time query-container scope after its used content box
    /// has been resolved. `container-type: normal` deliberately adds no
    /// record, keeping unrelated descendants out of the selection walk.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    pub(in crate::layout) fn push_container_unit_context(
        &mut self,
        style: &ComputedStyle,
        physical_width: PhysicalContentWidth,
        physical_height: PhysicalContentHeight,
    ) -> bool {
        if matches!(style.container_type, ContainerType::Normal) {
            return false;
        }
        self.container_unit_contexts.push(ContainerUnitContext {
            physical_width,
            physical_height,
            writing_mode: style.writing_mode,
            container_type: style.container_type,
        });
        true
    }

    pub(in crate::layout) fn pop_container_unit_context(&mut self, active: bool) {
        if active {
            self.container_unit_contexts
                .pop()
                .expect("container unit scopes must be lexically balanced");
        }
    }

    pub(in crate::layout) fn style_for_layout_element_with_parent_font_metrics(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
    ) -> ComputedStyle {
        let ancestors = self.ancestors.clone();
        self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            element,
            signature,
            stylesheets,
            parent,
            &ancestors,
        )
    }

    pub(in crate::layout) fn style_for_layout_element_with_parent_font_metrics_and_ancestors(
        &mut self,
        element: &Element,
        signature: ElementSignature,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let signature = layout_element_signature(element, signature, parent);
        let inline_style = element.attrs.get("style").map(String::as_str);
        let mut style = style_for_layout_signature_with_parent_ch_advance(
            signature.clone(),
            inline_style,
            stylesheets,
            parent,
            ancestors,
            Some(parent_ch_advance),
        );
        if style
            .deferred_font_size
            .requires_parent_ch_advance(inheritance_source.font_size)
        {
            parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
            style = style_for_layout_signature_with_parent_ch_advance(
                signature.clone(),
                inline_style,
                stylesheets,
                parent,
                ancestors,
                Some(parent_ch_advance),
            );
        }
        let pseudo_parent_ch_advance = css::fallback_ch_advance_for_style(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        if style.pseudo_styles_require_parent_ch_advance() {
            let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
            css::apply_pseudo_rules_with_parent_ch_advance(
                &mut style,
                &signature,
                stylesheets,
                ancestors,
                pseudo_parent_ch_advance,
            );
        }
        self.resolve_deferred_parent_font_metric_font_size(&mut style, &inheritance_source);
        self.resolve_deferred_root_font_metric_font_size(&mut style);
        self.resolve_style_font_metric_lengths(&mut style);
        // Computed styles deliberately do not inherit text-decoration
        // longhands.  At this layout boundary, materialize the decorating
        // ancestors as used-style paint layers instead.  Keeping this after
        // pseudo and font-metric resolution preserves the decorating box's
        // resolved paint parameters while allowing descendant text to retain
        // its own computed style.
        //
        // CSS Text Decoration Level 4 § 2.1, Line Decoration: text
        // decorations propagate through in-flow descendants, rather than
        // behaving as inherited CSS properties.
        if let Some(parent_style) = parent {
            style =
                TextDecorationPropagationContext::from_style(parent_style).used_child_style(&style);
        }
        style
    }

    pub(in crate::layout) fn style_for_signature_with_parent_font_metrics(
        &mut self,
        signature: ElementSignature,
        inline_style: Option<&str>,
        stylesheets: &Stylesheets<'_>,
        parent: Option<&ComputedStyle>,
        ancestors: &[ElementSignature],
    ) -> ComputedStyle {
        let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
        let mut parent_ch_advance = css::fallback_ch_advance_for_style(&inheritance_source);
        let mut style = css::style_for_element_with_signature_and_parent_ch_advance(
            signature.clone(),
            inline_style,
            stylesheets,
            parent,
            ancestors,
            parent_ch_advance,
        );
        if style
            .deferred_font_size
            .requires_parent_ch_advance(inheritance_source.font_size)
        {
            parent_ch_advance = self.font_system.ch_advance(&inheritance_source);
            style = css::style_for_element_with_signature_and_parent_ch_advance(
                signature.clone(),
                inline_style,
                stylesheets,
                parent,
                ancestors,
                parent_ch_advance,
            );
        }
        let pseudo_parent_ch_advance = css::fallback_ch_advance_for_style(&style);
        css::apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &signature,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
        if style.pseudo_styles_require_parent_ch_advance() {
            let pseudo_parent_ch_advance = self.font_system.ch_advance(&style);
            css::apply_pseudo_rules_with_parent_ch_advance(
                &mut style,
                &signature,
                stylesheets,
                ancestors,
                pseudo_parent_ch_advance,
            );
        }
        self.resolve_deferred_parent_font_metric_font_size(&mut style, &inheritance_source);
        self.resolve_deferred_root_font_metric_font_size(&mut style);
        self.resolve_style_font_metric_lengths(&mut style);
        style
    }
}
