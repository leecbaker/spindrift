use super::*;
use crate::units::{
    IntoLayoutLength, content_box_to_margin_box_length, layout_to_content_box_length,
};

mod alignment;
mod baseline;
mod children;
mod compute;
mod estimate;
mod layout;
mod model;
mod taffy;

use alignment::*;
use baseline::*;
pub(in crate::layout) use children::flex_container_fragment_boundary_breaks;
use children::*;
pub(in crate::layout::flex) use compute::automatic_minimum_main_content_size;
use estimate::estimated_outer_cross_size;
pub(in crate::layout) use layout::flex_gap_decoration_primitives_with_gutters;
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
            .block_percentage_context_stack
            .current_percentage_basis();
        flex_container_height_percentage_basis_for_context(
            stack_basis,
            self.current_child_available_space()
                .physical_height_percentage_basis(),
            self.layout_pass_kind,
        )
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
        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let intrinsic_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        );
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
                height: intrinsic_height.map(PhysicalContentHeight::new),
                height_basis: flex_available_percentage_basis(
                    intrinsic_height,
                    FlexAvailableSizeSource::IntrinsicContainerSize,
                ),
            },
        );
        FlexIntrinsicWidthContributions::new(
            PhysicalContentWidth::new(intrinsic.min_width),
            PhysicalContentWidth::new(intrinsic.width),
        )
    }

    /// Measure a floated flex container's used margin-box height after the
    /// float algorithm has resolved its used inline size.
    ///
    /// Intrinsic flex contributions establish a float's shrink-to-fit width,
    /// but CSS 2.2 derives an automatic float height from the used formatting
    /// context at that width.  In particular, a following cleared sibling must
    /// see the final flex line geometry, not an intrinsic cross-size estimate.
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width>
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
    pub(in crate::layout) fn measure_floated_flex_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        content_width: PhysicalContentWidth,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> MarginBoxLength {
        let border_widths = used_border_widths(style);
        // Computed style still stores used edge values as CSS scalars. Turn
        // the vertical edges into a box-model quantity at this boundary so the
        // final flex content height cannot accidentally mix with margin-box
        // geometry.
        let vertical_non_content = non_content_pt(
            border_widths.top + border_widths.bottom + style.padding.top + style.padding.bottom,
        );
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
        let mut children =
            flex_children_from_boxes(element, &container_signature, style, child_boxes);
        self.resolve_styled_children_used_lengths(&mut children);
        let final_layout = self.compute_flex_layout(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: content_width.non_negative(),
                width_basis: flex_available_percentage_basis(
                    Some(content_width.content_box_length()),
                    FlexAvailableSizeSource::ContainingBlock,
                ),
                height: explicit_content_height.map(PhysicalContentHeight::new),
                height_basis: flex_available_percentage_basis(
                    explicit_content_height,
                    FlexAvailableSizeSource::ContainingBlock,
                ),
            },
        );
        let content_height = explicit_content_height.unwrap_or_else(|| {
            final_layout
                .map(|layout| layout.height.content_box_length())
                // `compute_flex_layout` only declines malformed trees that
                // cannot produce a final flex formatting context. Preserve a
                // finite fallback for that defensive path; ordinary floated
                // flex layout always uses the finalized result above.
                .unwrap_or_else(|| content_box_pt(0.0))
        });
        let percentage_height_basis = height_basis
            .value()
            .unwrap_or_else(|| content_width.content_box_length());
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
    pub(in crate::layout) fn with_flex_item_percentage_height_basis<R>(
        &mut self,
        basis: IntrinsicBlockBasis,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        // An indefinite item basis must shadow an enclosing definite flex
        // basis.  The enclosing basis belongs to the item's containing
        // block, not to an auto-sized intermediate flex item: letting it
        // leak through would resolve a grandchild's percentage height as if
        // the intermediate item had a definite height.  This is especially
        // visible while measuring `min-height:auto` after suppressing an
        // item's preferred main size.
        // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
        // <https://drafts.csswg.org/css-sizing-3/#percentage-sizing>
        self.block_percentage_context_stack
            .push_percentage_basis(basis.descendant_percentage_basis());
        let result = layout(self);
        self.block_percentage_context_stack.pop();
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
        let intrinsic_basis = match basis {
            PercentageBasis::Definite { value, .. } => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::PostFlexingMainSize,
            ),
            PercentageBasis::Indefinite => IntrinsicBlockBasis::Indefinite,
        };
        let result = self.with_flex_item_percentage_height_basis(intrinsic_basis, layout);
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

/// Select Flex's physical-height percentage basis at a formatting-context
/// boundary.
///
/// Positioned intrinsic sizing explicitly scopes an indefinite block basis.
/// Its numeric available height remains a geometric constraint, but CSS
/// Sizing requires that cyclic percentages behave as `auto` rather than being
/// reconstructed from that available space.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
fn flex_container_height_percentage_basis_for_context(
    scoped_basis: BlockSizePercentageBasis,
    available_height_basis: PercentageBasis<PhysicalContentHeight>,
    layout_pass_kind: LayoutPassKind,
) -> BlockSizePercentageBasis {
    if scoped_basis.is_definite()
        || layout_pass_kind == LayoutPassKind::PositionedAutoSizeMeasurement
    {
        return scoped_basis;
    }

    match available_height_basis {
        PercentageBasis::Definite { value: height, .. } => PercentageBasis::definite_from(
            height.content_box_length(),
            BlockSizeBasisSource::ContainingBlock,
        ),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
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

/// Whether a flex item's intrinsic descendants may resolve block-axis
/// percentages against the item's content box.
///
/// A numeric available height is not itself sufficient: CSS Sizing keeps
/// cyclic intrinsic contributions indefinite.  This enum records the small
/// set of Flexbox and CSS Sizing operations that have established the item's
/// *own* definite content-box height instead.
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
fn flex_item_estimate_percentage_height_basis(
    style: &ComputedStyle,
    available: FlexItemAvailableSpace,
    vertical_non_content: NonContentLength,
) -> IntrinsicBlockBasis {
    // A preferred height which resolves against a definite containing block
    // is itself definite. This must be tested before considering flex's
    // synthetic available-space sources: a nested item with `height: 100%`
    // owns a definite basis even if its parent is being intrinsically sized.
    let containing_block_basis =
        intrinsic_block_basis_from_flex_available_height(available.height_basis);
    if let Some(height) = used_content_box_height_or_auto_with_basis(
        style,
        containing_block_basis.descendant_percentage_basis(),
        vertical_non_content,
    ) {
        return IntrinsicBlockBasis::DefiniteFromContainingBlock(height);
    }
    // A flex-established item height can supply a descendant percentage
    // basis only through the typed available-size provenance. In particular,
    // do not recover this from the raw `stretched_height` field: that field
    // also records final replay sizes which are too late to affect flex-base
    // selection.
    intrinsic_block_basis_from_flex_available_height(available.height_basis)
}

/// Convert a flex available block-size into the narrower basis accepted by
/// intrinsic descendants. `IntrinsicContainerSize` is deliberately excluded:
/// it can constrain an intrinsic probe, but it has not established the
/// flex item's used block size for percentage resolution.
fn intrinsic_block_basis_from_flex_available_height(
    basis: FlexAvailablePercentageBasis,
) -> IntrinsicBlockBasis {
    match basis {
        PercentageBasis::Indefinite => IntrinsicBlockBasis::Indefinite,
        PercentageBasis::Definite {
            value,
            source: FlexAvailableSizeSource::ContainingBlock,
        } => IntrinsicBlockBasis::DefiniteFromContainingBlock(value),
        PercentageBasis::Definite { value, source } => match source {
            FlexAvailableSizeSource::DefiniteCrossSize => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::ExistingFlexItem,
            ),
            FlexAvailableSizeSource::DefiniteFlexBase => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::DefiniteFlexBase,
            ),
            FlexAvailableSizeSource::PostFlexingMainSize => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::PostFlexingMainSize,
            ),
            FlexAvailableSizeSource::DefinitePreferredMainSize
            | FlexAvailableSizeSource::DefinitePreferredCrossSize => {
                IntrinsicBlockBasis::from_flex_layout(
                    value,
                    FlexIntrinsicBlockBasisSource::DefinitePreferredSize,
                )
            }
            FlexAvailableSizeSource::BalancedLineSlot => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::BalancedLineSlot,
            ),
            FlexAvailableSizeSource::DefiniteSingleLineStretch => {
                IntrinsicBlockBasis::from_flex_layout(
                    value,
                    FlexIntrinsicBlockBasisSource::DefiniteSingleLineStretch,
                )
            }
            FlexAvailableSizeSource::AspectRatioDerived => IntrinsicBlockBasis::from_flex_layout(
                value,
                FlexIntrinsicBlockBasisSource::AspectRatioTransfer,
            ),
            FlexAvailableSizeSource::IntrinsicContainerSize => IntrinsicBlockBasis::Indefinite,
            FlexAvailableSizeSource::ContainingBlock => unreachable!(
                "containing-block percentage bases are handled before Flex layout bases"
            ),
        },
    }
}

/// Return a definite block basis only when an explicit, externally-resolved
/// block constraint actually wins over the intrinsic candidate.
///
/// This is intentionally separate from [`IntrinsicBlockBasis`] creation
/// from a preferred size. An automatic/content-based size remains indefinite
/// while calculating an intrinsic contribution; a definite `min-height` or
/// `max-height` can establish the basis only after it has constrained that
/// candidate.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
fn flex_item_winning_intrinsic_block_constraint(
    style: &ComputedStyle,
    intrinsic_candidate: ContentBoxLength,
    containing_block_basis: FlexAvailablePercentageBasis,
    vertical_non_content: NonContentLength,
) -> IntrinsicBlockBasis {
    if !style.box_values.height.is_auto() {
        return IntrinsicBlockBasis::Indefinite;
    }
    // Intrinsic keywords and an unresolved percentage do not create a
    // definite constraint here. In particular, do not let the content-based
    // automatic minimum leak into this path: Flexbox keeps it cyclic while
    // calculating the item's intrinsic contribution.
    let has_definite_explicit_constraint = [
        style.box_values.min_height.clone(),
        style.box_values.max_height.clone(),
    ]
    .into_iter()
    .any(|constraint| {
        used_content_box_size_with_basis(
            constraint,
            style.box_sizing,
            containing_block_basis,
            vertical_non_content,
        )
        .is_some()
    });
    if !has_definite_explicit_constraint {
        return IntrinsicBlockBasis::Indefinite;
    }
    let constrained = constrain_height_with_intrinsic(
        style,
        intrinsic_candidate,
        intrinsic_candidate,
        intrinsic_candidate,
        containing_block_basis,
        vertical_non_content,
    );
    if (constrained.points() - intrinsic_candidate.points()).abs() > 0.01 {
        IntrinsicBlockBasis::from_winning_constraint(
            constrained,
            if constrained < intrinsic_candidate {
                WinningBlockConstraintKind::Maximum
            } else {
                WinningBlockConstraintKind::Minimum
            },
        )
    } else {
        IntrinsicBlockBasis::Indefinite
    }
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
    fn definite_single_line_stretch_creates_a_content_box_percentage_basis() {
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
            height_basis: PercentageBasis::definite_from(
                content_box_pt(50.0),
                FlexAvailableSizeSource::DefiniteSingleLineStretch,
            ),
            stretched_width: None,
            stretched_height: Some(PhysicalContentHeight::new(content_box_pt(50.0))),
        };

        assert_eq!(
            flex_item_estimate_percentage_height_basis(&style, available, non_content_pt(0.0),)
                .descendant_percentage_basis()
                .value(),
            Some(content_box_pt(50.0))
        );
    }

    #[test]
    fn final_stretch_replay_cannot_create_an_intrinsic_block_basis() {
        let style = ComputedStyle::initial();
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
            flex_item_estimate_percentage_height_basis(&style, available, non_content_pt(0.0)),
            IntrinsicBlockBasis::Indefinite
        );
    }

    #[test]
    fn content_based_automatic_minimum_cannot_create_an_intrinsic_block_basis() {
        let style = ComputedStyle::initial();

        assert_eq!(
            flex_item_winning_intrinsic_block_constraint(
                &style,
                content_box_pt(200.0),
                PercentageBasis::indefinite(),
                non_content_pt(0.0),
            ),
            IntrinsicBlockBasis::Indefinite
        );
    }

    #[test]
    fn intrinsic_container_measurement_cannot_create_a_descendant_block_basis() {
        assert_eq!(
            intrinsic_block_basis_from_flex_available_height(PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::IntrinsicContainerSize,
            )),
            IntrinsicBlockBasis::Indefinite
        );
    }

    #[test]
    fn resolved_nested_flex_height_creates_a_descendant_block_basis() {
        assert_eq!(
            intrinsic_block_basis_from_flex_available_height(PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::DefinitePreferredCrossSize,
            )),
            IntrinsicBlockBasis::from_flex_layout(
                content_box_pt(100.0),
                FlexIntrinsicBlockBasisSource::DefinitePreferredSize,
            )
        );
    }

    #[test]
    fn winning_explicit_max_height_creates_an_intrinsic_block_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );

        assert_eq!(
            flex_item_winning_intrinsic_block_constraint(
                &style,
                content_box_pt(200.0),
                PercentageBasis::indefinite(),
                non_content_pt(0.0),
            ),
            IntrinsicBlockBasis::from_winning_constraint(
                content_box_pt(100.0),
                WinningBlockConstraintKind::Maximum,
            )
        );
    }

    #[test]
    fn positioned_intrinsic_measurement_does_not_reconstruct_a_flex_percentage_basis() {
        let fallback = PercentageBasis::definite(PhysicalContentHeight::new(content_box_pt(100.0)));

        assert!(
            !flex_container_height_percentage_basis_for_context(
                PercentageBasis::indefinite(),
                fallback,
                LayoutPassKind::PositionedAutoSizeMeasurement,
            )
            .is_definite()
        );
    }
}
