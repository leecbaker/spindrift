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

/// Converts a flex item's physical size for Taffy at an explicit Flexbox
/// cross-sizing phase.
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
            css::ComputedLengthPercentageOrAuto::Auto => match context.cross_sizing_phase {
                FlexCrossSizingPhase::Hypothetical => {
                    match context.hypothetical_automatic_cross_size {
                        FlexHypotheticalAutomaticCrossSize::Intrinsic => {
                            taffy_layout::Dimension::auto()
                        }
                        FlexHypotheticalAutomaticCrossSize::FitContent { used_content_size } => {
                            taffy_layout::Dimension::length(used_content_size.points().max(0.0))
                        }
                    }
                }
                FlexCrossSizingPhase::StretchToLine {
                    line_outer_cross_size,
                } => taffy_stretch_fit_dimension(FlexStretchFitContext {
                    available_margin_box_size: Some(layout_pt(line_outer_cross_size.points())),
                    ..context.stretch
                }),
            },
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
    /// The only cross-size phase that may influence this physical dimension.
    /// Main-axis callers carry the same context but ignore this value.
    pub(super) cross_sizing_phase: FlexCrossSizingPhase,
    /// Automatic sizing input used exclusively during the hypothetical phase.
    pub(super) hypothetical_automatic_cross_size: FlexHypotheticalAutomaticCrossSize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexStretchFitContext {
    pub(super) available_margin_box_size: Option<LayoutLength>,
    pub(super) margin_size: LayoutLength,
    pub(super) non_content_size: NonContentLength,
    pub(super) box_sizing: BoxSizing,
}

fn taffy_stretch_fit_dimension(context: FlexStretchFitContext) -> taffy_layout::Dimension {
    let Some(content_size) = resolved_stretch_fit_content_box_size(context) else {
        return taffy_layout::Dimension::auto();
    };
    let size = match context.box_sizing {
        BoxSizing::ContentBox => content_size.points(),
        BoxSizing::BorderBox => {
            content_box_to_border_box_length(content_size, context.non_content_size).points()
        }
    };
    taffy_layout::Dimension::length(size.max(0.0))
}

/// Resolves a definite stretch-fit margin-box slot into the item's content-box
/// size.
///
/// This is shared by normal `stretch` sizing and Flexbox's automatic-minimum
/// transferred-size suggestion, which both need the same box-model conversion:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>.
fn resolved_stretch_fit_content_box_size(
    context: FlexStretchFitContext,
) -> Option<ContentBoxLength> {
    context.available_margin_box_size.map(|available| {
        stretch_fit_content_box_size(available, context.margin_size, context.non_content_size)
    })
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

/// Resolve a table flex item's authored length/percentage minimum without
/// discarding the table grid's intrinsic minimum.
///
/// CSS Tables makes the used table minimum at least its min-content width,
/// while a definite (including percentage-resolved) authored `min-width` can
/// raise that floor.  Taffy has no `max(length, percent)` dimension, so this
/// adapter resolves the author value at the Flex percentage-basis boundary.
/// An indefinite percentage basis leaves the intrinsic table floor in place.
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
/// <https://www.w3.org/TR/css-sizing-3/#min-size-properties>
pub(super) fn table_length_percentage_min_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
    table_min_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    debug_assert!(matches!(
        value,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
    ));
    let table_min_content = table_min_content.max(content_box_pt(0.0));
    let table_min_layout = layout_pt(table_min_content.points());
    let authored = used_length_percentage_or_auto(value, percentage_basis)
        .unwrap_or(table_min_layout)
        .max(table_min_layout);
    taffy_layout::Dimension::length(authored.points())
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
    context: FlexMinSizeDimensionContext<'_>,
) -> taffy_layout::Dimension {
    let Some(automatic_minimum) = resolve_automatic_flex_minimum(specified.clone(), context) else {
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
    context: FlexMinSizeDimensionContext<'_>,
) -> Option<AutomaticFlexMinimum> {
    let minimum_kind =
        flex_main_axis_content_based_minimum_kind(&specified, context.style, context.direction)?;
    if !context.is_main_axis {
        return None;
    }
    if minimum_kind == FlexContentBasedMinimumKind::CssAutomatic && context.overflow.is_scrollable()
    {
        return Some(AutomaticFlexMinimum {
            content_size_suggestion: content_box_pt(0.0),
            transferred_size_suggestion: None,
            specified_size_suggestion: None,
            used_content_box: content_box_pt(0.0),
        });
    }
    let inputs = context.automatic_minimum_inputs?;
    // CSS Flexbox 4.5: non-scrollable flex items use the content-based
    // minimum size in the main axis, capped by a definite preferred main
    // size. Cross-axis auto minimums remain automatic.
    // The pass-scoped input preserves whether this is a genuine intrinsic
    // contribution or a ratio-only replaced item. `calc-size(auto, …)` must
    // still peel back an estimate's raised floor before substituting `auto`.
    // <https://drafts.csswg.org/css-values-5/#calc-size>.
    let intrinsic_content_size_suggestion = if specified.calc_size_with_auto_basis().is_some() {
        inputs
            .content_size_source
            .content_size_suggestion()
            .min(inputs.max_content_size)
    } else {
        inputs.content_size_source.content_size_suggestion()
    };
    let authored_stretch_fit_cross_size = authored_cross_stretch_fit_content_box_size(
        context.style,
        context.direction,
        context.cross_stretch,
    );
    let transferred_size_suggestion = automatic_minimum_transferred_size_suggestion(
        context.style,
        context.direction,
        inputs.preferred_aspect_ratio,
        FlexCrossSizeSuggestionContext {
            available_cross_size: context.available_cross_size,
            authored_stretch_fit_cross_size,
            stretched_cross_size: context.stretched_cross_size,
            automatic_preferred_cross_size: inputs
                .automatic_preferred_cross_size
                .content_box_size(),
            intrinsic: FlexCrossIntrinsicContributions {
                min_content: inputs.cross_intrinsic.min_content,
                max_content: inputs.cross_intrinsic.max_content,
            },
        },
    )
    .map(|suggestion| {
        inputs
            .aspect_ratio_sizing
            .map(|sizing| {
                if context.direction.is_row_axis() {
                    sizing.constraints.constrain_width(suggestion)
                } else {
                    sizing.constraints.constrain_height(suggestion)
                }
            })
            .unwrap_or(suggestion)
    });
    // A replaced item's automatic preferred main size is itself derived from
    // its definite preferred cross size and ratio. In particular, an authored
    // cross-axis `stretch` is not merely an additional minimum constraint on
    // the intrinsic object: it establishes the replaced item's used content
    // contribution before Flexbox compares content and transferred
    // suggestions. Otherwise the replaced-element `min()` rule would retain
    // the natural object size and defeat a larger stretch-fit cross size.
    // <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
    // <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>.
    let content_size_suggestion = if inputs.is_replaced && authored_stretch_fit_cross_size.is_some()
    {
        transferred_size_suggestion.unwrap_or(intrinsic_content_size_suggestion)
    } else {
        intrinsic_content_size_suggestion
    };
    let selection = AutomaticFlexMinimum::from_suggestions(
        content_size_suggestion,
        transferred_size_suggestion,
        inputs.definite_preferred_content_size,
        inputs.is_replaced,
    );
    selection.debug_assert_consistent(inputs.is_replaced);
    let automatic_minimum = selection.used_content_box.points();
    let automatic_minimum = specified
        .calc_size_with_auto_basis()
        .map(|value| {
            value
                .used_value(
                    automatic_minimum,
                    content_size_suggestion.points(),
                    inputs.max_content_size.points(),
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
        transferred_size_suggestion,
        specified_size_suggestion: inputs.definite_preferred_content_size,
        used_content_box: content_box_pt(automatic_minimum.max(0.0)),
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexMinSizeDimensionContext<'a> {
    pub(super) style: &'a ComputedStyle,
    pub(super) direction: FlexDirection,
    pub(super) automatic_minimum_inputs: Option<FlexAutomaticMinimumInputs>,
    pub(super) available_cross_size: Option<FlexCrossSize>,
    /// The cross-axis stretch-fit context for an authored `width`/`height:
    /// `stretch`. This is distinct from `stretched_cross_size`, which records
    /// Flexbox self-alignment stretch after line sizing.
    pub(super) cross_stretch: FlexStretchFitContext,
    pub(super) stretched_cross_size: Option<FlexCrossSize>,
    pub(super) is_main_axis: bool,
    pub(super) overflow: css::Overflow,
    pub(super) percentage_basis: FlexAvailablePercentageBasis,
    pub(super) stretch: FlexStretchFitContext,
}

/// Why a flex item's main-axis minimum needs the content-based floor.
///
/// `min-content` in an item's logical block axis is automatic sizing under
/// CSS Sizing. It therefore selects the same content-based suggestion as an
/// automatic flex minimum, but remains an authored intrinsic constraint: the
/// scroll-container exception applies only to an actual `auto` minimum.
/// Keeping those origins distinct prevents a scrollable `min-height:
/// min-content` item from being silently converted to zero.
/// <https://www.w3.org/TR/css-sizing-3/#valdef-width-min-content> and
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexContentBasedMinimumKind {
    CssAutomatic,
    BlockAxisMinContent,
}

/// Select a main-axis minimum that needs Flexbox's content-based suggestion.
///
/// This is intentionally defined at the physical-main/logical-block boundary:
/// a vertical writing-mode item can have a horizontal block axis, so the
/// property name alone is not enough to classify `min-content`.
pub(super) fn flex_main_axis_content_based_minimum_kind(
    value: &css::ComputedLengthPercentageOrAuto,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> Option<FlexContentBasedMinimumKind> {
    if value.is_auto() || value.calc_size_with_auto_basis().is_some() {
        return Some(FlexContentBasedMinimumKind::CssAutomatic);
    }
    if matches!(value, css::ComputedLengthPercentageOrAuto::MinContent)
        && flex_main_axis_is_item_block_axis(style, physical_direction)
    {
        return Some(FlexContentBasedMinimumKind::BlockAxisMinContent);
    }
    None
}

/// Whether the container's physical flex main axis is the item's logical
/// block axis.
fn flex_main_axis_is_item_block_axis(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    match WritingModeAxes::new(style.writing_mode, style.used_direction())
        .physical_axis(LogicalAxis::Block)
    {
        PhysicalAxis::Horizontal => physical_direction.is_row_axis(),
        PhysicalAxis::Vertical => physical_direction.is_column_axis(),
    }
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
            if let Some(size) = context.ratio_only_replaced_base_size {
                return ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                    flex_auto_content_basis(
                        style,
                        size.main_content_size(context.direction),
                        context.direction,
                    )
                    .points(),
                ));
            }
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
        } else if let Some(size) = context.ratio_only_replaced_base_size {
            ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
                flex_auto_content_basis(
                    style,
                    size.main_content_size(FlexDirection::Row),
                    FlexDirection::Row,
                )
                .points(),
            ))
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
    } else if let Some(size) = context.ratio_only_replaced_base_size {
        ResolvedFlexBasis::normal_flow_content(taffy_layout::Dimension::length(
            flex_auto_content_basis(
                style,
                size.main_content_size(FlexDirection::Column),
                FlexDirection::Column,
            )
            .points(),
        ))
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
    pub(super) ratio_only_replaced_base_size: Option<RatioOnlyReplacedFlexBaseSize>,
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

    /// Whether Flexbox 9.8 treats the resulting post-flexing main size as
    /// definite for descendant percentage sizing.
    /// <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    pub(super) fn is_definite(self) -> bool {
        matches!(
            self,
            Self::AspectRatioTransfer | Self::MainSizeProperty | Self::DefiniteFlexBasis
        )
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

    /// Reports definiteness independently of the scalar Taffy dimension.
    ///
    /// A basis transferred through a preferred aspect ratio is definite when
    /// its determining cross size is definite, just like an authored definite
    /// basis or a definite main-size property:
    /// <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>.
    pub(super) fn is_definite(self) -> bool {
        self.provenance.is_definite()
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
    let transferred = aspect_ratio_transferred_content_main_size_with_cross_constraints(
        style,
        direction,
        preferred_aspect_ratio,
        FlexCrossSizeSuggestionContext {
            available_cross_size,
            authored_stretch_fit_cross_size: None,
            stretched_cross_size,
            automatic_preferred_cross_size: None,
            intrinsic: FlexCrossIntrinsicContributions {
                min_content: if direction.is_row_axis() {
                    estimate.min_height
                } else {
                    estimate.min_width
                },
                max_content: if direction.is_row_axis() {
                    estimate.content_height
                } else {
                    estimate.content_width
                },
            },
        },
    )
    .map(|size| {
        estimate
            .aspect_ratio_sizing
            .map(|sizing| {
                if direction.is_row_axis() {
                    sizing.constraints.constrain_width(size)
                } else {
                    sizing.constraints.constrain_height(size)
                }
            })
            .unwrap_or(size)
    });
    let transferred = transferred.or_else(|| {
        let sizing = estimate.aspect_ratio_sizing?;
        if !style.box_values.width.is_auto() || !style.box_values.height.is_auto() {
            return None;
        }
        // Flexbox 9.2 Part E lays an auto/auto ratio item into its
        // fit-content cross size while ignoring min/max constraints in the
        // main axis. Apply only the authored cross-axis constraints here;
        // using the fully transferred result would reflect `min-height` into
        // a column item's fit-content width and inflate its flex base.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
        Some(if direction.is_row_axis() {
            let cross = sizing
                .authored_height_constraints
                .constrain(sizing.intrinsic_height);
            sizing.ratio.width_from_height(cross)
        } else {
            let cross = sizing
                .authored_width_constraints
                .constrain(sizing.intrinsic_width);
            sizing.ratio.height_from_width(cross)
        })
    })?;
    Some(flex_aspect_ratio_basis_from_content_box(
        style,
        transferred,
        direction,
    ))
}

/// Intrinsic cross-axis contributions used while resolving a transferred
/// aspect-ratio size.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexCrossIntrinsicContributions {
    pub(super) min_content: ContentBoxLength,
    pub(super) max_content: ContentBoxLength,
}

/// Cross-axis inputs that can establish a Flexbox ratio transfer.
///
/// These suggestions are one conceptual unit: they all describe the item's
/// used cross-size contribution before it transfers through the preferred
/// aspect ratio. Keeping them together prevents a caller from accidentally
/// pairing intrinsic contributions with an unrelated percentage basis.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexCrossSizeSuggestionContext {
    pub(super) available_cross_size: Option<FlexCrossSize>,
    pub(super) authored_stretch_fit_cross_size: Option<ContentBoxLength>,
    pub(super) stretched_cross_size: Option<FlexCrossSize>,
    pub(super) automatic_preferred_cross_size: Option<ContentBoxLength>,
    pub(super) intrinsic: FlexCrossIntrinsicContributions,
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
    preferred_aspect_ratio: Option<f32>,
    cross: FlexCrossSizeSuggestionContext,
) -> Option<ContentBoxLength> {
    let ratio = preferred_aspect_ratio?;
    let cross_non_content = if direction.is_row_axis() {
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style))
    } else {
        non_content_pt(style.padding.left + style.padding.right + horizontal_border_width(style))
    };
    let specified_cross_size = if direction.is_row_axis() {
        used_content_box_height_or_auto_with_basis(
            style,
            percentage_basis_from_points(cross.available_cross_size.map(FlexCrossSize::points)),
            cross_non_content,
        )
    } else {
        used_content_box_width_or_auto_with_basis(
            style,
            percentage_basis_from_points(cross.available_cross_size.map(FlexCrossSize::points)),
            cross_non_content,
        )
    };
    let cross_content_size = specified_cross_size
        .or(cross.authored_stretch_fit_cross_size)
        .or_else(|| {
            cross
                .stretched_cross_size
                .map(|size| content_box_pt((size.points() - cross_non_content.points()).max(0.0)))
        })
        .or(cross.automatic_preferred_cross_size)?;

    // The definite cross size used for aspect-ratio transfer is a used size:
    // it is constrained by the item's cross-axis min/max properties before it
    // becomes a flex base size.  Keep the conversion in content-box space so
    // `box-sizing` is applied consistently to the preferred and constraint
    // sizes. CSS Flexbox 9.2 resolves the resulting cross size before deriving
    // the flex base size through the preferred aspect ratio:
    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
    let cross_percentage_basis =
        percentage_basis_from_points(cross.available_cross_size.map(FlexCrossSize::points));
    let constrained_cross = if direction.is_row_axis() {
        constrain_height_with_intrinsic(
            style,
            cross_content_size,
            cross.intrinsic.min_content,
            cross.intrinsic.max_content,
            cross_percentage_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
    } else {
        constrain_width_with_intrinsic(
            style,
            cross_content_size,
            cross.intrinsic.min_content,
            cross.intrinsic.max_content,
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
    preferred_aspect_ratio: Option<f32>,
    cross: FlexCrossSizeSuggestionContext,
) -> Option<ContentBoxLength> {
    if let Some(transferred) = aspect_ratio_transferred_content_main_size_with_cross_constraints(
        style,
        direction,
        preferred_aspect_ratio,
        cross,
    ) {
        return Some(transferred);
    }

    let ratio = preferred_aspect_ratio?;
    let percentage_basis =
        percentage_basis_from_points(cross.available_cross_size.map(FlexCrossSize::points));
    let constrained_minimum_cross = if direction.is_row_axis() {
        constrain_height_with_intrinsic(
            style,
            content_box_pt(0.0),
            cross.intrinsic.min_content,
            cross.intrinsic.max_content,
            percentage_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
    } else {
        constrain_width_with_intrinsic(
            style,
            content_box_pt(0.0),
            cross.intrinsic.min_content,
            cross.intrinsic.max_content,
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

/// Resolves an authored cross-axis `stretch` preferred size for Flexbox's
/// transferred-size suggestion.
///
/// This is intentionally separate from self-alignment stretch: the latter is
/// established after flex-line sizing, whereas CSS Sizing's `stretch` value
/// is itself a definite preferred cross size whenever its stretch-fit slot is
/// definite.
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>.
fn authored_cross_stretch_fit_content_box_size(
    style: &ComputedStyle,
    direction: FlexDirection,
    stretch: FlexStretchFitContext,
) -> Option<ContentBoxLength> {
    let cross_size = if direction.is_row_axis() {
        &style.box_values.height
    } else {
        &style.box_values.width
    };
    matches!(cross_size, css::ComputedLengthPercentageOrAuto::Stretch)
        .then(|| resolved_stretch_fit_content_box_size(stretch))
        .flatten()
}

/// Return a replaced item's CSS automatic preferred size on Flexbox's
/// physical cross axis when the corresponding CSS size property is `auto`.
///
/// The estimate records CSS Images' default object size separately from an
/// authored preferred size and from flex stretch. It is definite only for the
/// transferred-size suggestion; it must not turn `auto` into a general flex
/// basis override.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>
pub(super) fn automatic_preferred_cross_content_size(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
) -> Option<ContentBoxLength> {
    automatic_preferred_content_size_on_axis(
        style,
        estimate,
        if direction.is_row_axis() {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        },
    )
}

/// Select the stable inputs to one flex pass's automatic main-axis minimum.
///
/// The dynamic cross slot (available or stretched) is intentionally supplied
/// by each consumer later, but content, CSS Images fallback, and authored
/// preferred-size roles are selected exactly once here. This prevents a
/// post-layout correction from reclassifying an automatic preferred object
/// size as intrinsic content.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
pub(super) fn flex_automatic_minimum_inputs(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    direction: FlexDirection,
    content_size_source: FlexAutomaticMinimumContentSizeSource,
    preferred_aspect_ratio: Option<f32>,
    is_replaced: bool,
    available: FlexAvailableSpace,
) -> FlexAutomaticMinimumInputs {
    let (max_content_size, automatic_preferred_cross_size, cross_intrinsic) =
        if direction.is_row_axis() {
            (
                estimate.content_width,
                automatic_preferred_cross_content_size(style, estimate, FlexDirection::Row),
                FlexAutomaticMinimumCrossIntrinsicContributions {
                    min_content: estimate.min_height,
                    max_content: estimate.content_height,
                },
            )
        } else {
            (
                estimate.content_height,
                automatic_preferred_cross_content_size(style, estimate, FlexDirection::Column),
                FlexAutomaticMinimumCrossIntrinsicContributions {
                    min_content: estimate.min_width,
                    max_content: estimate.content_width,
                },
            )
        };
    let definite_preferred_content_size = if style.display.is_table() {
        None
    } else if direction.is_row_axis() {
        used_content_box_width_or_auto_with_basis(
            style,
            available.width_basis,
            non_content_pt(
                style.padding.left + style.padding.right + horizontal_border_width(style),
            ),
        )
    } else {
        used_content_box_height_or_auto_with_basis(
            style,
            available.height_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
    };
    FlexAutomaticMinimumInputs {
        content_size_source,
        max_content_size,
        automatic_preferred_cross_size: automatic_preferred_cross_size.map_or(
            FlexAutomaticMinimumAutomaticPreferredCrossSize::None,
            FlexAutomaticMinimumAutomaticPreferredCrossSize::CssImagesDefaultObjectSize,
        ),
        cross_intrinsic,
        preferred_aspect_ratio,
        aspect_ratio_sizing: estimate.aspect_ratio_sizing,
        is_replaced,
        definite_preferred_content_size,
    }
}

/// Store the one pass-scoped automatic-main-minimum input record used by all
/// Flexbox consumers.
///
/// Intrinsic contribution sizing, Taffy's flexible-length pass, and Quire's
/// final-layout safeguard must select the same content-size source and
/// preferred aspect ratio. The content probe is supplied separately because
/// it suppresses the preferred main size while retaining every other style.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
pub(super) fn set_flex_item_automatic_main_minimum_inputs(
    estimate: &mut FlexItemEstimate,
    style: &ComputedStyle,
    direction: FlexDirection,
    automatic_main_min_content: Option<ContentBoxLength>,
    preferred_aspect_ratio: Option<f32>,
    is_replaced: bool,
    available: FlexAvailableSpace,
) {
    let content_size_source = if is_replaced && estimate.automatic_preferred_physical_size.is_some()
    {
        // CSS Images' default object size remains a transferred-size source.
        // A viewBox-only replaced item has no intrinsic axis, so it must not
        // also become a min-content contribution.
        FlexAutomaticMinimumContentSizeSource::RatioOnlyReplaced
    } else {
        FlexAutomaticMinimumContentSizeSource::Intrinsic(automatic_main_min_content.unwrap_or_else(
            || {
                if direction.is_row_axis() {
                    estimate.min_width
                } else {
                    estimate.min_height
                }
            },
        ))
    };
    estimate.set_automatic_main_minimum_inputs(flex_automatic_minimum_inputs(
        style,
        estimate,
        direction,
        content_size_source,
        preferred_aspect_ratio,
        is_replaced,
        available,
    ));
}

/// Classify the computed CSS size property selected by the container's
/// physical cross axis. CSS logical properties have already been projected
/// into these physical width/height values by the cascade.
pub(super) fn flex_item_cross_size_property(
    style: &ComputedStyle,
    direction: FlexDirection,
) -> FlexCrossSizeProperty {
    let is_auto = if direction.is_row_axis() {
        style.box_values.height.is_auto()
    } else {
        style.box_values.width.is_auto()
    };
    if is_auto {
        FlexCrossSizeProperty::Auto
    } else {
        FlexCrossSizeProperty::NonAuto
    }
}

/// Whether this item may use automatic cross-size reconciliation in Quire's
/// current replaced-element adapter. The CSS-property classification remains
/// separate: an automatic preferred replaced size does not make its computed
/// cross-size property non-auto.
pub(super) fn flex_item_cross_size_is_auto(
    style: &ComputedStyle,
    direction: FlexDirection,
) -> bool {
    flex_item_cross_size_property(style, direction).is_auto()
}

/// Resolve the temporary physical size used for a ratio-only replaced item's
/// content-derived flex base size.
///
/// CSS Images retains the default object size as an automatic preferred size,
/// but Flexbox first sizes a content-based item into its available space. For
/// a viewBox-only SVG this means capping its logical inline size by the
/// margin-, border-, and padding-adjusted inline space and deriving the other
/// physical axis through the preferred aspect ratio.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
pub(super) fn ratio_only_replaced_flex_base_size(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    available: FlexItemAvailableSpace,
    margin: css::Edges,
    padding: css::Edges,
    borders: css::Edges,
    preferred_aspect_ratio: Option<f32>,
) -> Option<RatioOnlyReplacedFlexBaseSize> {
    if !style.box_values.width.is_auto() || !style.box_values.height.is_auto() {
        return None;
    }
    let automatic = estimate.automatic_preferred_physical_size?;
    let ratio = preferred_aspect_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0)?;
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let (available_inline, default_inline, inline_margin, inline_non_content) =
        if axes.swaps_physical_axes() {
            (
                available
                    .height
                    .map(PhysicalContentHeight::content_box_length)?,
                automatic.height.content_box_length(),
                margin.top + margin.bottom,
                padding.top + padding.bottom + borders.top + borders.bottom,
            )
        } else {
            (
                available.width.content_box_length(),
                automatic.width.content_box_length(),
                margin.left + margin.right,
                padding.left + padding.right + borders.left + borders.right,
            )
        };
    let inline = default_inline.min(content_box_pt(
        (available_inline.points() - inline_margin - inline_non_content).max(0.0),
    ));
    let (width, height) = if axes.swaps_physical_axes() {
        (inline.points() * ratio, inline.points())
    } else {
        (inline.points(), inline.points() / ratio)
    };
    let constrained = resolve_replaced_size_with_aspect_ratio(
        content_box_size_pt(width, height),
        ratio,
        ReplacedPreferredSizeAxes {
            width: ReplacedPreferredSize::Automatic,
            height: ReplacedPreferredSize::Automatic,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, available.width_basis)
                .map(|width| width.max(content_box_pt(0.0))),
            max_width: used_max_width(style, available.width_basis)
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
        },
    );
    Some(RatioOnlyReplacedFlexBaseSize::new(
        PhysicalContentWidth::new(content_box_pt(constrained.width)),
        PhysicalContentHeight::new(content_box_pt(constrained.height)),
    ))
}

/// Return a replaced item's CSS automatic preferred size on one physical axis.
pub(super) fn automatic_preferred_content_size_on_axis(
    style: &ComputedStyle,
    estimate: &FlexItemEstimate,
    axis: FlexDirection,
) -> Option<ContentBoxLength> {
    let size = estimate.automatic_preferred_physical_size?;
    if axis.is_row_axis() {
        style
            .box_values
            .width
            .is_auto()
            .then(|| size.width.content_box_length())
    } else {
        style
            .box_values
            .height
            .is_auto()
            .then(|| size.height.content_box_length())
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
    let border_widths = used_border_widths(style);
    let horizontal_non_content = non_content_pt(
        style.padding.left + style.padding.right + border_widths.left + border_widths.right,
    );
    let vertical_non_content = non_content_pt(
        style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom,
    );
    let calculation_box = if style.aspect_ratio.uses_content_box_for_non_replaced()
        || style.box_sizing == BoxSizing::ContentBox
    {
        AspectRatioCalculationBox::ContentBox
    } else {
        AspectRatioCalculationBox::BorderBox
    };
    let resolved = ResolvedAspectRatio::new(
        ratio,
        calculation_box,
        horizontal_non_content,
        vertical_non_content,
    )
    .expect("preferred aspect ratios are positive and finite");
    if direction.is_row_axis() {
        resolved.width_from_height(cross_content_size)
    } else {
        resolved.height_from_width(cross_content_size)
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
    // The intrinsic estimate is already the item's content-box contribution.
    // In particular, white-space processing belongs to text measurement, not
    // flex-basis conversion: adding a line-height or rounding here changes a
    // `flex-basis:auto` item even when its measured max-content width is
    // definite.
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
    fn automatic_content_basis_preserves_measured_width_with_preserved_whitespace() {
        let mut style = ComputedStyle::initial();
        style.white_space = css::WhiteSpace::Pre;

        assert_eq!(
            flex_auto_content_basis(&style, content_box_pt(18.65625), FlexDirection::Row).points(),
            18.65625
        );
    }

    #[test]
    fn block_axis_min_content_preserves_its_authored_minimum_kind() {
        let horizontal = ComputedStyle::initial();
        assert_eq!(
            flex_main_axis_content_based_minimum_kind(
                &css::ComputedLengthPercentageOrAuto::MinContent,
                &horizontal,
                FlexDirection::Column,
            ),
            Some(FlexContentBasedMinimumKind::BlockAxisMinContent),
        );
        assert_eq!(
            flex_main_axis_content_based_minimum_kind(
                &css::ComputedLengthPercentageOrAuto::MinContent,
                &horizontal,
                FlexDirection::Row,
            ),
            None,
        );
        assert_eq!(
            flex_main_axis_content_based_minimum_kind(
                &css::ComputedLengthPercentageOrAuto::Auto,
                &horizontal,
                FlexDirection::Column,
            ),
            Some(FlexContentBasedMinimumKind::CssAutomatic),
        );
    }

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
            cross_sizing_phase: FlexCrossSizingPhase::Hypothetical,
            hypothetical_automatic_cross_size: FlexHypotheticalAutomaticCrossSize::Intrinsic,
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
    fn cross_size_property_and_adapter_phase_keep_orthogonal_auto_distinct_from_inline_size() {
        let mut style = ComputedStyle::initial();
        // `inline-size` on a vertical item is projected by cascade to its
        // physical height. In a horizontal row that is the Flex cross-size
        // property and must therefore be non-auto; physical width remains
        // automatic for a column container's cross axis.
        style.writing_mode = WritingMode::VerticalRl;
        *style.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(6.0),
        );
        assert_eq!(
            flex_item_cross_size_property(&style, FlexDirection::Row),
            FlexCrossSizeProperty::NonAuto
        );
        assert_eq!(
            flex_item_cross_size_property(&style, FlexDirection::Column),
            FlexCrossSizeProperty::Auto
        );

        let context = FlexItemSizeDimensionContext {
            flex_direction: FlexDirection::Row,
            dimension_axis: FlexDirection::Column,
            percentage_basis: PercentageBasis::indefinite(),
            stretch: FlexStretchFitContext {
                available_margin_box_size: None,
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: BoxSizing::ContentBox,
            },
            flex_basis_overrides_main_size: false,
            cross_sizing_phase: FlexCrossSizingPhase::Hypothetical,
            hypothetical_automatic_cross_size: FlexHypotheticalAutomaticCrossSize::Intrinsic,
        };
        assert!(
            flex_item_size_dimension(
                css::ComputedLengthPercentageOrAuto::Auto,
                content_box_pt(10.0),
                content_box_pt(10.0),
                content_box_pt(20.0),
                context,
            )
            .is_auto()
        );

        let fit_content = FlexItemSizeDimensionContext {
            hypothetical_automatic_cross_size: FlexHypotheticalAutomaticCrossSize::FitContent {
                used_content_size: content_box_pt(36.0),
            },
            ..context
        };
        assert_eq!(
            flex_item_size_dimension(
                css::ComputedLengthPercentageOrAuto::Auto,
                content_box_pt(10.0),
                content_box_pt(10.0),
                content_box_pt(20.0),
                fit_content,
            ),
            taffy_layout::Dimension::length(36.0)
        );

        let stretched = FlexItemSizeDimensionContext {
            cross_sizing_phase: FlexCrossSizingPhase::StretchToLine {
                line_outer_cross_size: FlexCrossSize::new(44.0),
            },
            ..context
        };
        assert_eq!(
            flex_item_size_dimension(
                css::ComputedLengthPercentageOrAuto::Auto,
                content_box_pt(10.0),
                content_box_pt(10.0),
                content_box_pt(20.0),
                stretched,
            ),
            taffy_layout::Dimension::length(44.0)
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
            ratio_only_replaced_base_size: None,
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

    fn ratio_only_replaced_estimate(width: f32, height: f32) -> FlexItemEstimate {
        let mut estimate = test_flex_estimate();
        estimate.set_automatic_preferred_physical_size(FlexAutomaticPreferredPhysicalSize {
            width: PhysicalContentWidth::new(content_box_pt(width)),
            height: PhysicalContentHeight::new(content_box_pt(height)),
        });
        estimate
    }

    fn ratio_only_available_space(width: f32, height: Option<f32>) -> FlexItemAvailableSpace {
        FlexItemAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(width)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(width),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: height.map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            height_basis: height.map_or_else(PercentageBasis::indefinite, |height| {
                PercentageBasis::definite_from(
                    content_box_pt(height),
                    FlexAvailableSizeSource::ContainingBlock,
                )
            }),
            stretched_width: None,
            stretched_height: None,
        }
    }

    #[test]
    fn ratio_only_replaced_flex_base_uses_margin_adjusted_inline_space() {
        let style = ComputedStyle::initial();
        let estimate = ratio_only_replaced_estimate(225.0, 225.0);
        let base = ratio_only_replaced_flex_base_size(
            &style,
            &estimate,
            ratio_only_available_space(112.5, None),
            css::Edges {
                right: 37.5,
                ..css::Edges::ZERO
            },
            css::Edges::ZERO,
            css::Edges::ZERO,
            Some(1.0),
        )
        .expect("a ratio-only replaced item has a temporary flex base size");

        assert_eq!(base.main_content_size(FlexDirection::Row).points(), 75.0);
        assert_eq!(base.main_content_size(FlexDirection::Column).points(), 75.0);
        assert_eq!(base.cross_content_size(FlexDirection::Row).points(), 75.0);
        assert!(flex_item_cross_size_is_auto(&style, FlexDirection::Column));
        assert_eq!(
            automatic_preferred_cross_content_size(&style, &estimate, FlexDirection::Column)
                .expect("the CSS Images fallback remains available to automatic minimum sizing")
                .points(),
            225.0,
        );

        let resolved = resolve_taffy_flex_basis(
            &style,
            &estimate,
            FlexBasisContext {
                ratio_only_replaced_base_size: Some(base),
                ..test_flex_basis_context()
            },
        );
        assert_eq!(resolved.dimension, taffy_layout::Dimension::length(75.0));
        assert_eq!(
            resolved.provenance,
            FlexMainSizeProvenance::NormalFlowContent
        );

        let column = resolve_taffy_flex_basis(
            &style,
            &estimate,
            FlexBasisContext {
                direction: FlexDirection::Column,
                ratio_only_replaced_base_size: Some(base),
                ..test_flex_basis_context()
            },
        );
        assert_eq!(column.dimension, taffy_layout::Dimension::length(75.0));
    }

    #[test]
    fn ratio_only_replaced_flex_base_projects_vertical_inline_space() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let estimate = ratio_only_replaced_estimate(225.0, 112.5);
        let base = ratio_only_replaced_flex_base_size(
            &style,
            &estimate,
            ratio_only_available_space(400.0, Some(80.0)),
            css::Edges {
                top: 20.0,
                ..css::Edges::ZERO
            },
            css::Edges::ZERO,
            css::Edges::ZERO,
            Some(2.0),
        )
        .expect("a vertical ratio-only replaced item has a temporary flex base size");

        assert_eq!(base.main_content_size(FlexDirection::Row).points(), 120.0);
        assert_eq!(base.main_content_size(FlexDirection::Column).points(), 60.0);
        assert_eq!(base.cross_content_size(FlexDirection::Row).points(), 60.0);
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
        assert!(resolve_taffy_flex_basis(&transferred, &estimate, context).is_definite());
    }

    #[test]
    fn auto_auto_ratio_column_basis_uses_fit_content_cross_and_ignores_main_minimum() {
        let mut style = ComputedStyle::initial();
        style.aspect_ratio = css::AspectRatio::from_ratio(1.0).unwrap();
        let ratio = ResolvedAspectRatio::new(
            1.0,
            AspectRatioCalculationBox::ContentBox,
            non_content_pt(0.0),
            non_content_pt(0.0),
        )
        .unwrap();
        let authored_width_constraints = AspectRatioAxisConstraints {
            minimum: Some(content_box_pt(100.0)),
            ..Default::default()
        };
        let authored_height_constraints = AspectRatioAxisConstraints {
            minimum: Some(content_box_pt(200.0)),
            ..Default::default()
        };
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(100.0)),
        );
        estimate.set_aspect_ratio_sizing(FlexAspectRatioSizing {
            ratio,
            authored_width_constraints,
            authored_height_constraints,
            constraints: ResolvedAspectRatioConstraints::resolve(
                ratio,
                authored_width_constraints,
                authored_height_constraints,
            ),
            intrinsic_width: content_box_pt(1.0),
            intrinsic_height: content_box_pt(0.0),
        });
        let context = FlexBasisContext {
            direction: FlexDirection::Column,
            preferred_aspect_ratio: Some(1.0),
            ..test_flex_basis_context()
        };

        let resolved = resolve_taffy_flex_basis(&style, &estimate, context);

        assert_eq!(resolved.dimension, taffy_layout::Dimension::length(100.0));
        assert_eq!(
            resolved.provenance,
            FlexMainSizeProvenance::AspectRatioTransfer
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
            Some(1.0),
            FlexCrossSizeSuggestionContext {
                available_cross_size: None,
                authored_stretch_fit_cross_size: None,
                stretched_cross_size: None,
                automatic_preferred_cross_size: None,
                intrinsic: FlexCrossIntrinsicContributions {
                    min_content: content_box_pt(10.0),
                    max_content: content_box_pt(100.0),
                },
            },
        )
        .expect("a definite cross minimum supplies a transferred suggestion");

        assert_eq!(transferred.points(), 30.0);
    }

    #[test]
    fn automatic_preferred_replaced_cross_size_transfers_through_max_constraints() {
        let mut column_style = ComputedStyle::initial();
        column_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );
        let column_transferred = automatic_minimum_transferred_size_suggestion(
            &column_style,
            FlexDirection::Column,
            Some(1.0),
            FlexCrossSizeSuggestionContext {
                available_cross_size: None,
                authored_stretch_fit_cross_size: None,
                stretched_cross_size: None,
                automatic_preferred_cross_size: Some(content_box_pt(225.0)),
                intrinsic: FlexCrossIntrinsicContributions {
                    min_content: content_box_pt(0.0),
                    max_content: content_box_pt(225.0),
                },
            },
        )
        .expect("automatic width transfers through max-width");
        assert_eq!(column_transferred.points(), 100.0);

        let mut row_style = ComputedStyle::initial();
        row_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );
        let row_transferred = automatic_minimum_transferred_size_suggestion(
            &row_style,
            FlexDirection::Row,
            Some(1.0),
            FlexCrossSizeSuggestionContext {
                available_cross_size: None,
                authored_stretch_fit_cross_size: None,
                stretched_cross_size: None,
                automatic_preferred_cross_size: Some(content_box_pt(225.0)),
                intrinsic: FlexCrossIntrinsicContributions {
                    min_content: content_box_pt(0.0),
                    max_content: content_box_pt(225.0),
                },
            },
        )
        .expect("automatic height transfers through max-height");
        assert_eq!(row_transferred.points(), 100.0);
    }

    #[test]
    fn ratio_only_automatic_minimum_keeps_css_images_fallback_transferred_only() {
        let style = ComputedStyle::initial();
        let inputs = FlexAutomaticMinimumInputs {
            content_size_source: FlexAutomaticMinimumContentSizeSource::RatioOnlyReplaced,
            max_content_size: content_box_pt(225.0),
            automatic_preferred_cross_size:
                FlexAutomaticMinimumAutomaticPreferredCrossSize::CssImagesDefaultObjectSize(
                    content_box_pt(225.0),
                ),
            cross_intrinsic: FlexAutomaticMinimumCrossIntrinsicContributions {
                min_content: content_box_pt(0.0),
                max_content: content_box_pt(225.0),
            },
            preferred_aspect_ratio: Some(1.0),
            aspect_ratio_sizing: None,
            is_replaced: true,
            definite_preferred_content_size: None,
        };
        let resolved = resolve_automatic_flex_minimum(
            css::ComputedLengthPercentageOrAuto::Auto,
            FlexMinSizeDimensionContext {
                style: &style,
                direction: FlexDirection::Row,
                automatic_minimum_inputs: Some(inputs),
                available_cross_size: None,
                cross_stretch: FlexStretchFitContext {
                    available_margin_box_size: None,
                    margin_size: layout_pt(0.0),
                    non_content_size: non_content_pt(0.0),
                    box_sizing: BoxSizing::ContentBox,
                },
                stretched_cross_size: None,
                is_main_axis: true,
                overflow: flex_item_main_axis_overflow(&style, FlexDirection::Row),
                percentage_basis: PercentageBasis::indefinite(),
                stretch: FlexStretchFitContext {
                    available_margin_box_size: None,
                    margin_size: layout_pt(0.0),
                    non_content_size: non_content_pt(0.0),
                    box_sizing: BoxSizing::ContentBox,
                },
            },
        )
        .expect("a main-axis automatic minimum resolves");

        assert_eq!(resolved.content_size_suggestion.points(), 0.0);
        assert_eq!(
            resolved
                .transferred_size_suggestion
                .expect("the CSS Images fallback transfers through the ratio")
                .points(),
            225.0
        );
        assert_eq!(resolved.used_content_box.points(), 0.0);
    }

    #[test]
    fn intrinsic_replaced_automatic_minimum_retains_its_content_suggestion() {
        let style = ComputedStyle::initial();
        let inputs = FlexAutomaticMinimumInputs {
            content_size_source: FlexAutomaticMinimumContentSizeSource::Intrinsic(content_box_pt(
                50.0,
            )),
            max_content_size: content_box_pt(100.0),
            automatic_preferred_cross_size: FlexAutomaticMinimumAutomaticPreferredCrossSize::None,
            cross_intrinsic: FlexAutomaticMinimumCrossIntrinsicContributions {
                min_content: content_box_pt(0.0),
                max_content: content_box_pt(0.0),
            },
            preferred_aspect_ratio: None,
            aspect_ratio_sizing: None,
            is_replaced: true,
            definite_preferred_content_size: None,
        };
        let resolved = resolve_automatic_flex_minimum(
            css::ComputedLengthPercentageOrAuto::Auto,
            FlexMinSizeDimensionContext {
                style: &style,
                direction: FlexDirection::Row,
                automatic_minimum_inputs: Some(inputs),
                available_cross_size: None,
                cross_stretch: FlexStretchFitContext {
                    available_margin_box_size: None,
                    margin_size: layout_pt(0.0),
                    non_content_size: non_content_pt(0.0),
                    box_sizing: BoxSizing::ContentBox,
                },
                stretched_cross_size: None,
                is_main_axis: true,
                overflow: flex_item_main_axis_overflow(&style, FlexDirection::Row),
                percentage_basis: PercentageBasis::indefinite(),
                stretch: FlexStretchFitContext {
                    available_margin_box_size: None,
                    margin_size: layout_pt(0.0),
                    non_content_size: non_content_pt(0.0),
                    box_sizing: BoxSizing::ContentBox,
                },
            },
        )
        .expect("a main-axis automatic minimum resolves");

        assert_eq!(resolved.used_content_box.points(), 50.0);
    }

    #[test]
    fn scrollable_block_axis_min_content_does_not_take_auto_minimum_zero() {
        let mut style = ComputedStyle::initial();
        style.overflow_y = css::Overflow::Auto;
        let inputs = FlexAutomaticMinimumInputs {
            content_size_source: FlexAutomaticMinimumContentSizeSource::Intrinsic(content_box_pt(
                50.0,
            )),
            max_content_size: content_box_pt(100.0),
            automatic_preferred_cross_size: FlexAutomaticMinimumAutomaticPreferredCrossSize::None,
            cross_intrinsic: FlexAutomaticMinimumCrossIntrinsicContributions {
                min_content: content_box_pt(0.0),
                max_content: content_box_pt(0.0),
            },
            preferred_aspect_ratio: None,
            aspect_ratio_sizing: None,
            is_replaced: true,
            definite_preferred_content_size: None,
        };
        let context = FlexMinSizeDimensionContext {
            style: &style,
            direction: FlexDirection::Column,
            automatic_minimum_inputs: Some(inputs),
            available_cross_size: None,
            cross_stretch: FlexStretchFitContext {
                available_margin_box_size: None,
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: BoxSizing::ContentBox,
            },
            stretched_cross_size: None,
            is_main_axis: true,
            overflow: flex_item_main_axis_overflow(&style, FlexDirection::Column),
            percentage_basis: PercentageBasis::indefinite(),
            stretch: FlexStretchFitContext {
                available_margin_box_size: None,
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: BoxSizing::ContentBox,
            },
        };

        let auto =
            resolve_automatic_flex_minimum(css::ComputedLengthPercentageOrAuto::Auto, context)
                .expect("a scrollable automatic minimum resolves");
        let min_content = resolve_automatic_flex_minimum(
            css::ComputedLengthPercentageOrAuto::MinContent,
            context,
        )
        .expect("a block-axis min-content minimum resolves");

        assert_eq!(auto.used_content_box.points(), 0.0);
        assert_eq!(min_content.used_content_box.points(), 50.0);
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
