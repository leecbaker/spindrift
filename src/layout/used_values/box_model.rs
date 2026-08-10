use super::*;
/// Used physical margin, padding, or border edges for a layout formatting
/// context.
///
/// CSS Box Model defines physical box edges and percentage resolution for
/// margin, padding, and border widths:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#border-width-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedEdges {
    pub(in crate::layout) top: LayoutLength,
    pub(in crate::layout) right: LayoutLength,
    pub(in crate::layout) bottom: LayoutLength,
    pub(in crate::layout) left: LayoutLength,
}

impl UsedEdges {
    /// Converts the renderer's existing edge shape to typed used lengths.
    ///
    /// CSS Box Model defines the physical edge order used here:
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn from_css_edges(edges: css::Edges) -> Self {
        Self {
            top: layout_pt(edges.top),
            right: layout_pt(edges.right),
            bottom: layout_pt(edges.bottom),
            left: layout_pt(edges.left),
        }
    }

    /// Converts used edge lengths back to the renderer's existing edge shape.
    ///
    /// CSS Box Model defines the physical edge order used here:
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn to_css_edges(self) -> css::Edges {
        css::Edges {
            top: self.top.points(),
            right: self.right.points(),
            bottom: self.bottom.points(),
            left: self.left.points(),
        }
    }
}

/// Used margin and padding edges for a box in a specific containing block.
///
/// CSS Box resolves margin and padding percentages against the containing
/// block's logical inline basis. In horizontal writing modes this is the
/// physical width described by CSS 2.2:
/// <https://drafts.csswg.org/css-box-3/#margin-physical> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedBoxEdges {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) padding: UsedEdges,
}

/// Used physical box metrics after margin and padding percentages are resolved.
///
/// CSS Box Model lays out content, padding, border, and margin as nested
/// physical edges; CSS Box resolves margin and padding percentages against the
/// containing block's logical inline basis before used geometry is computed:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://drafts.csswg.org/css-box-3/#margin-physical>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedBoxMetrics {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) padding: UsedEdges,
    pub(in crate::layout) border: UsedEdges,
}

impl UsedBoxMetrics {
    /// Returns horizontal padding and border in non-content box-model space.
    pub(in crate::layout) fn horizontal_non_content_length(self) -> NonContentLength {
        non_content_pt(
            (self.border.left + self.border.right + self.padding.left + self.padding.right)
                .points(),
        )
    }

    /// Returns vertical padding and border in non-content box-model space.
    pub(in crate::layout) fn vertical_non_content_length(self) -> NonContentLength {
        non_content_pt(
            (self.border.top + self.border.bottom + self.padding.top + self.padding.bottom)
                .points(),
        )
    }
}
/// Resolves used padding edges for the current containing block.
///
/// CSS Box says padding percentages on all sides refer to the containing
/// block's logical inline basis (the physical width in CSS 2.2 horizontal
/// writing): <https://drafts.csswg.org/css-box-3/#padding-physical>.
pub(in crate::layout) fn used_padding_edges<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength, Source>,
) -> UsedEdges {
    let padding = style.box_values.padding.clone();
    UsedEdges {
        top: used_padding_edge(padding.top, style.padding.top, inline_basis),
        right: used_padding_edge(padding.right, style.padding.right, inline_basis),
        bottom: used_padding_edge(padding.bottom, style.padding.bottom, inline_basis),
        left: used_padding_edge(padding.left, style.padding.left, inline_basis),
    }
}

/// Resolve padding edges from a logical inline percentage basis without
/// erasing its axis marker at the caller.
pub(in crate::layout) fn used_padding_edges_for_logical_inline_basis<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: LogicalInlinePercentageBasis<Source>,
) -> UsedEdges {
    used_padding_edges(
        style,
        inline_basis.map_value(crate::units::IntoLayoutLength::into_layout_length),
    )
}

/// Resolves one padding edge, using the typed percentage component when present.
///
/// CSS Box padding percentages resolve against the containing block's logical
/// inline basis: <https://drafts.csswg.org/css-box-3/#padding-physical>.
/// CSS Sizing resolves cyclic percentage contributions against zero during
/// intrinsic sizing, while preserving fixed lengths in the same calculation:
/// <https://drafts.csswg.org/css-sizing/#cyclic-percentage-contribution>.
pub(in crate::layout) fn used_padding_edge<Source>(
    value: css::ComputedLengthPercentage,
    legacy_length: f32,
    basis: PercentageBasis<LayoutLength, Source>,
) -> LayoutLength {
    css::clamp_used_layout_length(layout_pt(
        value
            .length_if_no_percent()
            .map(|_| legacy_length)
            .unwrap_or_else(|| used_length_percentage(value, basis).points())
            .max(0.0),
    ))
}

/// Resolves used margin edges for the current containing block.
///
/// CSS Box says margin percentages on all sides refer to the containing
/// block's logical inline basis. Auto margins are resolved by the formatting context; this helper
/// returns zero for auto edges when a caller only needs occupied non-auto
/// margin space:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::layout) fn used_margin_edges<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength, Source>,
) -> UsedEdges {
    let margin = style.box_values.margin.clone();
    UsedEdges {
        top: used_margin_edge(margin.top, style.margin.top, inline_basis),
        right: used_margin_edge(margin.right, style.margin.right, inline_basis),
        bottom: used_margin_edge(margin.bottom, style.margin.bottom, inline_basis),
        left: used_margin_edge(margin.left, style.margin.left, inline_basis),
    }
}

/// Resolves padding edges for intrinsic size contributions.
///
/// CSS Sizing defines intrinsic size contributions in terms of the box's outer
/// size, but cyclic percentages in margins and padding resolve against zero
/// for those contributions. This helper uses computed padding values rather
/// than any cached used edges from a previous layout pass:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_padding_edges(style: &ComputedStyle) -> UsedEdges {
    let padding = style.box_values.padding.clone();
    UsedEdges {
        top: intrinsic_padding_edge(padding.top),
        right: intrinsic_padding_edge(padding.right),
        bottom: intrinsic_padding_edge(padding.bottom),
        left: intrinsic_padding_edge(padding.left),
    }
}

/// Resolves one padding edge for intrinsic size contributions.
///
/// CSS Sizing resolves cyclic percentage padding contributions against zero,
/// preserving fixed length components in the same value:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_padding_edge(
    value: css::ComputedLengthPercentage,
) -> LayoutLength {
    used_length_percentage(value, PercentageBasis::definite(layout_pt(0.0))).max(layout_pt(0.0))
}

/// Resolves margin edges for intrinsic size contributions.
///
/// CSS Sizing bases intrinsic contributions on outer size, treats auto margins
/// as zero, and resolves cyclic percentage margin contributions against zero:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_margin_edges(style: &ComputedStyle) -> UsedEdges {
    let margin = style.box_values.margin.clone();
    UsedEdges {
        top: intrinsic_margin_edge(margin.top),
        right: intrinsic_margin_edge(margin.right),
        bottom: intrinsic_margin_edge(margin.bottom),
        left: intrinsic_margin_edge(margin.left),
    }
}

/// Resolves one margin edge for intrinsic size contributions.
///
/// CSS Sizing resolves cyclic percentage margin contributions against zero and
/// treats auto margins as zero:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_margin_edge(
    value: css::ComputedLengthPercentageOrAuto,
) -> LayoutLength {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            used_length_percentage(value, PercentageBasis::definite(layout_pt(0.0)))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => layout_pt(0.0),
    }
}

/// Resolves one margin edge, preserving formatting-context handling for `auto`.
///
/// CSS Box margin percentages resolve against the containing block's logical
/// inline basis: <https://drafts.csswg.org/css-box-3/#margin-physical>.
/// CSS Sizing resolves cyclic percentage contributions against zero during
/// intrinsic sizing, while preserving fixed lengths in the same calculation:
/// <https://drafts.csswg.org/css-sizing/#cyclic-percentage-contribution>.
pub(in crate::layout) fn used_margin_edge<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: PercentageBasis<LayoutLength, Source>,
) -> LayoutLength {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => layout_pt(0.0),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value
            .length_if_no_percent()
            .map(|_| layout_pt(legacy_length))
            .unwrap_or_else(|| used_length_percentage(value, basis)),
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => layout_pt(legacy_length),
    }
}

/// Synchronize resolved fixed box edges with the renderer's legacy edge cache.
///
/// The typed computed box values retain selected-font metric units until the
/// used-value stage. Inline box edges are consumed immediately after that
/// resolution, while older layout code still reads the scalar edge cache for
/// fixed values. Project only fixed values here; percentage edges must remain
/// deferred to their containing-block basis.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>
pub(in crate::layout) fn synchronize_resolved_fixed_box_edge_cache(style: &mut ComputedStyle) {
    let margin = style.box_values.margin.clone();
    let padding = style.box_values.padding.clone();
    if let Some(value) = margin.top.length_if_no_percent() {
        style.margin.top = value;
    }
    if let Some(value) = margin.right.length_if_no_percent() {
        style.margin.right = value;
    }
    if let Some(value) = margin.bottom.length_if_no_percent() {
        style.margin.bottom = value;
    }
    if let Some(value) = margin.left.length_if_no_percent() {
        style.margin.left = value;
    }
    if let Some(value) = padding.top.length_if_no_percent() {
        style.padding.top = value;
    }
    if let Some(value) = padding.right.length_if_no_percent() {
        style.padding.right = value;
    }
    if let Some(value) = padding.bottom.length_if_no_percent() {
        style.padding.bottom = value;
    }
    if let Some(value) = padding.left.length_if_no_percent() {
        style.padding.left = value;
    }
}

/// Remove an item's margins after its parent layout algorithm has already
/// positioned its margin box.
///
/// Flex and Grid replay an item through an independent normal-flow formatting
/// context after their layout algorithms consume the item's margins. Clear
/// both representations because normal-flow used-value resolution rebuilds
/// the scalar edge cache from the typed computed values.
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
pub(in crate::layout) fn suppress_replayed_item_margins(style: &mut ComputedStyle) {
    style.margin = css::Edges::ZERO;
    style.box_values.margin = css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
}

/// Freezes an item's already-resolved physical padding for normal-flow replay.
///
/// Flex and Grid resolve padding percentages against the containing block's
/// logical inline size before placing an item. Replaying the item against its
/// own content box must retain those used edges instead of resolving the
/// percentage against a different basis.
/// <https://www.w3.org/TR/css-box-3/#padding-physical> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
pub(in crate::layout) fn freeze_replayed_item_padding(
    style: &mut ComputedStyle,
    padding: css::Edges,
) {
    style.padding = padding;
    style.box_values.padding = css::CssEdges {
        top: css::ComputedLengthPercentage::from_points(padding.top),
        right: css::ComputedLengthPercentage::from_points(padding.right),
        bottom: css::ComputedLengthPercentage::from_points(padding.bottom),
        left: css::ComputedLengthPercentage::from_points(padding.left),
    };
}

/// Resolves both margin and padding edges for a box.
///
/// CSS 2.2 defines the used-value resolution for margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn used_box_edges<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength, Source>,
) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: used_margin_edges(style, inline_basis),
        padding: used_padding_edges(style, inline_basis),
    }
}

/// Resolves margin and padding edges for intrinsic size contributions.
///
/// CSS Sizing's cyclic-percentage contribution rules are distinct from final
/// used-value resolution, so intrinsic sizing callers must not reuse normal
/// used edges resolved against a concrete containing block:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_box_edges(style: &ComputedStyle) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: intrinsic_margin_edges(style),
        padding: intrinsic_padding_edges(style),
    }
}

/// Return used box metrics without mutating the caller's style.
///
/// This is useful for intrinsic sizing paths that need resolved non-content
/// edges but must not overwrite the computed style carried by a formatting box.
/// CSS separates computed and used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn used_box_metrics<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength, Source>,
) -> UsedBoxMetrics {
    let used_edges = used_box_edges(style, inline_basis);
    UsedBoxMetrics {
        margin: used_edges.margin,
        padding: used_edges.padding,
        border: UsedEdges::from_css_edges(used_border_widths(style)),
    }
}

/// Return used box metrics from a CSS logical inline percentage basis.
///
/// This is the preferred boundary for layout modes that know the containing
/// block's writing-mode projection. It prevents a physical width or height
/// from being passed as a CSS Box edge basis by accident.
/// <https://drafts.csswg.org/css-box-3/#margin-physical>
pub(in crate::layout) fn used_box_metrics_for_logical_inline_basis<Source: Copy>(
    style: &ComputedStyle,
    inline_basis: LogicalInlinePercentageBasis<Source>,
) -> UsedBoxMetrics {
    used_box_metrics(
        style,
        inline_basis.map_value(crate::units::IntoLayoutLength::into_layout_length),
    )
}

/// Return box metrics for intrinsic size contributions.
///
/// The returned margin and padding edges are resolved from computed values with
/// a zero cyclic-percentage basis, while borders remain ordinary used border
/// widths:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_box_metrics(style: &ComputedStyle) -> UsedBoxMetrics {
    let intrinsic_edges = intrinsic_box_edges(style);
    UsedBoxMetrics {
        margin: intrinsic_edges.margin,
        padding: intrinsic_edges.padding,
        border: UsedEdges::from_css_edges(used_border_widths(style)),
    }
}

/// Resolve a temporary layout style's margin and padding and return box metrics.
///
/// Layout code often needs both the mutated style, so later code can consume
/// used edge values, and the derived non-content sizes. Keeping those steps
/// together avoids call sites accidentally mixing computed and used edges:
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn apply_used_box_metrics(
    style: &mut ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedBoxMetrics {
    let metrics = used_box_metrics(style, inline_basis);
    style.margin = metrics.margin.to_css_edges();
    style.padding = metrics.padding.to_css_edges();
    metrics
}

/// Resolve and cache box metrics from a CSS logical inline basis.
///
/// Formatting contexts that know their containing block's writing-mode
/// projection should use this instead of converting through `LayoutLength` at
/// the call site.
pub(in crate::layout) fn apply_used_box_metrics_for_logical_inline_basis<Source: Copy>(
    style: &mut ComputedStyle,
    inline_basis: LogicalInlinePercentageBasis<Source>,
) -> UsedBoxMetrics {
    let metrics = used_box_metrics_for_logical_inline_basis(style, inline_basis);
    style.margin = metrics.margin.to_css_edges();
    style.padding = metrics.padding.to_css_edges();
    metrics
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn length_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn percent_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(value),
        )
    }
    #[test]
    fn used_padding_edge_resolves_zero_percent_calc_against_zero_basis() {
        let calc_zero_percent =
            css::ComputedLengthPercentage::from_affine(layout_pt(50.0), 0.0, true);

        assert_eq!(
            used_padding_edge(
                calc_zero_percent,
                0.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(50.0)
        );
        assert_eq!(
            used_padding_edge(
                css::ComputedLengthPercentage::from_points(7.0),
                7.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(7.0)
        );
        assert_eq!(
            used_padding_edge(
                css::ComputedLengthPercentage::from_percent(0.25),
                0.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(20.0)
        );
    }

    #[test]
    fn used_margin_edge_resolves_zero_percent_calc_against_zero_basis() {
        let calc_zero_percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(30.0), 0.0, true),
        );

        assert_eq!(
            used_margin_edge(
                calc_zero_percent,
                0.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(30.0)
        );
        assert_eq!(
            used_margin_edge(
                length_auto(9.0),
                9.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(9.0)
        );
        assert_eq!(
            used_margin_edge(
                percent_auto(0.25),
                0.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(20.0)
        );
        assert_eq!(
            used_margin_edge(
                css::ComputedLengthPercentageOrAuto::Auto,
                42.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(0.0)
        );
    }

    #[test]
    fn intrinsic_margin_edges_resolve_cyclic_percentages_against_zero() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(100.0), 0.10, true),
        );
        style.box_values.margin.right = percent_auto(0.25);
        style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        style.margin.left = 999.0;
        style.margin.right = 999.0;

        let margin = intrinsic_margin_edges(&style).to_css_edges();

        assert_eq!(margin.left, 100.0);
        assert_eq!(margin.right, 0.0);
        assert_eq!(margin.top, 0.0);
    }

    #[test]
    fn intrinsic_padding_edges_resolve_cyclic_percentages_against_zero() {
        let mut style = ComputedStyle::initial();
        style.box_values.padding.left =
            css::ComputedLengthPercentage::from_affine(layout_pt(50.0), 0.20, true);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(0.25);
        style.box_values.padding.top = css::ComputedLengthPercentage::from_points(-5.0);
        style.padding.left = 999.0;

        let padding = intrinsic_padding_edges(&style).to_css_edges();

        assert_eq!(padding.left, 50.0);
        assert_eq!(padding.right, 0.0);
        assert_eq!(padding.top, 0.0);
    }

    #[test]
    fn intrinsic_box_metrics_include_zero_basis_edges_and_borders() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(10.0), 0.25, true),
        );
        style.box_values.padding.left =
            css::ComputedLengthPercentage::from_affine(layout_pt(20.0), 0.25, true);
        style.border_width_values.left = css::ComputedLengthPercentage::from_points(3.0);
        style.border_width_values.right = css::ComputedLengthPercentage::from_points(4.0);
        style.border_widths.left = 3.0;
        style.border_widths.right = 4.0;
        style.border_styles.left = css::BorderStyle::Solid;
        style.border_styles.right = css::BorderStyle::Solid;

        let metrics = intrinsic_box_metrics(&style);

        assert_eq!(metrics.margin.left, layout_pt(10.0));
        assert_eq!(metrics.padding.left, layout_pt(20.0));
        assert_eq!(metrics.border.left, layout_pt(3.0));
        assert_eq!(metrics.border.right, layout_pt(4.0));
        assert_eq!(
            metrics.horizontal_non_content_length(),
            non_content_pt(27.0)
        );
    }

    #[test]
    fn applying_used_box_metrics_updates_style_only_at_the_css_edge_boundary() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = percent_auto(0.25);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(0.5);
        style.border_width_values.top = css::ComputedLengthPercentage::from_points(3.0);
        style.border_widths.top = 3.0;
        style.border_styles.top = css::BorderStyle::Solid;

        let metrics =
            apply_used_box_metrics(&mut style, PercentageBasis::definite(layout_pt(80.0)));

        assert_eq!(metrics.margin.left, layout_pt(20.0));
        assert_eq!(metrics.padding.right, layout_pt(40.0));
        assert_eq!(metrics.border.top, layout_pt(3.0));
        assert_eq!(style.margin.left, 20.0);
        assert_eq!(style.padding.right, 40.0);
    }
    #[test]
    fn logical_inline_box_metrics_keep_axis_identity_until_resolution() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = percent_auto(0.1);
        style.box_values.padding.left = css::ComputedLengthPercentage::from_percent(0.1);

        let metrics = used_box_metrics_for_logical_inline_basis(
            &style,
            PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(100.0))),
        );

        assert_eq!(metrics.margin.left.points(), 10.0);
        assert_eq!(metrics.padding.left.points(), 10.0);
    }
}
