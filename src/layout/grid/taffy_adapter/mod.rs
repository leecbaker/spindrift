use std::num::NonZeroU16;

use super::{
    AlignContent, AlignItems, AlignSelf, AlignmentSafety, ComputedStyle, ContentAlignmentKeyword,
    ContentBoxLength, GridChild, GridPercentageBasis, JustifyContent, JustifyItems, JustifySelf,
    PercentageBasis, SelfAlignmentKeyword, SemanticLengthExt, content_box_pt, css, grid_line_index,
    layout_pt, negative_named_implicit_grid_line_index, taffy_layout, used_length_percentage,
};
use crate::layout::taffy_bridge;

mod placement;
use placement::{
    backward_named_span_startward_line_range, negative_named_line_startward_line_range,
};
pub(in crate::layout::grid) use placement::{
    taffy_grid_auto_flow, taffy_grid_line, taffy_grid_line_with_startward_adjustment,
};

pub(super) fn taffy_dimension(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::LengthPercentageAuto {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => {
            taffy_layout::LengthPercentageAuto::auto()
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            taffy_length_percentage(value).into()
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent => {
            taffy_layout::LengthPercentageAuto::auto()
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => limit
            .map(taffy_dimension_from_length_percentage)
            .map(taffy_bridge::min_max_constraint)
            .unwrap_or_else(taffy_layout::LengthPercentageAuto::auto),
        css::ComputedLengthPercentageOrAuto::CalcSize(_) => {
            taffy_layout::LengthPercentageAuto::auto()
        }
    }
}

pub(super) fn taffy_grid_item_dimension(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: GridPercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::Dimension {
    taffy_grid_item_dimension_for_purpose(
        value,
        percentage_basis,
        min_content,
        max_content,
        GridTaffyDimensionPurpose::UsedItemSize,
    )
}

/// Selects the CSS Grid phase that consumes the converted Taffy dimension.
///
/// A grid item's used size may retain a pure percentage until its final grid
/// area is known. A min/max constraint instead participates in track sizing,
/// where Taffy would otherwise resolve the percentage against its available
/// sizing input. CSS Grid gives those phases different percentage bases:
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing> and
/// <https://www.w3.org/TR/css-grid-1/#algo-track-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridTaffyDimensionPurpose {
    UsedItemSize,
    TrackSizingConstraint,
}

/// Convert a Grid item size or constraint into Taffy's scalar dimension model.
///
/// The CSS sizing keywords have identical behavior in Grid's item-layout and
/// track-sizing phases. Only the `<length-percentage>` branch differs, because
/// pure percentages must remain tied to the final grid area for a used item
/// size but must not be resolved by Taffy's track-sizing available space.
fn taffy_grid_item_dimension_for_purpose(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: GridPercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    purpose: GridTaffyDimensionPurpose,
) -> taffy_layout::Dimension {
    // Preferred item sizes can retain Taffy's native intrinsic keywords:
    // `taffy_grid_measurement` answers their min/max-content requests from
    // Quire's typed estimate. Track-sizing constraints intentionally stay on
    // Quire's scalar path because CSS Grid gives their percentage phase a
    // different basis.
    if purpose == GridTaffyDimensionPurpose::UsedItemSize {
        match &value {
            css::ComputedLengthPercentageOrAuto::MinContent => {
                return taffy_layout::Dimension::min_content();
            }
            css::ComputedLengthPercentageOrAuto::MaxContent => {
                return taffy_layout::Dimension::max_content();
            }
            css::ComputedLengthPercentageOrAuto::FitContent(None) => {
                return taffy_layout::Dimension::fit_content();
            }
            css::ComputedLengthPercentageOrAuto::FitContent(Some(limit))
                if limit.is_definitely_absolute() =>
            {
                return taffy_layout::Dimension::fit_content_px(limit.length_max_zero().points());
            }
            css::ComputedLengthPercentageOrAuto::Stretch => {
                return taffy_layout::Dimension::stretch();
            }
            _ => {}
        }
    }
    let min_content = min_content.points().max(0.0);
    let max_content = max_content.points().max(min_content);
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => taffy_layout::Dimension::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            match purpose {
                GridTaffyDimensionPurpose::UsedItemSize => {
                    // A grid item's percentage size resolves against its final grid
                    // area, not the grid container. Preserve a pure percentage for
                    // Taffy's grid-item resolution instead of eagerly turning it
                    // into a container-relative length:
                    // <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>.
                    if let Some(percent) = value
                        .pure_percentage_coefficient()
                        .filter(|percent| *percent != 0.0)
                    {
                        percentage_basis
                            .points()
                            .map(|_| taffy_layout::Dimension::percent(percent))
                            .unwrap_or_else(taffy_layout::Dimension::auto)
                    } else if value.contains_percentage() {
                        // A mixed length-percentage cannot be represented by
                        // Taffy's scalar percentage dimension. More
                        // importantly, its percentage component is cyclic
                        // while Grid sizes intrinsic tracks. Defer the whole
                        // value until the final grid-area sizing phase rather
                        // than resolving it against the grid container.
                        // <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
                        taffy_layout::Dimension::auto()
                    } else {
                        taffy_dimension_from_length_percentage_with_basis(value, percentage_basis)
                    }
                }
                GridTaffyDimensionPurpose::TrackSizingConstraint => {
                    // Grid item percentage sizes (including min/max
                    // constraints) are cyclic while intrinsic tracks are
                    // sized, and therefore behave as `auto`. Taffy's
                    // available space here is the grid container, not the
                    // final grid area, so it must not become the CSS basis.
                    // <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
                    if value.contains_percentage() {
                        taffy_layout::Dimension::auto()
                    } else {
                        taffy_dimension_from_length_percentage_with_basis(value, percentage_basis)
                    }
                }
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent => {
            taffy_layout::Dimension::length(min_content)
        }
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            taffy_layout::Dimension::length(max_content)
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let limit = limit
                .and_then(|limit| {
                    percentage_basis.points().map(|basis| {
                        used_length_percentage(limit, PercentageBasis::definite(layout_pt(basis)))
                            .points()
                    })
                })
                .unwrap_or(max_content);
            taffy_layout::Dimension::length(max_content.min(min_content.max(limit)).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let percentage_basis = percentage_basis.points().unwrap_or(0.0);
            let fit_content = max_content.min(min_content.max(percentage_basis));
            taffy_layout::Dimension::length(
                value
                    .used_value(
                        max_content,
                        min_content,
                        max_content,
                        fit_content,
                        percentage_basis,
                        PercentageBasis::definite(layout_pt(percentage_basis)),
                    )
                    .max(layout_pt(0.0))
                    .points(),
            )
        }
    }
}

/// Convert a grid item's min/max constraint for Taffy's track-sizing pass.
///
/// Taffy's grid algorithm resolves a percentage `Dimension` relative to its
/// available sizing input while calculating tracks. That is appropriate for a
/// used item size only after its grid area is known, but not for a constraint
/// that participates in track sizing. Resolve intrinsic values to Quire's
/// measured scalar contributions and defer cyclic percentages as `auto`.
///
/// Taffy accepts only scalar lengths, percentages, or `auto` for min/max
/// constraints. Keeping that restriction in this return type prevents the
/// preferred-size path's native intrinsic dimensions from crossing this
/// boundary.
pub(super) fn taffy_grid_item_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: GridPercentageBasis,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> taffy_layout::LengthPercentageAuto {
    taffy_bridge::min_max_constraint(taffy_grid_item_dimension_for_purpose(
        value,
        percentage_basis,
        min_content,
        max_content,
        GridTaffyDimensionPurpose::TrackSizingConstraint,
    ))
}

pub(super) fn taffy_dimension_from_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::Dimension {
    // CSS zero percentages resolve to zero even when the usual percentage
    // basis is indefinite. Do not turn an authored `0%` into `auto` while
    // translating into Taffy's scalar model.
    if value.pure_percentage_coefficient() == Some(0.0) {
        return taffy_layout::Dimension::length(0.0);
    }
    if let Some(percent) = value
        .pure_percentage_coefficient()
        .filter(|percent| *percent != 0.0)
    {
        taffy_layout::Dimension::percent(percent)
    } else if let Some(length) =
        value.used_length_with_percentage_basis(PercentageBasis::<ContentBoxLength>::indefinite())
    {
        taffy_layout::Dimension::length(length.points())
    } else {
        // Deferred metric terms must be resolved in their CSS phase before
        // Taffy receives a scalar.  Passing only their fixed component would
        // silently turn `calc(10pt + 1em)` into `10pt`.
        taffy_layout::Dimension::auto()
    }
}

/// Convert a grid item size into Taffy's model with a definite percentage basis.
///
/// CSS Sizing resolves percentages only against definite containing-block
/// sizes. For grid row intrinsic sizing the container block size can be
/// indefinite, so unresolved nonzero percentages must behave as automatic
/// sizes instead of being handed to Taffy as percentages:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing> and
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
pub(super) fn taffy_dimension_from_length_percentage_with_basis(
    value: css::ComputedLengthPercentage,
    percentage_basis: GridPercentageBasis,
) -> taffy_layout::Dimension {
    if value.pure_percentage_coefficient() == Some(0.0) {
        return taffy_layout::Dimension::length(0.0);
    }
    if let Some(basis) = percentage_basis.points()
        && let Some(length) =
            value.used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
    {
        return taffy_layout::Dimension::length(length.points());
    }
    if let Some(length) =
        value.used_length_with_percentage_basis(PercentageBasis::<ContentBoxLength>::indefinite())
    {
        return taffy_layout::Dimension::length(length.points());
    }
    taffy_layout::Dimension::auto()
}

pub(super) fn taffy_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::LengthPercentage {
    if let Some(percent) = value
        .pure_percentage_coefficient()
        .filter(|percent| *percent != 0.0)
    {
        taffy_layout::LengthPercentage::percent(percent)
    } else {
        taffy_layout::LengthPercentage::length(value.length_points())
    }
}

/// Maps CSS Box Alignment content distribution into Taffy's grid container model.
///
/// CSS Grid consumes `align-content` and `justify-content` to distribute the
/// grid tracks inside the grid container. Taffy models the common distribution
/// and positional keywords; baseline content alignment currently falls back to
/// start-side packing at this adapter boundary:
/// <https://www.w3.org/TR/css-align-3/#content-distribution> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
pub(super) fn taffy_grid_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    match keyword {
        // Taffy has no Grid content-baseline mode. CSS Align falls a
        // first/last baseline content-alignment request back to the
        // corresponding safe start/end alignment when no shared baseline can
        // be formed. Grid's own baseline resolver supplies sharing later;
        // preserve the two distinct fallback edges at this adapter boundary.
        // <https://www.w3.org/TR/css-align-3/#baseline-align-content>
        ContentAlignmentKeyword::Baseline => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety: taffy_layout::AlignmentSafety::Safe,
        },
        ContentAlignmentKeyword::LastBaseline => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety: taffy_layout::AlignmentSafety::Safe,
        },
        _ => taffy_bridge::content_alignment(keyword, safety),
    }
}

pub(super) fn taffy_grid_align_content(align_content: AlignContent) -> taffy_layout::AlignContent {
    taffy_grid_content_alignment(align_content.keyword, align_content.safety)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::grid::{GridAvailableSizeSource, grid_percentage_basis};

    #[test]
    fn no_grid_template_areas_stays_absent_at_the_taffy_boundary() {
        assert_eq!(
            taffy_grid_template_areas(&css::GridTemplateAreas::None),
            None
        );
    }

    #[test]
    fn named_grid_template_areas_keep_bounds_and_template_dimensions() {
        let template = css::GridTemplateAreas::Areas(vec![
            css::GridTemplateAreaRow {
                cells: vec![Some("head".into()), Some("head".into()), None],
            },
            css::GridTemplateAreaRow {
                cells: vec![Some("main".into()), Some("side".into()), None],
            },
        ]);

        let areas = taffy_grid_template_areas(&template).expect("authored template");

        assert_eq!(areas.row_count, 2);
        assert_eq!(areas.column_count, 3);
        assert_eq!(areas.areas.len(), 3);
        assert!(areas.areas.iter().any(|area| {
            area.name == "head"
                && area.row_start == 1
                && area.row_end == 2
                && area.column_start == 1
                && area.column_end == 3
        }));
    }

    #[test]
    fn unnamed_grid_template_cells_still_define_template_dimensions() {
        let template = css::GridTemplateAreas::Areas(vec![
            css::GridTemplateAreaRow {
                cells: vec![None, None, None],
            },
            css::GridTemplateAreaRow {
                cells: vec![None, None, None],
            },
        ]);

        let areas = taffy_grid_template_areas(&template).expect("authored template");

        assert_eq!(areas.row_count, 2);
        assert_eq!(areas.column_count, 3);
        assert!(areas.areas.is_empty());
    }

    #[test]
    fn grid_item_dimension_preserves_native_min_and_max_content_keywords() {
        let indefinite_basis: GridPercentageBasis = PercentageBasis::indefinite();
        let min_content = taffy_grid_item_dimension(
            css::ComputedLengthPercentageOrAuto::MinContent,
            indefinite_basis,
            content_box_pt(12.0),
            content_box_pt(48.0),
        );
        let max_content = taffy_grid_item_dimension(
            css::ComputedLengthPercentageOrAuto::MaxContent,
            indefinite_basis,
            content_box_pt(12.0),
            content_box_pt(48.0),
        );

        assert_eq!(min_content, taffy_layout::Dimension::min_content());
        assert_eq!(max_content, taffy_layout::Dimension::max_content());
    }

    #[test]
    fn grid_item_dimension_keeps_indefinite_percentages_auto() {
        let percent = css::ComputedLengthPercentage::from_percent(0.5);
        let indefinite_basis = PercentageBasis::indefinite();
        let definite_basis = grid_percentage_basis(
            Some(content_box_pt(80.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );

        assert_eq!(
            taffy_dimension_from_length_percentage_with_basis(percent.clone(), indefinite_basis),
            taffy_layout::Dimension::auto()
        );
        assert_eq!(
            taffy_dimension_from_length_percentage_with_basis(percent, definite_basis),
            taffy_layout::Dimension::length(40.0)
        );
    }

    #[test]
    fn grid_used_sizes_and_track_constraints_keep_percentage_phases_distinct() {
        let percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        let definite_basis = grid_percentage_basis(
            Some(content_box_pt(80.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );
        let indefinite_basis = PercentageBasis::indefinite();

        assert_eq!(
            taffy_grid_item_dimension(
                percent.clone(),
                definite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            ),
            taffy_layout::Dimension::percent(0.5),
        );
        assert_eq!(
            taffy_grid_item_constraint(
                percent.clone(),
                definite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            ),
            taffy_layout::LengthPercentageAuto::auto(),
        );
        assert!(
            taffy_grid_item_dimension(
                percent.clone(),
                indefinite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            )
            .is_auto()
        );
        assert!(
            taffy_grid_item_constraint(
                percent,
                indefinite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            )
            .is_auto()
        );
    }

    #[test]
    fn grid_preferred_sizes_delegate_exact_keywords_but_constraints_remain_scalar() {
        let basis = grid_percentage_basis(
            Some(content_box_pt(80.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );
        let min_content = content_box_pt(12.0);
        let max_content = content_box_pt(48.0);
        let fit_content = css::ComputedLengthPercentageOrAuto::FitContent(Some(
            css::ComputedLengthPercentage::from_points(20.0),
        ));
        let calc_size = css::ComputedLengthPercentageOrAuto::CalcSize(css::CalcSize {
            basis: css::CalcSizeBasis::Auto,
            size_multiplier: 0.5,
            additive: css::ComputedLengthPercentage::from_points(4.0),
            lower_bound: None,
            upper_bound: None,
        });

        assert_eq!(
            taffy_grid_item_dimension(fit_content.clone(), basis, min_content, max_content),
            taffy_layout::Dimension::fit_content_px(20.0),
        );
        assert_eq!(
            taffy_grid_item_constraint(fit_content, basis, min_content, max_content),
            taffy_layout::LengthPercentageAuto::length(20.0),
        );
        assert_eq!(
            taffy_grid_item_dimension(calc_size.clone(), basis, min_content, max_content),
            taffy_layout::Dimension::length(28.0),
        );
        assert_eq!(
            taffy_grid_item_constraint(calc_size, basis, min_content, max_content),
            taffy_layout::LengthPercentageAuto::length(28.0),
        );
    }

    #[test]
    fn grid_track_sizing_intrinsic_constraints_are_scalar() {
        let basis: GridPercentageBasis = PercentageBasis::indefinite();
        let min_content = content_box_pt(12.0);
        let max_content = content_box_pt(48.0);

        assert_eq!(
            taffy_grid_item_constraint(
                css::ComputedLengthPercentageOrAuto::Auto,
                basis,
                min_content,
                max_content,
            ),
            taffy_layout::LengthPercentageAuto::auto(),
        );
        assert_eq!(
            taffy_grid_item_constraint(
                css::ComputedLengthPercentageOrAuto::MinContent,
                basis,
                min_content,
                max_content,
            ),
            taffy_layout::LengthPercentageAuto::length(12.0),
        );
        assert_eq!(
            taffy_grid_item_constraint(
                css::ComputedLengthPercentageOrAuto::MaxContent,
                basis,
                min_content,
                max_content,
            ),
            taffy_layout::LengthPercentageAuto::length(48.0),
        );
    }

    #[test]
    fn grid_dimension_does_not_treat_unresolved_metrics_as_zero() {
        let deferred = css::ComputedLengthPercentage::sum(
            css::ComputedLengthPercentage::from_points(10.0),
            css::ComputedLengthPercentage::from_em(1.0),
        );
        let indefinite_basis = PercentageBasis::indefinite();

        assert_eq!(
            taffy_dimension_from_length_percentage(deferred.clone()),
            taffy_layout::Dimension::auto(),
        );
        assert_eq!(
            taffy_dimension_from_length_percentage_with_basis(deferred, indefinite_basis),
            taffy_layout::Dimension::auto(),
        );
    }

    #[test]
    fn grid_dimension_keeps_authored_zero_percentage_as_zero_without_a_basis() {
        let zero_percent = css::ComputedLengthPercentage::from_percent(0.0);

        assert_eq!(
            taffy_dimension_from_length_percentage_with_basis(
                zero_percent,
                PercentageBasis::indefinite(),
            ),
            taffy_layout::Dimension::length(0.0),
        );
    }

    #[test]
    fn grid_item_cyclic_zero_percentage_stays_automatic_during_track_sizing() {
        let zero_percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.0),
        );
        let indefinite_basis = PercentageBasis::indefinite();

        assert_eq!(
            taffy_grid_item_dimension(
                zero_percent.clone(),
                indefinite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            ),
            taffy_layout::Dimension::auto(),
        );
        assert_eq!(
            taffy_grid_item_constraint(
                zero_percent,
                indefinite_basis,
                content_box_pt(12.0),
                content_box_pt(48.0),
            ),
            taffy_layout::LengthPercentageAuto::auto(),
        );
    }

    #[test]
    fn grid_gap_resolves_percentages_only_with_definite_basis() {
        let percent_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.5));
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(4.0), 0.5, true);
        let mixed_gap = css::ComputedGap::LengthPercentage(mixed);
        let indefinite_basis: GridPercentageBasis = PercentageBasis::indefinite();
        let definite_basis = grid_percentage_basis(
            Some(content_box_pt(40.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );

        assert_eq!(
            taffy_bridge::gap(percent_gap.clone(), indefinite_basis),
            taffy_layout::LengthPercentage::length(0.0)
        );
        assert_eq!(
            taffy_bridge::gap(percent_gap, definite_basis),
            taffy_layout::LengthPercentage::length(20.0)
        );
        assert_eq!(
            taffy_bridge::gap(mixed_gap.clone(), indefinite_basis),
            taffy_layout::LengthPercentage::length(4.0)
        );
        assert_eq!(
            taffy_bridge::gap(mixed_gap, definite_basis),
            taffy_layout::LengthPercentage::length(24.0)
        );
    }

    #[test]
    fn grid_content_baseline_fallback_preserves_first_and_last_edges() {
        let first = taffy_grid_content_alignment(
            ContentAlignmentKeyword::Baseline,
            AlignmentSafety::Default,
        );
        let last = taffy_grid_content_alignment(
            ContentAlignmentKeyword::LastBaseline,
            AlignmentSafety::Default,
        );

        assert_eq!(first.keyword, taffy_layout::AlignContentKeyword::Start);
        assert_eq!(last.keyword, taffy_layout::AlignContentKeyword::End);
        assert_eq!(first.safety, taffy_layout::AlignmentSafety::Safe);
        assert_eq!(last.safety, taffy_layout::AlignmentSafety::Safe);
    }

    #[test]
    fn zero_breadth_auto_repeat_uses_nonzero_counting_floor() {
        let track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::ZERO),
            max: css::GridMaxTrackBreadth::Flex(1.0),
        };

        assert_eq!(
            taffy_auto_repeat_track_size(&track, GridPercentageBasis::indefinite(),).min,
            taffy_layout::MinTrackSizingFunction::length(0.75)
        );
    }

    #[test]
    fn definite_auto_repeat_calc_tracks_resolve_before_taffy_counting() {
        let track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_percent(1.0),
                css::ComputedLengthPercentage::from_points(-7.5),
            )),
            max: css::GridMaxTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_percent(1.0),
                css::ComputedLengthPercentage::from_points(-75.0),
            )),
        };
        let basis = grid_percentage_basis(
            Some(content_box_pt(75.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );

        let converted = taffy_auto_repeat_track_size(&track, basis);

        assert_eq!(
            converted.min,
            taffy_layout::MinTrackSizingFunction::length(67.5)
        );
        assert_eq!(
            converted.max,
            taffy_layout::MaxTrackSizingFunction::length(0.0)
        );
    }

    #[test]
    fn indefinite_auto_repeat_calc_tracks_keep_the_deferred_counting_floor() {
        let track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_percent(1.0),
                css::ComputedLengthPercentage::from_points(-7.5),
            )),
            max: css::GridMaxTrackBreadth::LengthPercentage(css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_percent(1.0),
                css::ComputedLengthPercentage::from_points(-75.0),
            )),
        };

        let converted = taffy_auto_repeat_track_size(&track, GridPercentageBasis::indefinite());

        assert_eq!(
            converted.min,
            taffy_layout::MinTrackSizingFunction::length(0.75)
        );
    }

    #[test]
    fn auto_repeat_conversion_retains_fill_and_fit_markers() {
        let track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(1.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(1.0),
            ),
        };
        let basis = grid_percentage_basis(
            Some(content_box_pt(75.0)),
            GridAvailableSizeSource::ContainerInlineSize,
        );

        for (count, expected) in [
            (
                css::GridRepeatCount::AutoFill,
                taffy_layout::RepetitionCount::AutoFill,
            ),
            (
                css::GridRepeatCount::AutoFit,
                taffy_layout::RepetitionCount::AutoFit,
            ),
        ] {
            let component = css::GridTrackListComponent::Repeat(
                Vec::new(),
                css::GridRepeat {
                    count,
                    tracks: vec![css::GridTrackListComponent::Track(
                        Vec::new(),
                        track.clone(),
                    )],
                    trailing_names: Vec::new(),
                },
            );
            let taffy_layout::GridTemplateComponent::Repeat(repeat) =
                taffy_grid_template_component(&component, basis)
            else {
                panic!("auto-repeat should remain a Taffy repeat");
            };

            assert_eq!(repeat.count, expected);
            assert_eq!(
                repeat.tracks[0].min,
                taffy_layout::MinTrackSizingFunction::length(75.0)
            );
        }
    }

    fn fixed_track() -> css::GridTrackSize {
        css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(10.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(10.0),
            ),
        }
    }

    #[test]
    fn simple_auto_repeat_count_distinguishes_absent_resolved_and_indeterminate_inputs() {
        let track = css::GridTrackListComponent::Track(Vec::new(), fixed_track());
        assert_eq!(
            simple_fixed_auto_repeat_count(
                &[track],
                css::ComputedGap::Normal,
                content_box_pt(35.0),
            ),
            Some(SimpleAutoRepeatCount::NoAutoRepeat),
        );

        let repeat = css::GridTrackListComponent::Repeat(
            Vec::new(),
            css::GridRepeat {
                count: css::GridRepeatCount::AutoFill,
                tracks: vec![css::GridTrackListComponent::Track(
                    Vec::new(),
                    fixed_track(),
                )],
                trailing_names: Vec::new(),
            },
        );
        assert_eq!(
            simple_fixed_auto_repeat_count(
                &[repeat],
                css::ComputedGap::Normal,
                content_box_pt(35.0),
            ),
            Some(SimpleAutoRepeatCount::Count(
                NonZeroU16::new(3).expect("three is nonzero"),
            )),
        );

        let empty_repeat = css::GridTrackListComponent::Repeat(
            Vec::new(),
            css::GridRepeat {
                count: css::GridRepeatCount::AutoFit,
                tracks: Vec::new(),
                trailing_names: Vec::new(),
            },
        );
        assert_eq!(
            simple_fixed_auto_repeat_count(
                &[empty_repeat],
                css::ComputedGap::Normal,
                content_box_pt(35.0),
            ),
            None,
        );
    }
}

pub(super) fn taffy_grid_justify_content(
    justify_content: JustifyContent,
) -> taffy_layout::JustifyContent {
    let alignment = taffy_grid_content_alignment(justify_content.keyword, justify_content.safety);
    taffy_layout::JustifyContent {
        keyword: alignment.keyword,
        safety: alignment.safety,
    }
}

/// Maps CSS self-alignment into Taffy's grid item alignment model.
///
/// CSS Grid applies `align-items`/`justify-items` as defaults for grid items
/// and lets `align-self`/`justify-self` override them. Baseline alignment and
/// writing-mode-sensitive self-start/self-end need Quire-owned follow-up
/// handling after Taffy for cases outside the adapter's common positional and
/// stretch keyword mapping:
/// <https://www.w3.org/TR/css-align-3/#self-alignment> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
pub(super) fn taffy_grid_items_alignment(alignment: AlignItems) -> taffy_layout::AlignItems {
    taffy_bridge::item_alignment(alignment, taffy_bridge::TaffyAutoAlignment::Stretch)
}

pub(super) fn taffy_grid_self_alignment(alignment: AlignSelf) -> taffy_layout::AlignSelf {
    let items = taffy_grid_items_alignment(alignment);
    taffy_layout::AlignSelf {
        keyword: items.keyword,
        safety: items.safety,
    }
}

pub(super) fn taffy_grid_align_items(align_items: AlignItems) -> taffy_layout::AlignItems {
    taffy_grid_items_alignment(align_items)
}

pub(super) fn taffy_grid_justify_items(justify_items: JustifyItems) -> taffy_layout::AlignItems {
    taffy_grid_items_alignment(justify_items)
}

pub(super) fn taffy_effective_grid_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<taffy_layout::AlignSelf> {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        None
    } else {
        Some(taffy_grid_self_alignment(effective_grid_align_self(
            child_style,
            container_style,
        )))
    }
}

pub(super) fn taffy_effective_grid_justify_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<taffy_layout::AlignSelf> {
    if child_style.justify_self.keyword == SelfAlignmentKeyword::Auto {
        None
    } else {
        Some(taffy_grid_self_alignment(effective_grid_justify_self(
            child_style,
            container_style,
        )))
    }
}

pub(super) fn effective_grid_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

pub(super) fn effective_grid_justify_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> JustifySelf {
    if child_style.justify_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.justify_items
    } else {
        child_style.justify_self
    }
}

pub(super) fn taffy_grid_template_tracks(
    tracks: &css::GridTrackList,
    areas: &css::GridTemplateAreas,
    auto_tracks: &css::GridAutoTrackList,
    axis: GridAxis,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    let mut components = match tracks {
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => Vec::new(),
        css::GridTrackList::Tracks { components, .. } => components
            .iter()
            .map(|component| taffy_grid_template_component(component, auto_repeat_percentage_basis))
            .collect(),
    };
    let Some(track_count) = grid_template_track_component_count(tracks) else {
        return components;
    };
    let area_track_count = grid_template_area_track_count(areas, axis);
    if area_track_count <= track_count {
        return components;
    }
    components.extend((0..area_track_count - track_count).map(|index| {
        taffy_layout::GridTemplateComponent::Single(taffy_track_size(
            auto_tracks
                .get(index % auto_tracks.len())
                .expect("grid auto-track list is non-empty"),
        ))
    }));
    components
}

pub(super) fn taffy_grid_template_columns_with_startward_adjustment(
    style: &ComputedStyle,
    adjustment: &StartwardImplicitTrackAdjustment,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    taffy_grid_template_tracks_with_startward_adjustment(
        style,
        GridAxis::Column,
        adjustment,
        auto_repeat_percentage_basis,
    )
}

pub(super) fn taffy_grid_template_rows_with_startward_adjustment(
    style: &ComputedStyle,
    adjustment: &StartwardImplicitTrackAdjustment,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    taffy_grid_template_tracks_with_startward_adjustment(
        style,
        GridAxis::Row,
        adjustment,
        auto_repeat_percentage_basis,
    )
}

fn taffy_grid_template_tracks_with_startward_adjustment(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    let (tracks, areas, auto_tracks) = grid_template_axis_inputs(style, axis);
    let mut components = if adjustment.before_count > 0
        && let Some(auto_repeat_count) = adjustment.auto_repeat_count
    {
        expanded_grid_template_tracks_with_auto_repeat_count(
            tracks,
            areas,
            auto_tracks,
            axis,
            auto_repeat_count,
            auto_repeat_percentage_basis,
        )
    } else {
        taffy_grid_template_tracks(
            tracks,
            areas,
            auto_tracks,
            axis,
            auto_repeat_percentage_basis,
        )
    };
    if adjustment.before_count == 0 {
        return components;
    }
    let mut prefix = (0..adjustment.before_count)
        .filter_map(|index| {
            let distance_from_explicit = adjustment.before_count - index;
            startward_auto_track_size(auto_tracks, distance_from_explicit)
                .map(|size| taffy_layout::GridTemplateComponent::Single(taffy_track_size(&size)))
        })
        .collect::<Vec<_>>();
    prefix.extend(components);
    components = prefix;
    components
}

pub(super) fn taffy_grid_template_column_names_with_startward_adjustment(
    style: &ComputedStyle,
    adjustment: &StartwardImplicitTrackAdjustment,
) -> Vec<Vec<String>> {
    taffy_grid_template_line_names_with_startward_adjustment(style, GridAxis::Column, adjustment)
}

pub(super) fn taffy_grid_template_row_names_with_startward_adjustment(
    style: &ComputedStyle,
    adjustment: &StartwardImplicitTrackAdjustment,
) -> Vec<Vec<String>> {
    taffy_grid_template_line_names_with_startward_adjustment(style, GridAxis::Row, adjustment)
}

fn taffy_grid_template_line_names_with_startward_adjustment(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
) -> Vec<Vec<String>> {
    let (tracks, areas, _) = grid_template_axis_inputs(style, axis);
    let mut line_names = if adjustment.before_count > 0 && adjustment.auto_repeat_count.is_some() {
        adjustment.explicit_line_names.clone()
    } else {
        taffy_grid_template_line_names_without_generated_areas(tracks)
    };
    if adjustment.before_count == 0 {
        add_generated_area_line_names(&mut line_names, areas, axis);
        return line_names;
    }
    let mut prefix = vec![Vec::new(); adjustment.before_count];
    prefix.extend(line_names);
    line_names = prefix;
    add_shifted_generated_area_line_names(&mut line_names, areas, axis, adjustment.before_count);
    line_names
}

/// Convert template areas after startward implicit-column expansion.
///
/// CSS Grid's explicit grid can be enlarged by `grid-template-areas`, and
/// missing named span lines can synthesize startward implicit tracks. When
/// Quire prepends those tracks for Taffy, area coordinates must be shifted so
/// generated `*-start`/`*-end` lines remain attached to their explicit area
/// columns:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids> and
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-span-int>.
pub(super) fn taffy_grid_template_areas_with_startward_adjustment(
    areas: &css::GridTemplateAreas,
    row_adjustment: &StartwardImplicitTrackAdjustment,
    column_adjustment: &StartwardImplicitTrackAdjustment,
) -> Option<taffy::style::GridTemplateAreas<String>> {
    let mut template_areas = taffy_grid_template_areas(areas)?;
    if row_adjustment.before_count == 0 && column_adjustment.before_count == 0 {
        return Some(template_areas);
    }
    let row_shift = u16::try_from(row_adjustment.before_count).unwrap_or(0);
    let column_shift = u16::try_from(column_adjustment.before_count).unwrap_or(0);
    for area in &mut template_areas.areas {
        area.row_start = area.row_start.saturating_add(row_shift);
        area.row_end = area.row_end.saturating_add(row_shift);
        area.column_start = area.column_start.saturating_add(column_shift);
        area.column_end = area.column_end.saturating_add(column_shift);
    }
    Some(template_areas)
}

/// Startward implicit tracks that must be made visible to Taffy.
///
/// CSS Grid treats missing named lines in the search direction as existing on
/// implicit grid lines. Taffy currently resolves a backward named span with a
/// missing name on the after-explicit side, so Quire pre-expands simple
/// before-explicit tracks and rewrites the affected item placement into
/// numeric line coordinates before calling Taffy:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-span-int>.
#[derive(Debug, Clone, Default)]
pub(super) struct StartwardImplicitTrackAdjustment {
    before_count: usize,
    explicit_line_names: Vec<Vec<String>>,
    auto_repeat_count: Option<u16>,
}

impl StartwardImplicitTrackAdjustment {
    pub(super) fn has_startward_tracks(&self) -> bool {
        self.before_count > 0
    }

    fn line_shift(&self) -> i32 {
        i32::try_from(self.before_count).unwrap_or(0)
    }
}

pub(super) fn taffy_startward_implicit_column_adjustment(
    style: &ComputedStyle,
    children: &[GridChild<'_>],
    percentage_basis: GridPercentageBasis,
) -> StartwardImplicitTrackAdjustment {
    taffy_startward_implicit_track_adjustment(style, children, GridAxis::Column, percentage_basis)
}

pub(super) fn taffy_startward_implicit_row_adjustment(
    style: &ComputedStyle,
    children: &[GridChild<'_>],
    percentage_basis: GridPercentageBasis,
) -> StartwardImplicitTrackAdjustment {
    taffy_startward_implicit_track_adjustment(style, children, GridAxis::Row, percentage_basis)
}

fn taffy_startward_implicit_track_adjustment(
    style: &ComputedStyle,
    children: &[GridChild<'_>],
    axis: GridAxis,
    percentage_basis: GridPercentageBasis,
) -> StartwardImplicitTrackAdjustment {
    let Some(explicit) =
        simple_explicit_line_names_for_startward_adjustment(style, axis, percentage_basis)
    else {
        return StartwardImplicitTrackAdjustment::default();
    };
    let before_count = children
        .iter()
        .filter_map(|child| {
            let (start, end) = match axis {
                GridAxis::Row => (&child.style.grid_row_start, &child.style.grid_row_end),
                GridAxis::Column => (&child.style.grid_column_start, &child.style.grid_column_end),
            };
            backward_named_span_startward_line_range(start, end, &explicit.line_names).or_else(
                || negative_named_line_startward_line_range(start, end, &explicit.line_names),
            )
        })
        .filter_map(|range| {
            (range.start < 1)
                .then(|| usize::try_from(1_i32 - range.start).ok())
                .flatten()
        })
        .max()
        .unwrap_or(0);
    StartwardImplicitTrackAdjustment {
        before_count,
        explicit_line_names: explicit.line_names,
        auto_repeat_count: explicit.auto_repeat_count,
    }
}

struct StartwardAdjustmentExplicitLines {
    line_names: Vec<Vec<String>>,
    auto_repeat_count: Option<u16>,
}

fn simple_explicit_line_names_for_startward_adjustment(
    style: &ComputedStyle,
    axis: GridAxis,
    percentage_basis: GridPercentageBasis,
) -> Option<StartwardAdjustmentExplicitLines> {
    let (tracks, areas, _, gap) = grid_template_axis_inputs_with_gap(style, axis);
    let area_track_count = grid_template_area_track_count(areas, axis);
    let (mut line_names, auto_repeat_count) = match tracks {
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => {
            (vec![Vec::new(); area_track_count + 1], None)
        }
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } => {
            let auto_repeat_count = if grid_track_components_have_auto_repeat(components) {
                let SimpleAutoRepeatCount::Count(count) = simple_fixed_auto_repeat_count(
                    components,
                    gap,
                    content_box_pt(percentage_basis.points()?.max(0.0)),
                )?
                else {
                    return None;
                };
                Some(count.get())
            } else {
                None
            };
            (
                startward_adjustment_explicit_grid_line_names(
                    components,
                    trailing_names,
                    auto_repeat_count,
                )?,
                auto_repeat_count,
            )
        }
    };
    let explicit_track_count = line_names.len().saturating_sub(1).max(area_track_count);
    line_names.resize_with(explicit_track_count + 1, Vec::new);
    add_generated_area_line_names(&mut line_names, areas, axis);
    Some(StartwardAdjustmentExplicitLines {
        line_names,
        auto_repeat_count,
    })
}

fn grid_track_components_have_auto_repeat(components: &[css::GridTrackListComponent]) -> bool {
    components.iter().any(|component| match component {
        css::GridTrackListComponent::Track(_, _) => false,
        css::GridTrackListComponent::Repeat(_, repeat) => {
            matches!(
                repeat.count,
                css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
            ) || grid_track_components_have_auto_repeat(&repeat.tracks)
        }
    })
}

fn startward_adjustment_explicit_grid_line_names(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
    auto_repeat_count: Option<u16>,
) -> Option<Vec<Vec<String>>> {
    let mut line_names = Vec::new();
    let mut current_line_names = Vec::new();
    collect_startward_adjustment_line_names(
        components,
        auto_repeat_count,
        &mut current_line_names,
        &mut line_names,
    )?;
    current_line_names.extend(trailing_names.iter().cloned());
    line_names.push(current_line_names);
    Some(line_names)
}

fn collect_startward_adjustment_line_names(
    components: &[css::GridTrackListComponent],
    auto_repeat_count: Option<u16>,
    current_line_names: &mut Vec<String>,
    line_names: &mut Vec<Vec<String>>,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(names, _) => {
                current_line_names.extend(names.iter().cloned());
                line_names.push(std::mem::take(current_line_names));
            }
            css::GridTrackListComponent::Repeat(names, repeat) => {
                current_line_names.extend(names.iter().cloned());
                let count = match repeat.count {
                    css::GridRepeatCount::Number(count) => count,
                    css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => {
                        auto_repeat_count?
                    }
                };
                for _ in 0..count {
                    collect_startward_adjustment_line_names(
                        &repeat.tracks,
                        auto_repeat_count,
                        current_line_names,
                        line_names,
                    )?;
                    current_line_names.extend(repeat.trailing_names.iter().cloned());
                }
            }
        }
    }
    Some(())
}

/// Compute the definite same-page count for fixed-size auto-repeat.
///
/// This mirrors the fixed-size branch of Taffy's explicit-grid initialization
/// so Quire can resolve startward implicit named spans before constructing the
/// Taffy tree. Startward auto-fit support currently freezes the repeat count
/// for simple occupied-track placement; broader empty-track collapse
/// interactions remain tracked as a grid placement divergence:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
/// The result of resolving a fixed-size auto-repeat fragment.
///
/// `Option` around this type denotes an unsupported or indeterminate input;
/// the enum itself distinguishes a valid non-auto-repeat list from a resolved
/// auto-repeat count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::grid) enum SimpleAutoRepeatCount {
    NoAutoRepeat,
    Count(NonZeroU16),
}

pub(in crate::layout::grid) fn simple_fixed_auto_repeat_count(
    components: &[css::GridTrackListComponent],
    gap: css::ComputedGap,
    container_size: ContentBoxLength,
) -> Option<SimpleAutoRepeatCount> {
    let mut auto_repeat = None;
    let mut non_auto_track_count = 0_usize;
    let mut non_auto_track_used_space = content_box_pt(0.0);
    for component in components {
        collect_auto_repeat_count_inputs(
            component,
            container_size,
            &mut auto_repeat,
            &mut non_auto_track_count,
            &mut non_auto_track_used_space,
        )?;
    }
    let Some(auto_repeat) = auto_repeat else {
        return Some(SimpleAutoRepeatCount::NoAutoRepeat);
    };
    let AutoRepeatCountInput {
        track_count,
        used_space,
    } = auto_repeat;
    if track_count == 0 || used_space <= content_box_pt(0.0) {
        return None;
    }
    let gap = definite_auto_repeat_gap(gap, container_size)?;
    let first_repeat_size = content_box_pt(
        non_auto_track_used_space.points()
            + used_space.points()
            + (non_auto_track_count + track_count).saturating_sub(1) as f32 * gap.points(),
    );
    let count = if first_repeat_size > container_size {
        NonZeroU16::MIN
    } else {
        let per_repeat_size = used_space.points() + track_count as f32 * gap.points();
        let count = ((container_size.points() - first_repeat_size.points()) / per_repeat_size)
            .floor()
            .max(0.0)
            + 1.0;
        if !count.is_finite() || count > f32::from(u16::MAX) {
            return None;
        }
        // The finite range check above makes this conversion lossless.
        let count = count as u16;
        NonZeroU16::new(count).expect("auto-repeat count is at least one")
    };
    Some(SimpleAutoRepeatCount::Count(count))
}

#[derive(Debug, Clone, Copy)]
struct AutoRepeatCountInput {
    track_count: usize,
    used_space: ContentBoxLength,
}

fn collect_auto_repeat_count_inputs(
    component: &css::GridTrackListComponent,
    container_size: ContentBoxLength,
    auto_repeat: &mut Option<AutoRepeatCountInput>,
    non_auto_track_count: &mut usize,
    non_auto_track_used_space: &mut ContentBoxLength,
) -> Option<()> {
    match component {
        css::GridTrackListComponent::Track(_, size) => {
            *non_auto_track_count = non_auto_track_count.checked_add(1)?;
            *non_auto_track_used_space += definite_auto_repeat_track_size(size, container_size)?;
        }
        css::GridTrackListComponent::Repeat(_, repeat) => match repeat.count {
            css::GridRepeatCount::Number(count) => {
                let mut track_count = 0_usize;
                let mut used_space = content_box_pt(0.0);
                collect_repeat_track_count_and_used_space(
                    &repeat.tracks,
                    container_size,
                    &mut track_count,
                    &mut used_space,
                )?;
                *non_auto_track_count =
                    non_auto_track_count.checked_add(usize::from(count) * track_count)?;
                *non_auto_track_used_space = content_box_pt(
                    non_auto_track_used_space.points() + used_space.points() * f32::from(count),
                );
            }
            css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => {
                if auto_repeat.is_some() {
                    return None;
                }
                let mut track_count = 0_usize;
                let mut used_space = content_box_pt(0.0);
                collect_repeat_track_count_and_used_space(
                    &repeat.tracks,
                    container_size,
                    &mut track_count,
                    &mut used_space,
                )?;
                *auto_repeat = Some(AutoRepeatCountInput {
                    track_count,
                    used_space,
                });
            }
        },
    }
    Some(())
}

fn collect_repeat_track_count_and_used_space(
    components: &[css::GridTrackListComponent],
    container_size: ContentBoxLength,
    track_count: &mut usize,
    used_space: &mut ContentBoxLength,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, size) => {
                *track_count = track_count.checked_add(1)?;
                *used_space = content_box_pt(
                    used_space.points()
                        + definite_auto_repeat_track_size(size, container_size)?.points(),
                );
            }
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(count) = repeat.count else {
                    return None;
                };
                let mut repeated_count = 0_usize;
                let mut repeated_space = content_box_pt(0.0);
                collect_repeat_track_count_and_used_space(
                    &repeat.tracks,
                    container_size,
                    &mut repeated_count,
                    &mut repeated_space,
                )?;
                *track_count = track_count.checked_add(usize::from(count) * repeated_count)?;
                *used_space = content_box_pt(
                    used_space.points() + repeated_space.points() * f32::from(count),
                );
            }
        }
    }
    Some(())
}

pub(in crate::layout::grid) fn definite_auto_repeat_track_size(
    size: &css::GridTrackSize,
    container_size: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let max = match &size.max {
        css::GridMaxTrackBreadth::LengthPercentage(value)
        | css::GridMaxTrackBreadth::FitContent(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                container_size.points(),
            )))
            .map(|value| content_box_pt(value.points())),
        css::GridMaxTrackBreadth::Auto
        | css::GridMaxTrackBreadth::MinContent
        | css::GridMaxTrackBreadth::MaxContent
        | css::GridMaxTrackBreadth::Flex(_) => None,
    };
    let min = match &size.min {
        css::GridMinTrackBreadth::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                container_size.points(),
            )))
            .map(|value| content_box_pt(value.points())),
        css::GridMinTrackBreadth::Auto
        | css::GridMinTrackBreadth::MinContent
        | css::GridMinTrackBreadth::MaxContent => None,
    };
    max.map(|max| max.max(min.unwrap_or(content_box_pt(0.0))))
        .or(min)
        .map(|size| size.max(content_box_pt(0.0)))
}

fn definite_auto_repeat_gap(
    gap: css::ComputedGap,
    container_size: ContentBoxLength,
) -> Option<ContentBoxLength> {
    match gap {
        css::ComputedGap::Normal => Some(content_box_pt(0.0)),
        css::ComputedGap::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                container_size.points(),
            )))
            .map(|gap| content_box_pt(gap.points()).max(content_box_pt(0.0))),
    }
}

fn taffy_grid_template_line_names_without_generated_areas(
    tracks: &css::GridTrackList,
) -> Vec<Vec<String>> {
    match tracks {
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => Vec::new(),
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } => {
            let mut line_names = Vec::with_capacity(components.len() + 1);
            for component in components {
                match component {
                    css::GridTrackListComponent::Track(names, _)
                    | css::GridTrackListComponent::Repeat(names, _) => {
                        line_names.push(names.clone());
                    }
                }
            }
            line_names.push(trailing_names.clone());
            line_names
        }
    }
}

fn add_shifted_generated_area_line_names(
    line_names: &mut Vec<Vec<String>>,
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
    shift: usize,
) {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return;
    };
    for area in collect_grid_template_area_bounds(rows) {
        let (start, end) = axis.area_line_range(&area, shift);
        ensure_grid_line_names_length(line_names, end + 1);
        add_grid_line_name(&mut line_names[start], format!("{}-start", area.name));
        add_grid_line_name(&mut line_names[end], format!("{}-end", area.name));
    }
}

fn startward_auto_track_size(
    auto_tracks: &css::GridAutoTrackList,
    distance_from_explicit: usize,
) -> Option<css::GridTrackSize> {
    let len = auto_tracks.len();
    let offset = distance_from_explicit % len;
    let index = (len - offset) % len;
    auto_tracks.get(index).cloned()
}

fn expanded_grid_template_tracks_with_auto_repeat_count(
    tracks: &css::GridTrackList,
    areas: &css::GridTemplateAreas,
    auto_tracks: &css::GridAutoTrackList,
    axis: GridAxis,
    auto_repeat_count: u16,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    let mut components = match tracks {
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => Vec::new(),
        css::GridTrackList::Tracks { components, .. } => components
            .iter()
            .flat_map(|component| {
                expanded_grid_template_component_with_auto_repeat_count(
                    component,
                    auto_repeat_count,
                    auto_repeat_percentage_basis,
                    false,
                )
            })
            .collect(),
    };
    let area_track_count = grid_template_area_track_count(areas, axis);
    if components.len() < area_track_count {
        components.extend((0..area_track_count - components.len()).map(|index| {
            taffy_layout::GridTemplateComponent::Single(taffy_track_size(
                auto_tracks
                    .get(index % auto_tracks.len())
                    .expect("grid auto-track list is non-empty"),
            ))
        }));
    }
    components
}

fn expanded_grid_template_component_with_auto_repeat_count(
    component: &css::GridTrackListComponent,
    auto_repeat_count: u16,
    auto_repeat_percentage_basis: GridPercentageBasis,
    is_auto_repeat_track: bool,
) -> Vec<taffy_layout::GridTemplateComponent<String>> {
    match component {
        css::GridTrackListComponent::Track(_, size) => {
            vec![taffy_layout::GridTemplateComponent::Single(
                if is_auto_repeat_track {
                    taffy_auto_repeat_track_size(size, auto_repeat_percentage_basis)
                } else {
                    taffy_track_size(size)
                },
            )]
        }
        css::GridTrackListComponent::Repeat(_, repeat) => {
            let count = match repeat.count {
                css::GridRepeatCount::Number(count) => count,
                css::GridRepeatCount::AutoFill => auto_repeat_count,
                css::GridRepeatCount::AutoFit => auto_repeat_count,
            };
            let is_auto_repeat = matches!(
                repeat.count,
                css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit
            );
            (0..count)
                .flat_map(|_| {
                    repeat.tracks.iter().flat_map(|component| {
                        expanded_grid_template_component_with_auto_repeat_count(
                            component,
                            auto_repeat_count,
                            auto_repeat_percentage_basis,
                            is_auto_repeat_track || is_auto_repeat,
                        )
                    })
                })
                .collect()
        }
    }
}

/// Resolve the physical range occupied by an `auto-fit` repeat after any
/// startward implicit tracks have been prepended for Taffy.
pub(super) fn auto_fit_track_range_with_startward_adjustment(
    style: &ComputedStyle,
    axis: GridAxis,
    adjustment: &StartwardImplicitTrackAdjustment,
) -> Option<std::ops::Range<usize>> {
    let auto_repeat_count = adjustment.auto_repeat_count?;
    let (tracks, _, _) = grid_template_axis_inputs(style, axis);
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return None;
    };
    let range = auto_fit_track_range(components, auto_repeat_count)?;
    Some(
        range.start.checked_add(adjustment.before_count)?
            ..range.end.checked_add(adjustment.before_count)?,
    )
}

fn auto_fit_track_range(
    components: &[css::GridTrackListComponent],
    auto_repeat_count: u16,
) -> Option<std::ops::Range<usize>> {
    let mut start = 0_usize;
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, _) => start = start.checked_add(1)?,
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let repeat_track_count = repeated_track_component_count(&repeat.tracks)?;
                let repeated_count = match repeat.count {
                    css::GridRepeatCount::Number(count) => count,
                    css::GridRepeatCount::AutoFill => auto_repeat_count,
                    css::GridRepeatCount::AutoFit => {
                        let len = usize::from(auto_repeat_count).checked_mul(repeat_track_count)?;
                        return Some(start..start.checked_add(len)?);
                    }
                };
                start = start.checked_add(usize::from(repeated_count) * repeat_track_count)?;
            }
        }
    }
    None
}

fn repeated_track_component_count(components: &[css::GridTrackListComponent]) -> Option<usize> {
    components.iter().try_fold(0_usize, |count, component| {
        Some(match component {
            css::GridTrackListComponent::Track(_, _) => count.checked_add(1)?,
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repeat_count) = repeat.count else {
                    return None;
                };
                count.checked_add(
                    usize::from(repeat_count) * repeated_track_component_count(&repeat.tracks)?,
                )?
            }
        })
    })
}

/// Count finite explicit grid tracks represented by an authored track list.
///
/// CSS Grid's explicit grid can be enlarged by `grid-template-areas`; this
/// helper tells the Taffy adapter how many missing area-created explicit
/// tracks need sizes from `grid-auto-rows` or `grid-auto-columns`:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids>.
fn grid_template_track_component_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks { components, .. } = tracks else {
        return Some(0);
    };
    let mut count = 0_usize;
    for component in components {
        match component {
            css::GridTrackListComponent::Track(_, _) => count = count.checked_add(1)?,
            css::GridTrackListComponent::Repeat(_, repeat) => {
                let css::GridRepeatCount::Number(repeat_count) = repeat.count else {
                    return None;
                };
                let repeated_tracks = repeat
                    .tracks
                    .iter()
                    .filter(|component| {
                        matches!(component, css::GridTrackListComponent::Track(_, _))
                    })
                    .count();
                count = count.checked_add(usize::from(repeat_count) * repeated_tracks)?;
            }
        }
    }
    Some(count)
}

/// Count the explicit grid-axis tracks created by `grid-template-areas`.
///
/// Grid containers use this to enlarge an authored track list, and Grid Lanes
/// uses the same count to construct its grid-axis topology:
/// <https://drafts.csswg.org/css-grid-2/#explicit-grids> and
/// <https://drafts.csswg.org/css-grid-3/#grid-axis-track-sizing>
pub(super) fn grid_template_area_track_count(
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
) -> usize {
    axis.template_area_track_count(areas)
}

fn grid_template_axis_inputs(
    style: &ComputedStyle,
    axis: GridAxis,
) -> (
    &css::GridTrackList,
    &css::GridTemplateAreas,
    &css::GridAutoTrackList,
) {
    axis.template_inputs(style)
}

fn grid_template_axis_inputs_with_gap(
    style: &ComputedStyle,
    axis: GridAxis,
) -> (
    &css::GridTrackList,
    &css::GridTemplateAreas,
    &css::GridAutoTrackList,
    css::ComputedGap,
) {
    let (tracks, areas, auto_tracks) = grid_template_axis_inputs(style, axis);
    (tracks, areas, auto_tracks, axis.gap(style))
}

pub(super) fn taffy_grid_template_component(
    component: &css::GridTrackListComponent,
    auto_repeat_percentage_basis: GridPercentageBasis,
) -> taffy_layout::GridTemplateComponent<String> {
    match component {
        css::GridTrackListComponent::Track(_, size) => {
            taffy_layout::GridTemplateComponent::Single(taffy_track_size(size))
        }
        css::GridTrackListComponent::Repeat(_, repeat) => {
            taffy_layout::GridTemplateComponent::Repeat(taffy::style::GridTemplateRepetition {
                count: match repeat.count {
                    css::GridRepeatCount::Number(count) => {
                        taffy_layout::RepetitionCount::Count(count)
                    }
                    css::GridRepeatCount::AutoFill => taffy_layout::RepetitionCount::AutoFill,
                    css::GridRepeatCount::AutoFit => taffy_layout::RepetitionCount::AutoFit,
                },
                // Only auto-repeat fragments need a definite track breadth for
                // repetition selection. Numbered repeats retain the existing
                // general track conversion, including its deferred percentage
                // behavior.
                tracks: repeat
                    .tracks
                    .iter()
                    .filter_map(|component| match component {
                        css::GridTrackListComponent::Track(_, size) => Some(match repeat.count {
                            css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => {
                                taffy_auto_repeat_track_size(size, auto_repeat_percentage_basis)
                            }
                            css::GridRepeatCount::Number(_) => taffy_track_size(size),
                        }),
                        css::GridTrackListComponent::Repeat(_, _) => None,
                    })
                    .collect(),
                line_names: taffy_grid_repeat_line_names(repeat),
            })
        }
    }
}

/// Convert a track used by auto-repeat while applying Grid's non-zero floor.
///
/// The auto-repeat count algorithm must floor a zero fixed track breadth to a
/// UA-defined non-zero value to avoid division by zero and an unbounded repeat
/// count. Quire uses one CSS pixel (0.75 PDF points), the value suggested by
/// CSS Grid. The floor only affects repeat-count selection; flexible growth
/// and auto-fit collapse remain Taffy's ordinary track sizing behavior.
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>
fn taffy_auto_repeat_track_size(
    value: &css::GridTrackSize,
    percentage_basis: GridPercentageBasis,
) -> taffy_layout::TrackSizingFunction {
    let mut track = taffy_track_size(value);
    if let Some(basis) = percentage_basis.points() {
        track.min = resolved_auto_repeat_min_track_breadth(value.min.clone(), percentage_basis);
        track.max = resolved_auto_repeat_max_track_breadth(value.max.clone(), percentage_basis);
        if definite_auto_repeat_track_size(value, content_box_pt(basis.max(0.0)))
            .is_some_and(|size| size <= content_box_pt(0.0))
        {
            track.min = taffy_layout::MinTrackSizingFunction::length(0.75);
        }
    } else if definite_auto_repeat_track_size(value, content_box_pt(0.0))
        .is_some_and(|size| size <= content_box_pt(0.0))
    {
        track.min = taffy_layout::MinTrackSizingFunction::length(0.75);
    }
    track
}

/// Resolve the fixed track breadth required by CSS Grid's auto-repeat count
/// against its definite grid-axis content-box basis before entering Taffy.
/// Mixed `calc(<percentage> +/- <length>)` values cannot otherwise be
/// represented by Taffy's scalar percentage-or-length model.
/// <https://drafts.csswg.org/css-grid-2/#auto-repeat>
fn resolved_auto_repeat_min_track_breadth(
    value: css::GridMinTrackBreadth,
    percentage_basis: GridPercentageBasis,
) -> taffy_layout::MinTrackSizingFunction {
    match value {
        css::GridMinTrackBreadth::LengthPercentage(value) => value
            .used_length_with_percentage_basis(percentage_basis)
            .map(|value| taffy_layout::MinTrackSizingFunction::length(value.points().max(0.0)))
            .unwrap_or_else(|| {
                taffy_min_track_breadth(css::GridMinTrackBreadth::LengthPercentage(value))
            }),
        value => taffy_min_track_breadth(value),
    }
}

fn resolved_auto_repeat_max_track_breadth(
    value: css::GridMaxTrackBreadth,
    percentage_basis: GridPercentageBasis,
) -> taffy_layout::MaxTrackSizingFunction {
    match value {
        css::GridMaxTrackBreadth::LengthPercentage(value) => value
            .used_length_with_percentage_basis(percentage_basis)
            .map(|value| taffy_layout::MaxTrackSizingFunction::length(value.points().max(0.0)))
            .unwrap_or_else(|| {
                taffy_max_track_breadth(css::GridMaxTrackBreadth::LengthPercentage(value))
            }),
        value => taffy_max_track_breadth(value),
    }
}

pub(super) fn taffy_grid_repeat_line_names(repeat: &css::GridRepeat) -> Vec<Vec<String>> {
    let mut line_names = Vec::with_capacity(repeat.tracks.len() + 1);
    for component in &repeat.tracks {
        match component {
            css::GridTrackListComponent::Track(names, _)
            | css::GridTrackListComponent::Repeat(names, _) => line_names.push(names.clone()),
        }
    }
    line_names.push(repeat.trailing_names.clone());
    line_names
}

pub(super) fn taffy_grid_template_areas(
    value: &css::GridTemplateAreas,
) -> Option<taffy::style::GridTemplateAreas<String>> {
    let css::GridTemplateAreas::Areas(rows) = value else {
        return None;
    };
    let row_count = u16::try_from(rows.len()).ok()?;
    let column_count = u16::try_from(rows.first()?.cells.len()).ok()?;
    let areas = collect_grid_template_area_bounds(rows)
        .into_iter()
        .filter_map(|area| {
            Some(taffy::style::GridTemplateArea {
                name: area.name.clone(),
                row_start: u16::try_from(area.row_start + 1).ok()?,
                row_end: u16::try_from(area.row_end + 2).ok()?,
                column_start: u16::try_from(area.column_start + 1).ok()?,
                column_end: u16::try_from(area.column_end + 2).ok()?,
            })
        })
        .collect();
    Some(taffy::style::GridTemplateAreas {
        areas,
        row_count,
        column_count,
    })
}

fn collect_grid_template_area_bounds(
    rows: &[css::GridTemplateAreaRow],
) -> Vec<GridTemplateAreaBounds> {
    let mut areas: Vec<GridTemplateAreaBounds> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            let Some(name) = cell else {
                continue;
            };
            if let Some(area) = areas.iter_mut().find(|area| area.name == *name) {
                area.row_start = area.row_start.min(row_index);
                area.row_end = area.row_end.max(row_index);
                area.column_start = area.column_start.min(column_index);
                area.column_end = area.column_end.max(column_index);
            } else {
                areas.push(GridTemplateAreaBounds {
                    name: name.clone(),
                    row_start: row_index,
                    row_end: row_index,
                    column_start: column_index,
                    column_end: column_index,
                });
            }
        }
    }
    areas
        .into_iter()
        .filter(|area| grid_template_area_is_rectangular(rows, area))
        .collect()
}

/// Add `*-start` and `*-end` implicit line names generated by named grid areas.
///
/// CSS Grid named areas create implicitly named lines on both axes, and those
/// line names participate in normal line-placement resolution:
/// <https://www.w3.org/TR/css-grid-1/#implicit-named-lines>.
pub(super) fn add_generated_area_line_names(
    line_names: &mut Vec<Vec<String>>,
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
) {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return;
    };
    for area in collect_grid_template_area_bounds(rows) {
        let (start, end) = axis.area_line_range(&area, 0);
        ensure_grid_line_names_length(line_names, end + 1);
        add_grid_line_name(&mut line_names[start], format!("{}-start", area.name));
        add_grid_line_name(&mut line_names[end], format!("{}-end", area.name));
    }
}

fn ensure_grid_line_names_length(line_names: &mut Vec<Vec<String>>, len: usize) {
    line_names.resize_with(len, Vec::new);
}

fn add_grid_line_name(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridAxis {
    Row,
    Column,
}

impl GridAxis {
    fn template_inputs(
        self,
        style: &ComputedStyle,
    ) -> (
        &css::GridTrackList,
        &css::GridTemplateAreas,
        &css::GridAutoTrackList,
    ) {
        match self {
            Self::Row => (
                &style.grid_template_rows,
                &style.grid_template_areas,
                &style.grid_auto_rows,
            ),
            Self::Column => (
                &style.grid_template_columns,
                &style.grid_template_areas,
                &style.grid_auto_columns,
            ),
        }
    }

    fn gap(self, style: &ComputedStyle) -> css::ComputedGap {
        match self {
            Self::Row => style.row_gap.clone(),
            Self::Column => style.column_gap.clone(),
        }
    }

    fn template_area_track_count(self, areas: &css::GridTemplateAreas) -> usize {
        let css::GridTemplateAreas::Areas(rows) = areas else {
            return 0;
        };
        match self {
            Self::Row => rows.len(),
            Self::Column => rows.iter().map(|row| row.cells.len()).max().unwrap_or(0),
        }
    }

    fn area_line_range(self, area: &GridTemplateAreaBounds, shift: usize) -> (usize, usize) {
        match self {
            Self::Row => (area.row_start + shift, area.row_end + 1 + shift),
            Self::Column => (area.column_start + shift, area.column_end + 1 + shift),
        }
    }
}

#[derive(Debug, Clone)]
struct GridTemplateAreaBounds {
    name: String,
    row_start: usize,
    row_end: usize,
    column_start: usize,
    column_end: usize,
}

fn grid_template_area_is_rectangular(
    rows: &[css::GridTemplateAreaRow],
    area: &GridTemplateAreaBounds,
) -> bool {
    (area.row_start..=area.row_end).all(|row_index| {
        (area.column_start..=area.column_end).all(|column_index| {
            rows.get(row_index)
                .and_then(|row| row.cells.get(column_index))
                .is_some_and(|cell| cell.as_ref() == Some(&area.name))
        })
    })
}

pub(super) fn taffy_grid_auto_tracks(
    value: &css::GridAutoTrackList,
) -> Vec<taffy_layout::TrackSizingFunction> {
    value.iter().map(taffy_track_size).collect()
}

pub(super) fn taffy_track_size(value: &css::GridTrackSize) -> taffy_layout::TrackSizingFunction {
    taffy_layout::TrackSizingFunction {
        min: taffy_min_track_breadth(value.min.clone()),
        max: taffy_max_track_breadth(value.max.clone()),
    }
}

pub(super) fn taffy_min_track_breadth(
    value: css::GridMinTrackBreadth,
) -> taffy_layout::MinTrackSizingFunction {
    match value {
        css::GridMinTrackBreadth::Auto => taffy_layout::MinTrackSizingFunction::auto(),
        css::GridMinTrackBreadth::MinContent => taffy_layout::MinTrackSizingFunction::min_content(),
        css::GridMinTrackBreadth::MaxContent => taffy_layout::MinTrackSizingFunction::max_content(),
        css::GridMinTrackBreadth::LengthPercentage(value) => taffy_length_percentage(value).into(),
    }
}

pub(super) fn taffy_max_track_breadth(
    value: css::GridMaxTrackBreadth,
) -> taffy_layout::MaxTrackSizingFunction {
    match value {
        css::GridMaxTrackBreadth::Auto => taffy_layout::MaxTrackSizingFunction::auto(),
        css::GridMaxTrackBreadth::MinContent => taffy_layout::MaxTrackSizingFunction::min_content(),
        css::GridMaxTrackBreadth::MaxContent => taffy_layout::MaxTrackSizingFunction::max_content(),
        css::GridMaxTrackBreadth::LengthPercentage(value) => taffy_length_percentage(value).into(),
        css::GridMaxTrackBreadth::Flex(value) => taffy_layout::MaxTrackSizingFunction::fr(value),
        css::GridMaxTrackBreadth::FitContent(value) => {
            if let Some(percent) = value
                .pure_percentage_coefficient()
                .filter(|percent| *percent != 0.0)
            {
                taffy_layout::MaxTrackSizingFunction::fit_content_percent(percent)
            } else {
                taffy_layout::MaxTrackSizingFunction::fit_content_px(value.length_points())
            }
        }
    }
}
