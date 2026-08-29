use super::*;

/// Selects how an item-alignment `auto` value crosses the Taffy boundary.
///
/// A container-level `align-items` value treats `auto` like `stretch`, while
/// an item-level override may need to preserve `auto` so the layout-mode
/// adapter can inherit from its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TaffyAutoAlignment {
    Stretch,
    Preserve,
}

/// Selects whether an unresolved pure percentage may remain symbolic in
/// Taffy's physical edge model.
///
/// CSS Box resolves edge percentages against the logical inline axis.  Grid
/// must erase a cyclic percentage before entering Taffy's physical model;
/// Flex retains its established symbolic percentage behavior for its legacy
/// line-construction path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum TaffyCyclicPercentage {
    ResolveToLengthComponent,
    PreservePurePercentage,
}

/// Convert a CSS direction into Taffy's physical direction switch.
pub(in crate::layout) fn direction(value: Direction) -> taffy::style::Direction {
    match value {
        Direction::Ltr => taffy::style::Direction::Ltr,
        Direction::Rtl => taffy::style::Direction::Rtl,
    }
}

/// Convert CSS box sizing into Taffy's box-sizing representation.
pub(in crate::layout) fn box_sizing(value: BoxSizing) -> taffy_layout::BoxSizing {
    match value {
        BoxSizing::BorderBox => taffy_layout::BoxSizing::BorderBox,
        BoxSizing::ContentBox => taffy_layout::BoxSizing::ContentBox,
    }
}

/// Convert a resolved scalar Taffy size into a min/max constraint.
///
/// Taffy 0.14 intentionally separates preferred `Dimension` values from
/// min/max `LengthPercentageAuto` constraints.  Quire resolves intrinsic and
/// mixed CSS sizing values before this boundary, so callers of this adapter
/// may only pass a scalar length, percentage, or `auto` value.
pub(in crate::layout) fn min_max_constraint(
    value: taffy_layout::Dimension,
) -> taffy_layout::LengthPercentageAuto {
    match value.expand() {
        taffy::style::ExpandedDimension::Length(value) => {
            taffy_layout::LengthPercentageAuto::length(value)
        }
        taffy::style::ExpandedDimension::Percent(value) => {
            taffy_layout::LengthPercentageAuto::percent(value)
        }
        taffy::style::ExpandedDimension::Auto => taffy_layout::LengthPercentageAuto::auto(),
        value => {
            unreachable!("min/max Taffy constraint must be resolved before conversion: {value:?}")
        }
    }
}

/// Form Taffy 0.14's leaf-layout result from Quire's measured content-box
/// geometry.  Taffy has no logical-writing-mode baseline channel, so callers
/// pass a baseline only when it is a physical top-edge offset for horizontal
/// text.
pub(in crate::layout) fn measured_leaf_output(
    size: taffy_layout::Size<f32>,
    first_baseline: Option<f32>,
    last_baseline: Option<f32>,
) -> taffy::tree::LayoutOutput {
    taffy::tree::LayoutOutput::from_sizes_and_baselines(
        size,
        taffy::geometry::Rect::ZERO,
        taffy::tree::Baselines {
            first: first_baseline,
            last: last_baseline,
        },
    )
}

/// Convert CSS alignment safety into Taffy's alignment safety.
pub(in crate::layout) fn alignment_safety(
    safety: AlignmentSafety,
) -> taffy_layout::AlignmentSafety {
    match safety {
        AlignmentSafety::Default | AlignmentSafety::Unsafe => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Safe => taffy_layout::AlignmentSafety::Safe,
    }
}

/// Convert CSS `align-content`/`justify-content` keywords into Taffy's
/// distribution model. Baseline content alignment uses its start-side
/// fallback because Taffy does not expose a content-baseline mode.
pub(in crate::layout) fn content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    let safety = alignment_safety(safety);
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::Stretch,
                safety,
            }
        }
        ContentAlignmentKeyword::Start => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        },
        ContentAlignmentKeyword::End => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        },
        ContentAlignmentKeyword::FlexStart | ContentAlignmentKeyword::Left => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        }
        ContentAlignmentKeyword::FlexEnd | ContentAlignmentKeyword::Right => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }
        ContentAlignmentKeyword::Center => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        },
        ContentAlignmentKeyword::SpaceBetween => taffy_layout::AlignContent::SPACE_BETWEEN,
        ContentAlignmentKeyword::SpaceAround => taffy_layout::AlignContent::SPACE_AROUND,
        ContentAlignmentKeyword::SpaceEvenly => taffy_layout::AlignContent::SPACE_EVENLY,
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignContent::FLEX_START
        }
    }
}

/// Convert a CSS item-alignment keyword into Taffy's common item alignment.
///
/// Taffy's measurement callback has no baseline channel.  Giving it a
/// baseline keyword would therefore let its fallback baseline algorithm affect
/// used item geometry before Flex can resolve the actual CSS baseline sets.
/// Use a cross-start placeholder instead; the Flex post-layout baseline pass
/// applies baseline sharing or fallback once typed item baselines are known:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout) fn item_alignment(
    alignment: AlignItems,
    auto: TaffyAutoAlignment,
) -> taffy_layout::AlignItems {
    let safety = alignment_safety(alignment.safety);
    match alignment.keyword {
        SelfAlignmentKeyword::Auto if auto == TaffyAutoAlignment::Preserve => {
            taffy_layout::AlignItems::STRETCH
        }
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Stretch,
            safety,
        },
        SelfAlignmentKeyword::Start => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Start,
            safety,
        },
        SelfAlignmentKeyword::End => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::End,
            safety,
        },
        // Taffy 0.13 resolves these against the item's direction.  Quire
        // retains its own vertical-writing and final-placement handling where
        // Taffy's horizontal-tb model cannot represent CSS writing modes.
        SelfAlignmentKeyword::SelfStart => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::SelfStart,
            safety,
        },
        SelfAlignmentKeyword::SelfEnd => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::SelfEnd,
            safety,
        },
        SelfAlignmentKeyword::FlexStart | SelfAlignmentKeyword::Left => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety,
        },
        SelfAlignmentKeyword::FlexEnd | SelfAlignmentKeyword::Right => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexEnd,
            safety,
        },
        SelfAlignmentKeyword::Center => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Center,
            safety,
        },
        SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignItems::FLEX_START
        }
    }
}

/// Convert used border widths to Taffy's length-only edge model.
pub(in crate::layout) fn border_edges(
    edges: css::Edges,
) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    taffy_layout::Rect {
        left: taffy_layout::LengthPercentage::length(edges.left),
        right: taffy_layout::LengthPercentage::length(edges.right),
        top: taffy_layout::LengthPercentage::length(edges.top),
        bottom: taffy_layout::LengthPercentage::length(edges.bottom),
    }
}

/// Convert a CSS gap using its typed percentage basis.
pub(in crate::layout) fn gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> taffy_layout::LengthPercentage {
    match value {
        css::ComputedGap::Normal => taffy_layout::LengthPercentage::length(0.0),
        css::ComputedGap::LengthPercentage(value) => percentage_basis
            .points()
            .map(|basis| {
                taffy_layout::LengthPercentage::length(
                    used_length_percentage(
                        value.clone(),
                        PercentageBasis::definite(layout_pt(basis.max(0.0))),
                    )
                    .points(),
                )
            })
            .unwrap_or_else(|| {
                taffy_layout::LengthPercentage::length(value.length_max_zero().points())
            }),
    }
}

/// Resolve a gap at the Quire-to-Taffy scalar boundary.
///
/// `gap` deliberately returns a Taffy length rather than forwarding a
/// percentage.  The CSS percentage basis is a Quire-owned decision, so the
/// expanded value must consequently always be an absolute length here.
pub(in crate::layout) fn resolved_gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> f32 {
    match gap(value, percentage_basis).expand() {
        taffy::style::ExpandedLengthPercentage::Length(value) => value,
        _ => unreachable!("Quire resolves a gap percentage before entering Taffy"),
    }
}

/// Convert CSS margins after resolving percentage components against the
/// caller-selected logical-inline basis.
pub(in crate::layout) fn margin<Source: Copy>(
    style: &ComputedStyle,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
    cyclic_percentage: TaffyCyclicPercentage,
) -> taffy_layout::Rect<taffy_layout::LengthPercentageAuto> {
    let edges = style.box_values.margin.clone();
    taffy_layout::Rect {
        left: margin_edge(edges.left, percentage_basis, cyclic_percentage),
        right: margin_edge(edges.right, percentage_basis, cyclic_percentage),
        top: margin_edge(edges.top, percentage_basis, cyclic_percentage),
        bottom: margin_edge(edges.bottom, percentage_basis, cyclic_percentage),
    }
}

fn margin_edge<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
    cyclic_percentage: TaffyCyclicPercentage,
) -> taffy_layout::LengthPercentageAuto {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::LengthPercentageAuto::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.needs_percentage_basis() {
                if let Some(resolved) = value.used_length_with_percentage_basis(percentage_basis) {
                    return taffy_layout::LengthPercentageAuto::length(resolved.points());
                }
                if cyclic_percentage == TaffyCyclicPercentage::ResolveToLengthComponent {
                    return taffy_layout::LengthPercentageAuto::length(value.length_points());
                }
            }
            if let Some(percent) = value
                .pure_percentage_coefficient()
                .filter(|percent| *percent != 0.0)
            {
                taffy_layout::LengthPercentageAuto::percent(percent)
            } else {
                taffy_layout::LengthPercentageAuto::length(value.length_points())
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => {
            taffy_layout::LengthPercentageAuto::auto()
        }
    }
}

/// Convert CSS padding after resolving percentage components against the
/// caller-selected logical-inline basis.
pub(in crate::layout) fn padding<Source: Copy>(
    style: &ComputedStyle,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
) -> taffy_layout::Rect<taffy_layout::LengthPercentage> {
    let edges = style.box_values.padding.clone();
    taffy_layout::Rect {
        left: padding_edge(edges.left, percentage_basis),
        right: padding_edge(edges.right, percentage_basis),
        top: padding_edge(edges.top, percentage_basis),
        bottom: padding_edge(edges.bottom, percentage_basis),
    }
}

fn padding_edge<Source>(
    value: css::ComputedLengthPercentage,
    percentage_basis: LogicalInlinePercentageBasis<Source>,
) -> taffy_layout::LengthPercentage {
    if value.needs_percentage_basis()
        && let Some(resolved) = value.used_length_with_percentage_basis(percentage_basis)
    {
        return taffy_layout::LengthPercentage::length(resolved.points());
    }
    taffy_layout::LengthPercentage::length(value.length_points())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_preserves_fixed_component_when_percentage_basis_is_indefinite() {
        let computed_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(8.0));
        assert_eq!(
            gap(
                computed_gap,
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            taffy_layout::LengthPercentage::length(8.0)
        );
    }

    #[test]
    fn gap_resolves_percentage_against_a_definite_basis() {
        let computed_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.1));
        assert_eq!(
            gap(
                computed_gap,
                PercentageBasis::definite(content_box_pt(80.0))
            ),
            taffy_layout::LengthPercentage::length(8.0)
        );
    }

    #[test]
    fn cyclic_grid_margin_percentage_does_not_become_a_physical_taffy_percentage() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        assert_eq!(
            margin(
                &style,
                PercentageBasis::<LogicalInlineContentSize>::indefinite(),
                TaffyCyclicPercentage::ResolveToLengthComponent,
            )
            .left,
            taffy_layout::LengthPercentageAuto::length(0.0)
        );
    }

    #[test]
    fn common_css_to_taffy_conversions_keep_their_layout_meaning() {
        assert_eq!(direction(Direction::Ltr), taffy::style::Direction::Ltr);
        assert_eq!(direction(Direction::Rtl), taffy::style::Direction::Rtl);
        assert_eq!(
            box_sizing(BoxSizing::BorderBox),
            taffy_layout::BoxSizing::BorderBox
        );
        assert_eq!(
            box_sizing(BoxSizing::ContentBox),
            taffy_layout::BoxSizing::ContentBox
        );
        assert_eq!(
            alignment_safety(AlignmentSafety::Default),
            taffy_layout::AlignmentSafety::Unsafe
        );
        assert_eq!(
            alignment_safety(AlignmentSafety::Safe),
            taffy_layout::AlignmentSafety::Safe
        );
        assert_eq!(
            content_alignment(ContentAlignmentKeyword::Center, AlignmentSafety::Default).keyword,
            taffy_layout::AlignContentKeyword::Center
        );
        assert_eq!(
            content_alignment(ContentAlignmentKeyword::Baseline, AlignmentSafety::Safe),
            taffy_layout::AlignContent::FLEX_START
        );
        for keyword in [
            SelfAlignmentKeyword::Baseline,
            SelfAlignmentKeyword::LastBaseline,
        ] {
            assert_eq!(
                item_alignment(AlignItems::new(keyword), TaffyAutoAlignment::Stretch),
                taffy_layout::AlignItems::FLEX_START,
                "Taffy must not synthesize a baseline before Flex resolves {keyword:?}",
            );
        }
        assert_eq!(
            item_alignment(
                AlignItems::new(SelfAlignmentKeyword::Auto),
                TaffyAutoAlignment::Preserve,
            ),
            taffy_layout::AlignItems::STRETCH
        );
        assert_eq!(
            border_edges(css::Edges {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            }),
            taffy_layout::Rect {
                top: taffy_layout::LengthPercentage::length(1.0),
                right: taffy_layout::LengthPercentage::length(2.0),
                bottom: taffy_layout::LengthPercentage::length(3.0),
                left: taffy_layout::LengthPercentage::length(4.0),
            }
        );
    }

    #[test]
    fn padding_resolves_against_the_supplied_logical_inline_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.padding.left =
            css::ComputedLengthPercentage::from_affine(layout_pt(5.0), 0.1, true);
        assert_eq!(
            padding(
                &style,
                PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(100.0))),
            )
            .left,
            taffy_layout::LengthPercentage::length(15.0)
        );
    }

    #[test]
    fn min_max_constraint_retains_only_scalar_percentage_bases() {
        assert_eq!(
            min_max_constraint(taffy_layout::Dimension::length(12.0)),
            taffy_layout::LengthPercentageAuto::length(12.0),
        );
        assert_eq!(
            min_max_constraint(taffy_layout::Dimension::percent(0.5)),
            taffy_layout::LengthPercentageAuto::percent(0.5),
        );
        assert!(min_max_constraint(taffy_layout::Dimension::auto()).is_auto());
    }

    #[test]
    fn measured_leaf_output_transports_both_horizontal_baselines() {
        let output = measured_leaf_output(
            taffy_layout::Size {
                width: 30.0,
                height: 12.0,
            },
            Some(8.0),
            Some(10.0),
        );

        assert_eq!(output.size.width, 30.0);
        assert_eq!(output.size.height, 12.0);
        assert_eq!(output.baselines.first, Some(8.0));
        assert_eq!(output.baselines.last, Some(10.0));
    }
}
