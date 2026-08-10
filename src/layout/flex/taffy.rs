use super::*;
use crate::layout::taffy_bridge;

type FlexLogicalInlinePercentageBasis = LogicalInlinePercentageBasis<FlexAvailableSizeSource>;

/// Wraps a raw Taffy layout result in the Taffy coordinate space.
///
/// Taffy returns physical x/y coordinates after Quire has mapped CSS flex axes
/// and writing direction into Taffy's row/column model. The returned rect must
/// be converted to container coordinates before storage in flex layout data:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_rect_from_layout(layout: &taffy_layout::Layout) -> TaffyRect {
    TaffyRect::new(
        TaffyPoint::new(layout.location.x, layout.location.y),
        TaffySize::new(layout.size.width, layout.size.height),
    )
}

/// Converts computed CSS margins to Taffy's flex-item margin representation.
///
/// CSS Flexible Box Layout uses margin boxes during flex item sizing and
/// alignment, and CSS Box Model permits negative margins to shift and overlap
/// boxes:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(super) fn taffy_margin(
    style: &ComputedStyle,
    containing_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let percentage_basis = logical_inline_percentage_basis(containing_style, available);
    taffy_bridge::margin(
        style,
        percentage_basis,
        taffy_bridge::TaffyCyclicPercentage::PreservePurePercentage,
    )
}

/// Converts computed CSS padding to Taffy's flex-item padding representation.
///
/// CSS Flexible Box Layout sizes flex items using their box model, and CSS
/// Box Model defines padding edge behavior:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn taffy_padding(
    style: &ComputedStyle,
    containing_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    // CSS Box resolves every physical padding percentage against the
    // containing block's logical inline size. Resolve it at this typed
    // physical/logical adapter boundary instead of letting Taffy apply a
    // physical-width percentage in vertical writing modes:
    // <https://www.w3.org/TR/css-box-3/#padding-physical>.
    let percentage_basis = logical_inline_percentage_basis(containing_style, available);
    taffy_bridge::padding(style, percentage_basis)
}

/// Resolve a flex item's padding for replay and box-model calculations.
///
/// Taffy receives physical edges, but CSS padding percentages retain the
/// flex container's logical inline basis even in vertical writing modes:
/// <https://www.w3.org/TR/css-box-3/#padding-physical>.
pub(super) fn flex_item_used_padding(
    style: &ComputedStyle,
    containing_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> css::Edges {
    let percentage_basis = logical_inline_percentage_basis(containing_style, available);
    if percentage_basis.is_definite() {
        used_box_metrics_for_logical_inline_basis(style, percentage_basis.map_source(|_| ()))
            .padding
            .to_css_edges()
    } else {
        style.padding
    }
}

/// Resolve a flex item's physical margins using its container's logical inline
/// percentage basis. Auto margins remain zero for size calculations; Taffy
/// retains their auto state separately for alignment.
/// <https://www.w3.org/TR/css-box-3/#margin-physical>.
pub(super) fn flex_item_used_margin(
    style: &ComputedStyle,
    containing_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> css::Edges {
    let percentage_basis = logical_inline_percentage_basis(containing_style, available);
    if percentage_basis.is_definite() {
        used_box_metrics_for_logical_inline_basis(style, percentage_basis.map_source(|_| ()))
            .margin
            .to_css_edges()
    } else {
        style.margin
    }
}

/// Return the percentage basis for physical box edges in a flex container.
///
/// CSS Box resolves all margin and padding percentages against the containing
/// block's logical inline size, before the Taffy adapter uses physical edges:
/// <https://www.w3.org/TR/css-box-3/#margin-physical>.
fn logical_inline_percentage_basis(
    containing_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> FlexLogicalInlinePercentageBasis {
    if WritingModeAxes::new(
        containing_style.writing_mode,
        containing_style.used_direction(),
    )
    .swaps_physical_axes()
    {
        available.height_basis
    } else {
        available.width_basis
    }
    .map_value(LogicalInlineContentSize::new)
}

/// Converts used border widths to Taffy's length-only edge representation.
///
/// CSS Flexible Box Layout includes borders in flex item sizing through the
/// CSS box model:
/// <https://www.w3.org/TR/css-flexbox-1/#box-model>.
pub(super) fn taffy_edges(edges: css::Edges) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    taffy_bridge::border_edges(edges)
}

/// Converts computed CSS gaps to Taffy's flex gap representation.
///
/// CSS Box Alignment defines `normal` gaps as zero in flex layout and
/// percentage gaps as resolving against the corresponding content-box size.
/// Cyclic percentage gaps contribute zero during intrinsic sizing, so an
/// indefinite basis preserves only the non-percentage length component:
/// <https://www.w3.org/TR/css-align-3/#gap-percent>.
pub(super) fn taffy_gap(
    value: css::ComputedGap,
    percentage_basis: FlexAvailablePercentageBasis,
) -> taffy_layout::LengthPercentage {
    taffy_bridge::gap(value, percentage_basis)
}

/// Converts a flex item's physical size for Taffy while preserving auto cross sizes.
///
/// CSS Flexbox resolves `auto` main sizes through content sizing for the flex
/// base size, but `align-items: stretch` requires the flex item's cross-size
/// property to remain automatic until flex lines are resolved. When
/// `flex-basis` is not `auto`, it is used in place of the main-size property
/// for flex base sizing, so the authored main-size property must not be
/// supplied to Taffy as a known main-axis size:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch> and
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
pub(super) fn flex_item_size_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    fallback: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    context: FlexItemSizeDimensionContext,
) -> taffy_layout::Dimension {
    if context
        .flex_direction
        .shares_axis_with(context.dimension_axis)
    {
        if context.flex_basis_overrides_main_size {
            return taffy_layout::Dimension::auto();
        }
        // An unresolved percentage main size is `auto` for flex-basis:auto.
        // The flex-basis adapter below supplies the item's content-based
        // fallback; passing Taffy a zero-length `size` here would instead
        // suppress that fallback and clip the item's normal content.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>
        // <https://www.w3.org/TR/css-sizing-3/#percentages>
        if matches!(value, css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value) if value.needs_percentage_basis() && !context.percentage_basis.is_definite())
        {
            return taffy_layout::Dimension::auto();
        }
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => {
                taffy_layout::Dimension::length(fallback.points().max(1.0))
            }
            _ => taffy_intrinsic_dimension_with_basis_and_stretch(
                value,
                context.percentage_basis,
                min_content,
                max_content,
                context.stretch,
            ),
        }
    } else {
        match value {
            css::ComputedLengthPercentageOrAuto::Auto => {
                if context.auto_cross_uses_stretch_fit {
                    taffy_stretch_fit_dimension(context.stretch)
                } else {
                    context
                        .auto_cross_fit_content
                        .map(|size| taffy_layout::Dimension::length(size.points().max(0.0)))
                        .unwrap_or_else(taffy_layout::Dimension::auto)
                }
            }
            _ => taffy_intrinsic_dimension_with_basis_and_stretch(
                value,
                context.percentage_basis,
                min_content,
                max_content,
                context.stretch,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemSizeDimensionContext {
    pub(super) flex_direction: FlexDirection,
    pub(super) dimension_axis: FlexDirection,
    pub(super) percentage_basis: FlexAvailablePercentageBasis,
    pub(super) stretch: FlexStretchFitContext,
    pub(super) flex_basis_overrides_main_size: bool,
    /// A balanced flex container with an explicit `flex-line-count` gives
    /// stretched auto cross sizes a definite per-line available size before
    /// line formation:
    /// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>.
    pub(super) auto_cross_uses_stretch_fit: bool,
    /// Wrapping column flex containers determine an automatic item's
    /// hypothetical cross size by fit-content sizing against the definite
    /// container cross size before forming lines:
    /// <https://drafts.csswg.org/css-flexbox-1/#algo-cross-item>.
    pub(super) auto_cross_fit_content: Option<ContentBoxLength>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexStretchFitContext {
    pub(super) available_margin_box_size: Option<LayoutLength>,
    pub(super) margin_size: LayoutLength,
    pub(super) non_content_size: NonContentLength,
    pub(super) box_sizing: BoxSizing,
}

fn taffy_stretch_fit_dimension(context: FlexStretchFitContext) -> taffy_layout::Dimension {
    let Some(available) = context.available_margin_box_size else {
        return taffy_layout::Dimension::auto();
    };
    let content_size =
        stretch_fit_content_box_size(available, context.margin_size, context.non_content_size);
    let size = match context.box_sizing {
        BoxSizing::ContentBox => content_size.points(),
        BoxSizing::BorderBox => {
            content_box_to_border_box_length(content_size, context.non_content_size).points()
        }
    };
    taffy_layout::Dimension::length(size.max(0.0))
}

/// Converts a CSS size to Taffy, resolving mixed length-percentages when possible.
///
/// CSS Values allows `<length-percentage>` math such as `calc(50% + 10pt)`.
/// Taffy's style interface cannot carry Quire's mixed CSS math representation
/// or percentage-definiteness semantics, so flex layout resolves mixed values
/// at this bridge when the relevant flex container axis is definite. CSS Sizing
/// leaves nonzero percentages unresolved when their basis is indefinite, so
/// those values stay automatic for Taffy and use intrinsic measurement instead:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>,
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>, and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_optional_dimension_with_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
) -> taffy_layout::Dimension {
    taffy_flex_optional_dimension(
        value,
        FlexTaffyPercentagePolicy::ResolveAgainstDefiniteBasis(percentage_basis),
    )
}

/// Selects whether a flex size may remain a symbolic Taffy percentage.
///
/// Flex item's used size resolution needs a definite containing-block basis,
/// while the flex container's own optional maximum constraint may preserve a
/// pure percentage for Taffy's later resolution. These are separate CSS
/// sizing phases, not interchangeable representations:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
#[derive(Debug, Clone, Copy)]
enum FlexTaffyPercentagePolicy {
    PreservePurePercentage,
    ResolveAgainstDefiniteBasis(FlexAvailablePercentageBasis),
}

/// Convert a Flex optional size through the policy selected by its CSS phase.
///
/// The value grammar is shared between the symbolic container constraint and
/// the definite-basis item sizing path. The policy governs only the
/// `<length-percentage>` arm; Flex's higher-level flex-basis, stretch-fit, and
/// automatic minimum-size logic remains with its callers.
fn taffy_flex_optional_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_policy: FlexTaffyPercentagePolicy,
) -> taffy_layout::Dimension {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_flex_length_percentage_dimension(value, percentage_policy)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => taffy_layout::Dimension::auto(),
    }
}

fn taffy_flex_length_percentage_dimension(
    value: css::ComputedLengthPercentage,
    percentage_policy: FlexTaffyPercentagePolicy,
) -> taffy_layout::Dimension {
    let FlexTaffyPercentagePolicy::ResolveAgainstDefiniteBasis(percentage_basis) =
        percentage_policy
    else {
        return taffy_dimension_from_length_percentage(value);
    };
    let percentage_basis = percentage_basis.points();
    if let Some(basis) = percentage_basis
        && let Some(resolved) = value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis.max(0.0))))
    {
        return taffy_layout::Dimension::length(resolved.points());
    }
    if value.needs_percentage_basis() && percentage_basis.is_none() {
        return taffy_layout::Dimension::auto();
    }
    if !value.is_definitely_absolute()
        && value
            .used_length_with_percentage_basis(PercentageBasis::<ContentBoxLength>::indefinite())
            .is_none()
    {
        return taffy_layout::Dimension::auto();
    }
    taffy_dimension_from_length_percentage(value)
}

/// Converts a CSS size constraint to Taffy when intrinsic contributions are known.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
/// intrinsic size keywords. Flex layout has already estimated each flex item's
/// intrinsic contributions before building the Taffy tree, so min/max
/// constraints can be resolved here instead of being dropped:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_intrinsic_dimension_with_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    taffy_intrinsic_dimension_with_basis_and_stretch(
        value,
        percentage_basis,
        min_content,
        max_content,
        FlexStretchFitContext {
            available_margin_box_size: None,
            margin_size: layout_pt(0.0),
            non_content_size: non_content_pt(0.0),
            box_sizing: BoxSizing::ContentBox,
        },
    )
}

pub(super) fn taffy_intrinsic_dimension_with_basis_and_stretch(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    stretch: FlexStretchFitContext,
) -> taffy_layout::Dimension {
    let min_content = min_content.points().max(0.0);
    let max_content = max_content.points().max(min_content);
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::Stretch => taffy_stretch_fit_dimension(stretch),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => {
            taffy_optional_dimension_with_basis(value, percentage_basis)
        }
        css::ComputedLengthPercentageOrAuto::MinContent => {
            taffy_layout::Dimension::length(min_content.max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            taffy_layout::Dimension::length(max_content.max(min_content).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .and_then(|value| {
                    if value.is_definitely_absolute() {
                        Some(value.length_max_zero().points())
                    } else {
                        percentage_basis.points().and_then(|basis| {
                            value
                                .used_length_with_percentage_basis(PercentageBasis::definite(
                                    layout_pt(basis.max(0.0)),
                                ))
                                .map(|length| length.points())
                        })
                    }
                })
                .unwrap_or_else(|| max_content.max(min_content).max(0.0));
            taffy_layout::Dimension::length(
                max_content
                    .max(min_content)
                    .min(stretch.max(min_content))
                    .max(0.0),
            )
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let percentage_basis = percentage_basis.points().unwrap_or(0.0);
            let stretch_size = stretch
                .available_margin_box_size
                .map(|value| value.points())
                .unwrap_or(percentage_basis)
                .max(0.0);
            let fit_content = max_content.min(min_content.max(stretch_size));
            taffy_layout::Dimension::length(
                value
                    .used_value(
                        max_content,
                        min_content,
                        max_content,
                        fit_content,
                        stretch_size,
                        PercentageBasis::definite(layout_pt(percentage_basis)),
                    )
                    .max(layout_pt(0.0))
                    .points(),
            )
        }
    }
}

/// Measures a leaf flex item for Taffy's layout algorithm from intrinsic estimates.
///
/// CSS Flexbox lays out each flex item to determine its flex base size and
/// hypothetical cross size, then later may override known dimensions during
/// line sizing and stretch alignment:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
pub(super) fn measure_flex_item(
    known_dimensions: taffy_layout::Size<Option<f32>>,
    _available_space: taffy_layout::Size<taffy_layout::AvailableSpace>,
    estimate: Option<&mut FlexItemEstimate>,
) -> taffy_layout::Size<f32> {
    let estimate = estimate.cloned().unwrap_or_else(|| {
        FlexItemEstimate::new(
            IntrinsicItemMetrics::zero(),
            FlexItemBaselineEstimate::default(),
        )
    });
    // Taffy receives generic scalar metrics. This is the explicit Flex
    // baseline transport boundary; all Flex recursion retains typed pairs.
    let metrics = estimate.legacy_metrics();
    measure_intrinsic_item_leaf(
        known_dimensions,
        metrics.preferred_aspect_ratio,
        taffy_layout::Size {
            width: metrics.width.points(),
            height: metrics.height.points(),
        },
    )
}

/// Converts a CSS optional size to Taffy's `Dimension`.
///
/// CSS Values defines the `<length-percentage> | auto` shape used by flex item
/// width, height, and flex-basis:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(super) fn taffy_optional_dimension(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::Dimension {
    taffy_flex_optional_dimension(value, FlexTaffyPercentagePolicy::PreservePurePercentage)
}

fn taffy_dimension_from_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::Dimension {
    if let Some(percent) = value
        .pure_percentage_coefficient()
        .filter(|percent| *percent != 0.0)
    {
        taffy_layout::Dimension::percent(percent.max(0.0))
    } else {
        taffy_layout::Dimension::length(value.length_max_zero().points())
    }
}

/// Converts a CSS min-size value for a flex container root.
///
/// CSS Sizing defines the initial `min-width`/`min-height` as `auto`; for a
/// flex container's own used size, that automatic minimum does not become the
/// flex item automatic minimum from Flexbox 4.5, so the root minimum is zero
/// unless the author supplies a definite length/percentage:
/// <https://www.w3.org/TR/css-sizing-3/#min-size-properties> and
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
pub(super) fn taffy_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
) -> taffy_layout::Dimension {
    used_length_percentage_or_auto(value, percentage_basis)
        .map(|length| taffy_layout::Dimension::length(length.points()))
        .unwrap_or_else(|| taffy_layout::Dimension::length(0.0))
}

/// Computes Taffy's automatic minimum size for a flex item.
///
/// CSS Flexbox section 4.5 defines the automatic minimum size of flex items as
/// a content-based minimum size. For flex items with a preferred aspect ratio,
/// that content-based minimum combines the content size suggestion and the
/// transferred size suggestion from CSS Sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>,
/// <https://www.w3.org/TR/css-flexbox-1/#content-based-minimum-size>, and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn flex_min_size_dimension(
    specified: css::ComputedLengthPercentageOrAuto,
    estimated_min_content: ContentBoxLength,
    estimated_max_content: ContentBoxLength,
    context: FlexMinSizeDimensionContext,
) -> taffy_layout::Dimension {
    let Some(automatic_minimum) = resolve_automatic_flex_minimum(
        specified.clone(),
        estimated_min_content,
        estimated_max_content,
        context,
    ) else {
        return taffy_intrinsic_dimension_with_basis_and_stretch(
            specified,
            context.percentage_basis,
            estimated_min_content,
            estimated_max_content,
            context.stretch,
        );
    };
    taffy_layout::Dimension::length(automatic_minimum.used_content_box.points())
}

/// The resolved content-based automatic minimum of one flex-item axis.
///
/// This record is deliberately shared by the Taffy adapter and Quire's
/// post-layout safeguard.  CSS Flexbox defines a single automatic-minimum
/// decision; keeping its suggestions and selected content-box result together
/// prevents either consumer from silently selecting a different minimum.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
#[derive(Debug, Clone, Copy)]
pub(super) struct AutomaticFlexMinimum {
    pub(super) content_size_suggestion: ContentBoxLength,
    pub(super) transferred_size_suggestion: Option<ContentBoxLength>,
    pub(super) specified_size_suggestion: Option<ContentBoxLength>,
    pub(super) used_content_box: ContentBoxLength,
}

impl AutomaticFlexMinimum {
    pub(super) fn from_suggestions(
        content_size_suggestion: ContentBoxLength,
        transferred_size_suggestion: Option<ContentBoxLength>,
        specified_size_suggestion: Option<ContentBoxLength>,
        is_replaced: bool,
    ) -> Self {
        let mut used_content_box = content_size_suggestion.max(content_box_pt(0.0));
        if let Some(transferred) = transferred_size_suggestion {
            let transferred = transferred.max(content_box_pt(0.0));
            used_content_box = if is_replaced {
                used_content_box.min(transferred)
            } else {
                used_content_box.max(transferred)
            };
        }
        if let Some(specified) = specified_size_suggestion {
            used_content_box = used_content_box.min(specified.max(content_box_pt(0.0)));
        }
        Self {
            content_size_suggestion,
            transferred_size_suggestion,
            specified_size_suggestion,
            used_content_box,
        }
    }

    pub(super) fn debug_assert_consistent(self, is_replaced: bool) {
        debug_assert!(self.used_content_box >= content_box_pt(0.0));
        if let Some(specified) = self.specified_size_suggestion {
            debug_assert!(self.used_content_box <= specified.max(content_box_pt(0.0)));
        }
        if let Some(transferred) = self.transferred_size_suggestion {
            let transferred = transferred.max(content_box_pt(0.0));
            if is_replaced {
                debug_assert!(self.used_content_box <= transferred);
            } else {
                debug_assert!(
                    self.used_content_box >= self.content_size_suggestion.max(content_box_pt(0.0))
                );
            }
        }
    }
}

/// Resolve Flexbox's automatic main-axis minimum, if this axis uses one.
///
/// A cross-axis automatic minimum remains `auto` for Taffy, and a scrollable
/// main-axis item has a zero automatic minimum.  The returned value is always
/// a non-negative content-box length; callers perform their explicit
/// content-to-border-box conversion at their own backend boundary.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
pub(super) fn resolve_automatic_flex_minimum(
    specified: css::ComputedLengthPercentageOrAuto,
    estimated_min_content: ContentBoxLength,
    estimated_max_content: ContentBoxLength,
    context: FlexMinSizeDimensionContext,
) -> Option<AutomaticFlexMinimum> {
    if !min_size_uses_automatic_flex_minimum(specified.clone(), context.is_item_block_axis)
        || !context.is_main_axis
    {
        return None;
    }
    if context.overflow.is_scrollable() {
        return Some(AutomaticFlexMinimum {
            content_size_suggestion: content_box_pt(0.0),
            transferred_size_suggestion: None,
            specified_size_suggestion: None,
            used_content_box: content_box_pt(0.0),
        });
    }
    // CSS Flexbox 4.5: non-scrollable flex items use the content-based
    // minimum size in the main axis, capped by a definite preferred main
    // size. Cross-axis auto minimums remain automatic.
    // The estimate's minimum contribution is normally the min-content
    // contribution. Its `min-*` constraint has already been folded into that
    // estimate, however, so `calc-size(auto, …)` must peel back that raised
    // floor before substituting `auto`; otherwise a value such as `size +
    // 20px` is applied twice.
    // <https://drafts.csswg.org/css-values-5/#calc-size>.
    let content_size_suggestion = if specified.calc_size_with_auto_basis().is_some() {
        estimated_min_content.min(estimated_max_content)
    } else {
        estimated_min_content
    };
    let selection = AutomaticFlexMinimum::from_suggestions(
        content_size_suggestion,
        context.transferred_size_suggestion,
        context.definite_preferred_content_size,
        context.is_replaced,
    );
    selection.debug_assert_consistent(context.is_replaced);
    let automatic_minimum = selection.used_content_box.points();
    let automatic_minimum = specified
        .calc_size_with_auto_basis()
        .map(|value| {
            value
                .used_value(
                    automatic_minimum,
                    content_size_suggestion.points(),
                    estimated_max_content.points(),
                    automatic_minimum,
                    context
                        .stretch
                        .available_margin_box_size
                        .map(SemanticLengthExt::points)
                        .unwrap_or(0.0),
                    PercentageBasis::definite(layout_pt(
                        context.percentage_basis.points().unwrap_or(0.0),
                    )),
                )
                .points()
        })
        .unwrap_or(automatic_minimum);
    Some(AutomaticFlexMinimum {
        content_size_suggestion,
        transferred_size_suggestion: context.transferred_size_suggestion,
        specified_size_suggestion: context.definite_preferred_content_size,
        used_content_box: content_box_pt(automatic_minimum.max(0.0)),
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexMinSizeDimensionContext {
    pub(super) definite_preferred_content_size: Option<ContentBoxLength>,
    pub(super) transferred_size_suggestion: Option<ContentBoxLength>,
    pub(super) is_replaced: bool,
    pub(super) is_main_axis: bool,
    /// CSS Sizing treats a `min-content` minimum in the block axis as the
    /// automatic minimum. This remains an intrinsic minimum in the inline
    /// axis:
    /// <https://www.w3.org/TR/css-sizing-3/#valdef-width-min-content>.
    pub(super) is_item_block_axis: bool,
    pub(super) overflow: css::Overflow,
    pub(super) percentage_basis: FlexAvailablePercentageBasis,
    pub(super) stretch: FlexStretchFitContext,
}

/// Returns whether a flex min-size value uses the automatic minimum algorithm.
///
/// The CSS Sizing block-axis `min-content` minimum aliases the automatic
/// minimum. Writing Modes determines which physical dimension is the item's
/// block axis:
/// <https://www.w3.org/TR/css-sizing-3/#valdef-width-min-content> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn flex_min_size_uses_automatic_minimum(
    value: css::ComputedLengthPercentageOrAuto,
    writing_mode: WritingMode,
    physical_axis: FlexDirection,
) -> bool {
    let is_block_axis = match WritingModeAxes::new(writing_mode, Direction::Ltr)
        .physical_axis(LogicalAxis::Block)
    {
        PhysicalAxis::Horizontal => physical_axis.is_row_axis(),
        PhysicalAxis::Vertical => physical_axis.is_column_axis(),
    };
    value.is_auto()
        || value.calc_size_with_auto_basis().is_some()
        || (matches!(value, css::ComputedLengthPercentageOrAuto::MinContent) && is_block_axis)
}

fn min_size_uses_automatic_flex_minimum(
    value: css::ComputedLengthPercentageOrAuto,
    is_item_block_axis: bool,
) -> bool {
    value.is_auto()
        || value.calc_size_with_auto_basis().is_some()
        || (is_item_block_axis && matches!(value, css::ComputedLengthPercentageOrAuto::MinContent))
}

/// Returns the overflow value for the flex item's main axis.
///
/// CSS Flexbox resolves automatic minimum sizes on the flex main axis, and CSS
/// Overflow exposes independent inline/block overflow controls through
/// `overflow-x` and `overflow-y`:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
pub(super) fn flex_item_main_axis_overflow(
    style: &ComputedStyle,
    direction: FlexDirection,
) -> css::Overflow {
    if direction.is_row_axis() {
        style.overflow_x
    } else {
        style.overflow_y
    }
}

/// Computes the Taffy `flex-basis` dimension from CSS flex and main-size values.
///
/// CSS Flexbox defines `flex-basis:auto` as retrieving the main-size property
/// and falling back to content sizing. Percentages resolve against the flex
/// container's inner main size, and if that size is indefinite the used value
/// is content:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(super) fn resolve_taffy_flex_basis(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    context: FlexBasisContext,
) -> ResolvedFlexBasis {
    match &style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length) => {
            let main_size_basis = context.main_size_basis.points();
            if length.contains_percentage() && main_size_basis.is_none() {
                return ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                    flex_auto_content_basis(
                        style,
                        if context.direction.is_row_axis() {
                            estimate.content_width
                        } else {
                            estimate.content_height
                        },
                        context.direction,
                    )
                    .points(),
                ));
            }
            return ResolvedFlexBasis::definite_flex_basis(taffy_optional_dimension_with_basis(
                css::ComputedLengthPercentageOrAuto::LengthPercentage(length.value.clone()),
                context.main_size_basis,
            ));
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => {
            let fallback = if context.direction.is_row_axis() {
                estimate.content_width
            } else {
                estimate.content_height
            };
            return aspect_ratio_transferred_flex_basis(
                style,
                estimate,
                context.direction,
                context.available_cross_size,
                context.stretched_cross_size,
                context.preferred_aspect_ratio,
            )
            .map(|basis| {
                ResolvedFlexBasis::aspect_ratio_transfer(taffy_layout::Dimension::length(
                    basis.points(),
                ))
            })
            .unwrap_or_else(|| {
                ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                    flex_auto_content_basis(style, fallback, context.direction).points(),
                ))
            });
        }
        css::ComputedFlexBasis::MinContent => {
            return ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                flex_auto_content_basis(
                    style,
                    if context.direction.is_row_axis() {
                        estimate.min_width
                    } else {
                        estimate.min_height
                    },
                    context.direction,
                )
                .points(),
            ));
        }
        css::ComputedFlexBasis::FitContent(limit) => {
            let min_content = if context.direction.is_row_axis() {
                estimate.min_width
            } else {
                estimate.min_height
            };
            let max_content = if context.direction.is_row_axis() {
                estimate.content_width
            } else {
                estimate.content_height
            };
            let limit = limit
                .clone()
                .and_then(|limit| {
                    if limit.is_definitely_absolute() {
                        Some(content_box_pt(limit.length_max_zero().points()))
                    } else {
                        context.main_size_basis.points().and_then(|basis| {
                            limit
                                .used_length_with_percentage_basis(PercentageBasis::definite(
                                    layout_pt(basis),
                                ))
                                .map(|length| content_box_pt(length.points()))
                        })
                    }
                })
                .unwrap_or_else(|| flex_main_content_box_length(context.available_main_size));
            return ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                flex_auto_content_basis(
                    style,
                    fit_content_basis(min_content, max_content, limit),
                    context.direction,
                )
                .points(),
            ));
        }
        css::ComputedFlexBasis::Auto => {}
    }

    // CSS Flexbox 7.2.3: `flex-basis:auto` retrieves the main-size property,
    // and if that is also auto the used flex basis is `content`. CSS Flexbox
    // 9.2 transfers a preferred aspect ratio through a definite cross size
    // before falling back to content sizing.
    if context.direction.is_row_axis() {
        if !style.box_values.width.is_auto() {
            let dimension = taffy_flex_basis_from_main_size(
                style,
                style.box_values.width.clone(),
                estimate,
                context.main_size_basis,
                FlexDirection::Row,
            );
            if main_size_property_needs_content_fallback(
                &style.box_values.width,
                context.main_size_basis,
            ) {
                ResolvedFlexBasis::normal_flow_content(dimension)
            } else {
                ResolvedFlexBasis::main_size_property(dimension)
            }
        } else if let Some(transferred) = aspect_ratio_transferred_flex_basis(
            style,
            estimate,
            context.direction,
            context.available_cross_size,
            context.stretched_cross_size,
            context.preferred_aspect_ratio,
        ) {
            ResolvedFlexBasis::aspect_ratio_transfer(taffy_layout::Dimension::length(
                transferred.points(),
            ))
        } else {
            ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                flex_auto_content_basis(style, estimate.content_width, FlexDirection::Row).points(),
            ))
        }
    } else if !style.box_values.height.is_auto() {
        let dimension = taffy_flex_basis_from_main_size(
            style,
            style.box_values.height.value().clone(),
            estimate,
            context.main_size_basis,
            FlexDirection::Column,
        );
        if main_size_property_needs_content_fallback(
            style.box_values.height.value(),
            context.main_size_basis,
        ) {
            ResolvedFlexBasis::normal_flow_content(dimension)
        } else {
            ResolvedFlexBasis::main_size_property(dimension)
        }
    } else if let Some(transferred) = aspect_ratio_transferred_flex_basis(
        style,
        estimate,
        context.direction,
        context.available_cross_size,
        context.stretched_cross_size,
        context.preferred_aspect_ratio,
    ) {
        ResolvedFlexBasis::aspect_ratio_transfer(taffy_layout::Dimension::length(
            transferred.points(),
        ))
    } else {
        ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
            flex_auto_content_basis(style, estimate.content_height, FlexDirection::Column).points(),
        ))
    }
}

fn main_size_property_needs_content_fallback(
    main_size: &css::ComputedLengthPercentageOrAuto,
    main_size_basis: FlexAvailablePercentageBasis,
) -> bool {
    matches!(main_size, css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
        if value.needs_percentage_basis() && !main_size_basis.is_definite())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexBasisContext {
    pub(super) direction: FlexDirection,
    pub(super) available_main_size: FlexMainSize,
    pub(super) available_cross_size: Option<FlexCrossSize>,
    pub(super) stretched_cross_size: Option<FlexCrossSize>,
    pub(super) main_size_basis: FlexAvailablePercentageBasis,
    pub(super) preferred_aspect_ratio: Option<f32>,
}

/// The CSS rule that supplied a resolved flex basis.
///
/// This remains separate from Taffy's scalar basis through final flex
/// geometry. A later normal-flow measurement may replace only a genuinely
/// content-derived provisional main span; neither an aspect-ratio transfer nor
/// a retrieved main-size property may be mistaken for that case.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexMainSizeProvenance {
    NormalFlowContent,
    AspectRatioTransfer,
    MainSizeProperty,
    DefiniteFlexBasis,
}

impl FlexMainSizeProvenance {
    pub(super) fn permits_final_normal_flow_block_span(self) -> bool {
        matches!(self, Self::NormalFlowContent)
    }
}

/// A Taffy basis and the CSS sizing rule that produced it.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedFlexBasis {
    pub(super) dimension: taffy_layout::Dimension,
    pub(super) provenance: FlexMainSizeProvenance,
}

impl ResolvedFlexBasis {
    fn normal_flow_content(dimension: taffy_layout::Dimension) -> Self {
        Self {
            dimension,
            provenance: FlexMainSizeProvenance::NormalFlowContent,
        }
    }

    fn aspect_ratio_transfer(dimension: taffy_layout::Dimension) -> Self {
        Self {
            dimension,
            provenance: FlexMainSizeProvenance::AspectRatioTransfer,
        }
    }

    fn main_size_property(dimension: taffy_layout::Dimension) -> Self {
        Self {
            dimension,
            provenance: FlexMainSizeProvenance::MainSizeProperty,
        }
    }

    fn definite_flex_basis(dimension: taffy_layout::Dimension) -> Self {
        Self {
            dimension,
            provenance: FlexMainSizeProvenance::DefiniteFlexBasis,
        }
    }
}

/// Computes the flex base-size transfer from a definite cross size.
///
/// Flexbox section 9.2 lets a flex item with a preferred aspect ratio use a
/// definite cross size to resolve its flex base size before falling back to
/// content sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
fn aspect_ratio_transferred_flex_basis(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
    available_cross_size: Option<FlexCrossSize>,
    stretched_cross_size: Option<FlexCrossSize>,
    preferred_aspect_ratio: Option<f32>,
) -> Option<LayoutLength> {
    aspect_ratio_transferred_content_main_size_with_cross_constraints(
        style,
        direction,
        available_cross_size,
        stretched_cross_size,
        preferred_aspect_ratio,
        if direction.is_row_axis() {
            estimate.min_height
        } else {
            estimate.min_width
        },
        if direction.is_row_axis() {
            estimate.content_height
        } else {
            estimate.content_width
        },
    )
    .map(|size| flex_aspect_ratio_basis_from_content_box(style, size, direction))
}

/// Computes a transferred main size after cross-axis min/max constraints.
///
/// Flexbox uses the item's used definite cross size for aspect-ratio transfer,
/// so min/max constraints apply before the transfer. Intrinsic constraint
/// keywords use the already-measured cross-axis contributions:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
pub(super) fn aspect_ratio_transferred_content_main_size_with_cross_constraints(
    style: &ComputedStyle,
    direction: FlexDirection,
    available_cross_size: Option<FlexCrossSize>,
    stretched_cross_size: Option<FlexCrossSize>,
    preferred_aspect_ratio: Option<f32>,
    cross_min_content: ContentBoxLength,
    cross_max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let ratio = preferred_aspect_ratio?;
    let transferred = aspect_ratio_transferred_content_main_size(
        style,
        direction,
        available_cross_size,
        stretched_cross_size,
        Some(ratio),
    )?;

    // The definite cross size used for aspect-ratio transfer is a used size:
    // it is constrained by the item's cross-axis min/max properties before it
    // becomes a flex base size.  Keep the conversion in content-box space so
    // `box-sizing` is applied consistently to the preferred and constraint
    // sizes. CSS Flexbox 9.2 resolves the resulting cross size before deriving
    // the flex base size through the preferred aspect ratio:
    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
    let unconstrained_cross =
        flex_aspect_ratio_transferred_cross_content_size(style, transferred, direction, ratio);
    let cross_percentage_basis =
        percentage_basis_from_points(available_cross_size.map(FlexCrossSize::points));
    let constrained_cross = if direction.is_row_axis() {
        constrain_height_with_intrinsic(
            style,
            unconstrained_cross,
            cross_min_content,
            cross_max_content,
            cross_percentage_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
    } else {
        constrain_width_with_intrinsic(
            style,
            unconstrained_cross,
            cross_min_content,
            cross_max_content,
            cross_percentage_basis,
            non_content_pt(
                style.padding.left + style.padding.right + horizontal_border_width(style),
            ),
        )
    };
    Some(flex_aspect_ratio_transferred_content_main_size(
        style,
        constrained_cross,
        direction,
        ratio,
    ))
}

/// Computes the transferred suggestion used by Flexbox's automatic minimum.
///
/// A definite preferred cross size supplies the normal transferred
/// suggestion. When that size is automatic, the automatic-minimum algorithm
/// still applies a definite cross-axis minimum before transferring the
/// preferred ratio. This is distinct from flex-base sizing: it must not make
/// an automatic cross size definite for the main flex basis.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn automatic_minimum_transferred_size_suggestion(
    style: &ComputedStyle,
    direction: FlexDirection,
    available_cross_size: Option<FlexCrossSize>,
    stretched_cross_size: Option<FlexCrossSize>,
    preferred_aspect_ratio: Option<f32>,
    cross_min_content: ContentBoxLength,
    cross_max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    if let Some(transferred) = aspect_ratio_transferred_content_main_size_with_cross_constraints(
        style,
        direction,
        available_cross_size,
        stretched_cross_size,
        preferred_aspect_ratio,
        cross_min_content,
        cross_max_content,
    ) {
        return Some(transferred);
    }

    let ratio = preferred_aspect_ratio?;
    let percentage_basis =
        percentage_basis_from_points(available_cross_size.map(FlexCrossSize::points));
    let constrained_minimum_cross = if direction.is_row_axis() {
        constrain_height_with_intrinsic(
            style,
            content_box_pt(0.0),
            cross_min_content,
            cross_max_content,
            percentage_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
    } else {
        constrain_width_with_intrinsic(
            style,
            content_box_pt(0.0),
            cross_min_content,
            cross_max_content,
            percentage_basis,
            non_content_pt(
                style.padding.left + style.padding.right + horizontal_border_width(style),
            ),
        )
    };
    (constrained_minimum_cross.points() > 0.0).then(|| {
        flex_aspect_ratio_transferred_content_main_size(
            style,
            constrained_minimum_cross,
            direction,
            ratio,
        )
    })
}

/// Computes the content-box transferred size suggestion for a flex item's main axis.
///
/// CSS Flexbox 4.5 and 9.2 both use CSS Sizing preferred aspect ratios to
/// transfer a definite cross size into a main-axis content size. The flex basis
/// adds border/padding separately, but automatic minimum size calculations use
/// this content-box suggestion directly:
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn aspect_ratio_transferred_content_main_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    available_cross_size: Option<FlexCrossSize>,
    stretched_cross_size: Option<FlexCrossSize>,
    preferred_aspect_ratio: Option<f32>,
) -> Option<ContentBoxLength> {
    let ratio = preferred_aspect_ratio?;
    if direction.is_row_axis() {
        let cross_non_content =
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
        let cross_content_height = used_content_box_height_or_auto_with_basis(
            style,
            percentage_basis_from_points(available_cross_size.map(FlexCrossSize::points)),
            cross_non_content,
        )
        .or_else(|| {
            stretched_cross_size
                .map(|size| content_box_pt((size.points() - cross_non_content.points()).max(0.0)))
        })?;
        Some(flex_aspect_ratio_transferred_content_main_size(
            style,
            cross_content_height,
            direction,
            ratio,
        ))
    } else {
        let cross_non_content = non_content_pt(
            style.padding.left + style.padding.right + horizontal_border_width(style),
        );
        let cross_content_width = used_content_box_width_or_auto_with_basis(
            style,
            percentage_basis_from_points(available_cross_size.map(FlexCrossSize::points)),
            cross_non_content,
        )
        .or_else(|| {
            stretched_cross_size
                .map(|size| content_box_pt((size.points() - cross_non_content.points()).max(0.0)))
        })?;
        Some(flex_aspect_ratio_transferred_content_main_size(
            style,
            cross_content_width,
            direction,
            ratio,
        ))
    }
}

fn taffy_flex_basis_from_main_size(
    style: &ComputedStyle,
    value: css::ComputedLengthPercentageOrAuto,
    estimate: &FlexItemEstimate,
    main_size_basis: FlexAvailablePercentageBasis,
    direction: FlexDirection,
) -> taffy_layout::Dimension {
    let (min_content, max_content) = if direction.is_row_axis() {
        (estimate.min_width, estimate.content_width)
    } else {
        (estimate.min_height, estimate.content_height)
    };
    if matches!(value, css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value) if value.needs_percentage_basis() && !main_size_basis.is_definite())
    {
        return taffy_layout::Dimension::length(
            flex_auto_content_basis(style, max_content, direction).points(),
        );
    }

    taffy_intrinsic_dimension_with_basis(value, main_size_basis, min_content, max_content)
}

/// Computes the intrinsic `fit-content` size clamp for `flex-basis`.
///
/// CSS Sizing defines fit-content as
/// `min(max-content, max(min-content, stretch-or-argument))`; Flexbox accepts
/// that width grammar for `flex-basis`:
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
fn fit_content_basis(
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    limit: ContentBoxLength,
) -> ContentBoxLength {
    content_box_pt(
        max_content
            .points()
            .max(0.0)
            .min(min_content.points().max(0.0).max(limit.points().max(0.0))),
    )
}

/// Transfer a flex item's cross-axis content size through its preferred ratio.
///
/// The result remains in content-box space for the flex sizing algorithm. A
/// bare ratio on a border-box item instead operates on border-box dimensions,
/// so convert into and then out of that coordinate space at this boundary.
/// `auto <ratio>` always remains content-box based.
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
pub(in crate::layout::flex) fn flex_aspect_ratio_transferred_content_main_size(
    style: &ComputedStyle,
    cross_content_size: ContentBoxLength,
    direction: FlexDirection,
    ratio: f32,
) -> ContentBoxLength {
    let uses_content_box = style.aspect_ratio.uses_content_box_for_non_replaced()
        || style.box_sizing == BoxSizing::ContentBox;
    if uses_content_box {
        if direction.is_row_axis() {
            content_box_pt(cross_content_size.points() * ratio)
        } else {
            content_box_pt(cross_content_size.points() / ratio)
        }
    } else {
        let cross_border_box =
            cross_content_size.points() + cross_axis_extras(style, direction).points();
        let main_border_box = if direction.is_row_axis() {
            cross_border_box * ratio
        } else {
            cross_border_box / ratio
        };
        content_box_pt((main_border_box - main_axis_extras(style, direction).points()).max(0.0))
    }
}

/// Recover the cross-axis content size from a ratio-transferred main content
/// size.
///
/// This is the inverse of `flex_aspect_ratio_transferred_content_main_size`.
/// CSS Flexbox applies cross-axis min/max constraints between these two
/// transformations, so a bare ratio must return through the box selected by
/// `box-sizing` rather than invert directly in content-box space:
/// <https://drafts.csswg.org/css-flexbox-1/#algo-main-item> and
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>.
fn flex_aspect_ratio_transferred_cross_content_size(
    style: &ComputedStyle,
    main_content_size: ContentBoxLength,
    direction: FlexDirection,
    ratio: f32,
) -> ContentBoxLength {
    let uses_content_box = style.aspect_ratio.uses_content_box_for_non_replaced()
        || style.box_sizing == BoxSizing::ContentBox;
    if uses_content_box {
        if direction.is_row_axis() {
            content_box_pt(main_content_size.points() / ratio)
        } else {
            content_box_pt(main_content_size.points() * ratio)
        }
    } else {
        let main_border_box =
            main_content_size.points() + main_axis_extras(style, direction).points();
        let cross_border_box = if direction.is_row_axis() {
            main_border_box / ratio
        } else {
            main_border_box * ratio
        };
        content_box_pt((cross_border_box - cross_axis_extras(style, direction).points()).max(0.0))
    }
}

fn main_axis_extras(style: &ComputedStyle, direction: FlexDirection) -> NonContentLength {
    let border_widths = used_border_widths(style);
    non_content_pt(if direction.is_row_axis() {
        style.padding.left + style.padding.right + border_widths.left + border_widths.right
    } else {
        style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom
    })
}

fn cross_axis_extras(style: &ComputedStyle, direction: FlexDirection) -> NonContentLength {
    let border_widths = used_border_widths(style);
    non_content_pt(if direction.is_row_axis() {
        style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom
    } else {
        style.padding.left + style.padding.right + border_widths.left + border_widths.right
    })
}

/// Computes the content-derived basis used when `flex-basis:auto` has no main size.
///
/// CSS Flexbox defines content-based flex basis resolution for `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(super) fn flex_auto_content_basis(
    style: &ComputedStyle,
    length: ContentBoxLength,
    direction: FlexDirection,
) -> LayoutLength {
    flex_auto_content_basis_from_content_box(style, length, direction)
}

fn flex_auto_content_basis_from_content_box(
    style: &ComputedStyle,
    length: ContentBoxLength,
    direction: FlexDirection,
) -> LayoutLength {
    // The CSS value is the content size. The intrinsic estimator and line
    // breaker both shape text, but through different APIs; round up so a tiny
    // metric disagreement does not create an avoidable flex-item wrap in
    // preserved-newline content such as `white-space: pre-line` address blocks.
    let length = content_box_pt(if style.white_space.preserves_newlines() {
        length.points().max(0.0).ceil() + style.font_size.ceil()
    } else {
        length.points().max(0.0)
    });
    if style.box_sizing == BoxSizing::BorderBox {
        crate::units::IntoLayoutLength::into_layout_length(content_box_to_border_box_length(
            length,
            main_axis_extras(style, direction),
        ))
    } else {
        crate::units::IntoLayoutLength::into_layout_length(length)
    }
}

/// Convert an aspect-ratio-derived content size into a flex main-size basis.
///
/// A bare `aspect-ratio` works in the box selected by `box-sizing`, but
/// `auto <ratio>` always works in the content box. Taffy applies the item's
/// `box-sizing` when resolving its flex basis, so retain a content-box basis
/// for `content-box` items and convert only `border-box` items.
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
fn flex_aspect_ratio_basis_from_content_box(
    style: &ComputedStyle,
    length: ContentBoxLength,
    direction: FlexDirection,
) -> LayoutLength {
    let length = content_box_pt(length.points().max(0.0));
    if style.box_sizing == BoxSizing::BorderBox {
        crate::units::IntoLayoutLength::into_layout_length(content_box_to_border_box_length(
            length,
            main_axis_extras(style, direction),
        ))
    } else {
        crate::units::IntoLayoutLength::into_layout_length(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_margin_adapter_preserves_negative_lengths_and_percentages() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(-50.0),
        );
        let length = taffy_bridge::margin(
            &style,
            PercentageBasis::<LogicalInlineContentSize>::indefinite(),
            taffy_bridge::TaffyCyclicPercentage::PreservePurePercentage,
        )
        .left;
        assert_eq!(length.resolve_to_option(200.0, |_, _| 0.0), Some(-50.0));

        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(-0.25),
        );
        let percentage = taffy_bridge::margin(
            &style,
            PercentageBasis::<LogicalInlineContentSize>::indefinite(),
            taffy_bridge::TaffyCyclicPercentage::PreservePurePercentage,
        )
        .left;
        assert_eq!(percentage.resolve_to_option(200.0, |_, _| 0.0), Some(-50.0));

        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::Auto;
        let auto = taffy_bridge::margin(
            &style,
            PercentageBasis::<LogicalInlineContentSize>::indefinite(),
            taffy_bridge::TaffyCyclicPercentage::PreservePurePercentage,
        )
        .left;
        assert!(auto.is_auto());
    }

    #[test]
    fn flex_size_adapter_still_clamps_negative_lengths_and_percentages() {
        assert_eq!(
            taffy_dimension_from_length_percentage(css::ComputedLengthPercentage::from_points(
                -50.0
            )),
            taffy_layout::Dimension::length(0.0)
        );
        assert_eq!(
            taffy_dimension_from_length_percentage(css::ComputedLengthPercentage::from_percent(
                -0.25
            )),
            taffy_layout::Dimension::percent(0.0)
        );
    }

    #[test]
    fn flex_optional_dimension_policies_preserve_or_resolve_pure_percentages() {
        let percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        let definite_basis = flex_available_percentage_basis_from_points(
            Some(80.0),
            FlexAvailableSizeSource::ContainingBlock,
        );
        let indefinite_basis = PercentageBasis::indefinite();

        assert_eq!(
            taffy_optional_dimension(percent.clone()),
            taffy_layout::Dimension::percent(0.5),
        );
        assert_eq!(
            taffy_optional_dimension_with_basis(percent.clone(), definite_basis),
            taffy_layout::Dimension::length(40.0),
        );
        assert!(taffy_optional_dimension_with_basis(percent, indefinite_basis).is_auto());
    }

    #[test]
    fn flex_optional_dimension_policies_share_fixed_lengths() {
        let fixed = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(12.0),
        );
        let indefinite_basis = PercentageBasis::indefinite();

        assert_eq!(
            taffy_optional_dimension(fixed.clone()),
            taffy_layout::Dimension::length(12.0),
        );
        assert_eq!(
            taffy_optional_dimension_with_basis(fixed, indefinite_basis),
            taffy_layout::Dimension::length(12.0),
        );
    }

    #[test]
    fn flex_size_adapter_does_not_treat_unresolved_metric_math_as_a_fixed_length() {
        let value = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_points(12.0),
                css::ComputedLengthPercentage::from_em(1.0),
            ),
        );

        assert!(
            taffy_optional_dimension_with_basis(value, PercentageBasis::indefinite()).is_auto()
        );
    }

    #[test]
    fn unresolved_percentage_main_size_remains_auto_for_content_flex_basis() {
        let value = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.01),
        );
        let context = FlexItemSizeDimensionContext {
            flex_direction: FlexDirection::Column,
            dimension_axis: FlexDirection::Column,
            percentage_basis: PercentageBasis::indefinite(),
            stretch: FlexStretchFitContext {
                available_margin_box_size: None,
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: BoxSizing::ContentBox,
            },
            flex_basis_overrides_main_size: false,
            auto_cross_uses_stretch_fit: false,
            auto_cross_fit_content: None,
        };

        assert!(
            flex_item_size_dimension(
                value,
                content_box_pt(10.0),
                content_box_pt(10.0),
                content_box_pt(20.0),
                context,
            )
            .is_auto()
        );
    }

    #[test]
    fn flex_container_min_dimension_uses_the_supplied_physical_basis() {
        let fifty_percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        let width_basis = flex_available_percentage_basis_from_points(
            Some(200.0),
            FlexAvailableSizeSource::ContainingBlock,
        );
        let height_basis = flex_available_percentage_basis_from_points(
            Some(80.0),
            FlexAvailableSizeSource::ContainingBlock,
        );

        assert_eq!(
            taffy_min_dimension(fifty_percent.clone(), width_basis),
            taffy_layout::Dimension::length(100.0)
        );
        assert_eq!(
            taffy_min_dimension(fifty_percent, height_basis),
            taffy_layout::Dimension::length(40.0)
        );
    }

    #[test]
    fn flex_gap_resolves_percentages_only_with_definite_basis() {
        let percent_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.5));
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(4.0), 0.5, true);
        let mixed_gap = css::ComputedGap::LengthPercentage(mixed);
        let indefinite_basis = PercentageBasis::indefinite();
        let definite_basis = flex_available_percentage_basis_from_points(
            Some(40.0),
            FlexAvailableSizeSource::ContainingBlock,
        );

        assert_eq!(
            taffy_gap(percent_gap.clone(), indefinite_basis),
            taffy_layout::LengthPercentage::length(0.0)
        );
        assert_eq!(
            taffy_gap(percent_gap, definite_basis),
            taffy_layout::LengthPercentage::length(20.0)
        );
        assert_eq!(
            taffy_gap(mixed_gap.clone(), indefinite_basis),
            taffy_layout::LengthPercentage::length(4.0)
        );
        assert_eq!(
            taffy_gap(mixed_gap, definite_basis),
            taffy_layout::LengthPercentage::length(24.0)
        );
    }

    fn test_flex_basis_context() -> FlexBasisContext {
        FlexBasisContext {
            direction: FlexDirection::Row,
            available_main_size: FlexMainSize::new(200.0),
            available_cross_size: None,
            stretched_cross_size: None,
            main_size_basis: PercentageBasis::definite_from(
                content_box_pt(200.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            preferred_aspect_ratio: None,
        }
    }

    fn test_flex_estimate() -> FlexItemEstimate {
        FlexItemEstimate::new(
            IntrinsicItemMetrics {
                width: content_box_pt(60.0),
                height: content_box_pt(30.0),
                min_width: content_box_pt(20.0),
                min_height: content_box_pt(10.0),
                content_width: content_box_pt(80.0),
                content_height: content_box_pt(40.0),
                preferred_aspect_ratio: None,
                first_baseline: None,
                last_baseline: None,
            },
            FlexItemBaselineEstimate::default(),
        )
    }

    #[test]
    fn flex_item_measurement_extracts_typed_content_box_lengths() {
        let mut estimate = test_flex_estimate();

        let measured = measure_flex_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(200.0),
                height: taffy_layout::AvailableSpace::Definite(200.0),
            },
            Some(&mut estimate),
        );

        assert_eq!(measured.width, 60.0);
        assert_eq!(measured.height, 30.0);
    }

    #[test]
    fn flex_basis_uses_typed_content_and_min_content_estimates() {
        let estimate = test_flex_estimate();
        let mut style = ComputedStyle::initial();

        style.flex_basis = css::ComputedFlexBasis::Content;
        assert_eq!(
            resolve_taffy_flex_basis(&style, &estimate, test_flex_basis_context()).dimension,
            taffy_layout::Dimension::length(80.0)
        );

        style.flex_basis = css::ComputedFlexBasis::MinContent;
        assert_eq!(
            resolve_taffy_flex_basis(&style, &estimate, test_flex_basis_context()).dimension,
            taffy_layout::Dimension::length(20.0)
        );

        style.flex_basis = css::ComputedFlexBasis::MaxContent;
        assert_eq!(
            resolve_taffy_flex_basis(&style, &estimate, test_flex_basis_context()).dimension,
            taffy_layout::Dimension::length(80.0)
        );
    }

    #[test]
    fn flex_basis_indefinite_percentage_falls_back_to_typed_content_estimate() {
        let estimate = test_flex_estimate();
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::LengthPercentage(
            css::ComputedFlexBasisLength::new(css::ComputedLengthPercentage::from_percent(0.5)),
        );
        let context = FlexBasisContext {
            main_size_basis: PercentageBasis::indefinite(),
            ..test_flex_basis_context()
        };

        let resolved = resolve_taffy_flex_basis(&style, &estimate, context);
        assert_eq!(resolved.dimension, taffy_layout::Dimension::length(80.0));
        assert_eq!(
            resolved.provenance,
            FlexMainSizeProvenance::NormalFlowContent,
            "an indefinite percentage flex basis, including flex: 1's 0%, uses content sizing",
        );
    }

    #[test]
    fn resolved_flex_basis_retains_the_main_size_provenance() {
        let estimate = test_flex_estimate();
        let mut content = ComputedStyle::initial();
        content.flex_basis = css::ComputedFlexBasis::Content;
        assert_eq!(
            resolve_taffy_flex_basis(&content, &estimate, test_flex_basis_context()).provenance,
            FlexMainSizeProvenance::NormalFlowContent,
        );

        let mut main_size_property = ComputedStyle::initial();
        main_size_property.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(60.0),
        );
        assert_eq!(
            resolve_taffy_flex_basis(&main_size_property, &estimate, test_flex_basis_context(),)
                .provenance,
            FlexMainSizeProvenance::MainSizeProperty,
        );

        let mut definite_basis = ComputedStyle::initial();
        definite_basis.flex_basis = css::ComputedFlexBasis::LengthPercentage(
            css::ComputedFlexBasisLength::new(css::ComputedLengthPercentage::from_points(60.0)),
        );
        assert_eq!(
            resolve_taffy_flex_basis(&definite_basis, &estimate, test_flex_basis_context())
                .provenance,
            FlexMainSizeProvenance::DefiniteFlexBasis,
        );

        let mut transferred = ComputedStyle::initial();
        transferred.flex_basis = css::ComputedFlexBasis::Content;
        transferred.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(80.0),
        );
        let context = FlexBasisContext {
            direction: FlexDirection::Column,
            available_cross_size: Some(FlexCrossSize::new(80.0)),
            preferred_aspect_ratio: Some(1.0),
            ..test_flex_basis_context()
        };
        assert_eq!(
            resolve_taffy_flex_basis(&transferred, &estimate, context).provenance,
            FlexMainSizeProvenance::AspectRatioTransfer,
        );
    }

    #[test]
    fn automatic_minimum_transfers_a_definite_cross_minimum_through_ratio() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(30.0),
        );

        let transferred = automatic_minimum_transferred_size_suggestion(
            &style,
            FlexDirection::Row,
            None,
            None,
            Some(1.0),
            content_box_pt(10.0),
            content_box_pt(100.0),
        )
        .expect("a definite cross minimum supplies a transferred suggestion");

        assert_eq!(transferred.points(), 30.0);
    }

    #[test]
    fn content_box_aspect_ratio_transfer_keeps_content_box_flex_basis() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::ContentBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(150.0),
        );
        // Taffy applies content-box sizing itself, so these edges do not
        // belong in the adapter's flex-basis value.
        style.padding.top = 75.0;
        style.padding.bottom = 75.0;

        let basis = aspect_ratio_transferred_flex_basis(
            &style,
            &test_flex_estimate(),
            FlexDirection::Column,
            Some(FlexCrossSize::new(300.0)),
            None,
            Some(1.0),
        )
        .expect("definite cross size should transfer through aspect ratio");

        assert_eq!(basis.points(), 150.0);
    }

    #[test]
    fn border_box_aspect_ratio_transfer_adds_extras_once() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(300.0),
        );
        style.padding.left = 75.0;
        style.padding.right = 75.0;
        style.padding.top = 75.0;
        style.padding.bottom = 75.0;

        let basis = aspect_ratio_transferred_flex_basis(
            &style,
            &test_flex_estimate(),
            FlexDirection::Column,
            Some(FlexCrossSize::new(300.0)),
            None,
            Some(1.0),
        )
        .expect("definite cross size should transfer through aspect ratio");

        assert_eq!(basis.points(), 300.0);
    }

    #[test]
    fn aspect_ratio_flex_basis_uses_max_constrained_cross_size() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(500.0),
        );
        style.box_values.max_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(100.0)),
        );

        let basis = aspect_ratio_transferred_flex_basis(
            &style,
            &estimate,
            FlexDirection::Column,
            Some(FlexCrossSize::new(100.0)),
            None,
            Some(1.0),
        )
        .expect("definite max-constrained cross size should transfer through aspect ratio");

        assert_eq!(basis.points(), 100.0);
    }
}
