use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct GridItemEstimate {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) min_width: f32,
    pub(super) min_height: f32,
    pub(super) content_width: f32,
    pub(super) content_height: f32,
}

impl GridItemEstimate {
    pub(super) fn fixed(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            min_width: width,
            min_height: height,
            content_width: width,
            content_height: height,
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Estimate a grid container's min-content and max-content inline widths.
    ///
    /// CSS Grid defines container intrinsic sizes from track sizing with grid
    /// item intrinsic contributions. This is a first Quire-native entrypoint
    /// for parent sizing and shrink-to-fit paths; it handles fixed and basic
    /// intrinsic explicit column tracks while keeping more complex spanning and
    /// flexible-track cases documented as remaining divergences:
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn estimate_grid_intrinsic_widths(
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
            built_child_boxes = box_tree::build_child_boxes_with_font_metrics(
                element,
                stylesheets,
                style,
                &self.ancestors,
                &mut self.font_system,
            );
            &built_child_boxes
        };
        let (mut children, _) = grid_child_lists_from_boxes(child_boxes);
        self.resolve_grid_children_viewport_lengths(&mut children);
        let mut estimates = Vec::with_capacity(children.len());
        for child in &children {
            let estimate = self.estimate_grid_item_size(child, stylesheets, available_width, None);
            estimates.push(estimate);
        }
        let (min_width, max_width) = grid_track_list_intrinsic_widths(
            &style.grid_template_columns,
            &children,
            &estimates,
            style.column_gap,
            available_width,
        );
        (min_width.max(0.0), max_width.max(min_width).max(0.0))
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Estimate one grid item's content-box intrinsic contribution.
    ///
    /// CSS Grid track sizing depends on grid item min-content and max-content
    /// contributions. This helper deliberately reuses Quire's existing inline,
    /// block, flex, table, and replaced-element estimators so grid content
    /// sizing stays aligned with other formatting contexts:
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(super) fn estimate_grid_item_size(
        &mut self,
        child: &GridChild<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        available_height: Option<f32>,
    ) -> GridItemEstimate {
        let style = &child.style;
        let inline_available = match style.writing_mode {
            WritingMode::HorizontalTb => available_width,
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                available_height.unwrap_or(available_width)
            }
        }
        .max(1.0);
        let inline_basis = match style.writing_mode {
            WritingMode::HorizontalTb => Some(available_width),
            WritingMode::VerticalRl | WritingMode::VerticalLr => available_height,
        };
        let block_basis = available_height.unwrap_or(available_width).max(1.0);

        if let Some(children) = child.anonymous_content() {
            let measurement = self.intrinsic_inline_measurement_for_boxes(
                children,
                style,
                stylesheets,
                inline_available,
            );
            return grid_item_estimate_from_intrinsic(
                style,
                available_width,
                inline_basis,
                block_basis,
                measurement.contribution.min_content,
                measurement.contribution.max_content,
                measurement.height().max(style.line_height),
            );
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return GridItemEstimate::fixed(0.0, 0.0);
        };

        self.with_ancestor_signature(signature.clone(), |layout| {
            if replaced_element_kind(element) == Some(ReplacedElementKind::Image)
                && let Some(image) = used_image(
                    element,
                    style,
                    available_width,
                    layout.base_url,
                    layout.root_url,
                    layout.resource_cache,
                )
            {
                return grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    image.content_width.max(1.0),
                    image.content_width.max(1.0),
                    image.content_height.max(1.0),
                );
            }

            if replaced_element_kind(element) == Some(ReplacedElementKind::Svg)
                && let Some((width, height, _)) = svg_rect(element)
            {
                return grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    width.max(1.0),
                    width.max(1.0),
                    height.max(1.0),
                );
            }

            let inline_measurement = layout.intrinsic_inline_measurement_for_element(
                element,
                style,
                stylesheets,
                child_boxes,
                inline_available,
            );
            let mut min_content = inline_measurement.contribution.min_content;
            let mut max_content = inline_measurement.contribution.max_content;
            if min_content == 0.0 && max_content == 0.0 {
                let (block_min, block_max) = layout.block_intrinsic_content_widths(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_width,
                );
                min_content = block_min;
                max_content = block_max;
            }
            let content_height = layout
                .estimate_element_height(element, style, stylesheets, available_width, child_boxes)
                .map(|height| {
                    let vertical_non_content = style.margin.top
                        + style.margin.bottom
                        + style.padding.top
                        + style.padding.bottom
                        + vertical_border_width(style);
                    (height - vertical_non_content).max(0.0)
                })
                .unwrap_or_else(|| inline_measurement.height().max(style.line_height));

            grid_item_estimate_from_intrinsic(
                style,
                available_width,
                inline_basis,
                block_basis,
                min_content,
                max_content,
                content_height,
            )
        })
    }
}

fn grid_track_list_intrinsic_widths(
    tracks: &css::GridTrackList,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    gap: css::ComputedGap,
    percentage_basis: f32,
) -> (f32, f32) {
    let fallback = grid_item_intrinsic_contribution(estimates.iter().copied());
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        return fallback;
    };
    if components.is_empty() {
        return fallback;
    }
    let Some(expanded) = expanded_grid_tracks(components, trailing_names) else {
        return fallback;
    };
    let track_sizes = &expanded.sizes;
    if track_sizes.is_empty() {
        return fallback;
    }
    let gap_width = definite_grid_gap_size(gap, percentage_basis);
    let contributions = grid_track_intrinsic_contributions(
        track_sizes.len(),
        &expanded.line_names,
        children,
        estimates,
        gap_width,
    );
    let track_widths = track_sizes
        .iter()
        .zip(contributions)
        .map(|(size, (item_min, item_max))| {
            grid_track_intrinsic_width(*size, item_min, item_max, percentage_basis)
        })
        .collect::<Vec<_>>();
    let min_width = track_widths.iter().map(|(min, _)| min).sum::<f32>();
    let max_width = track_widths.iter().map(|(_, max)| max).sum::<f32>();
    let gap_count = track_widths.len().saturating_sub(1);
    let total_gap_width = gap_width * gap_count as f32;
    (min_width + total_gap_width, max_width + total_gap_width)
}

#[derive(Debug, Clone, PartialEq)]
struct ExpandedGridTracks {
    sizes: Vec<css::GridTrackSize>,
    line_names: Vec<Vec<String>>,
}

fn expanded_grid_tracks(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
) -> Option<ExpandedGridTracks> {
    let mut sizes = Vec::new();
    let mut line_names = Vec::new();
    let mut current_line_names = Vec::new();
    collect_expanded_grid_tracks(
        components,
        &mut current_line_names,
        &mut sizes,
        &mut line_names,
    )?;
    current_line_names.extend(trailing_names.iter().cloned());
    line_names.push(current_line_names);
    Some(ExpandedGridTracks { sizes, line_names })
}

fn collect_expanded_grid_tracks(
    components: &[css::GridTrackListComponent],
    current_line_names: &mut Vec<String>,
    sizes: &mut Vec<css::GridTrackSize>,
    line_names: &mut Vec<Vec<String>>,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(names, size) => {
                current_line_names.extend(names.iter().cloned());
                line_names.push(std::mem::take(current_line_names));
                sizes.push(*size);
            }
            css::GridTrackListComponent::Repeat(names, repeat) => {
                let count = intrinsic_grid_repeat_count(repeat.count);
                current_line_names.extend(names.iter().cloned());
                for _ in 0..count {
                    collect_expanded_grid_tracks(
                        &repeat.tracks,
                        current_line_names,
                        sizes,
                        line_names,
                    )?;
                    current_line_names.extend(repeat.trailing_names.iter().cloned());
                }
            }
        }
    }
    Some(())
}

fn grid_track_intrinsic_contributions(
    track_count: usize,
    line_names: &[Vec<String>],
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    gap_width: f32,
) -> Vec<(f32, f32)> {
    let mut contributions = vec![(0.0_f32, 0.0_f32); track_count];
    let mut complex = (0.0_f32, 0.0_f32);
    let allow_simple_auto_placement = children
        .iter()
        .all(|child| grid_child_column_is_auto(&child.style));
    let mut auto_cursor = 0_usize;

    for (child, estimate) in children.iter().zip(estimates) {
        let contribution = (estimate.min_width, estimate.content_width);
        if let Some(range) = simple_grid_child_column_range(
            &child.style,
            track_count,
            line_names,
            allow_simple_auto_placement,
            &mut auto_cursor,
        ) {
            distribute_grid_item_contribution(&mut contributions, range, contribution, gap_width);
        } else {
            complex.0 = complex.0.max(contribution.0);
            complex.1 = complex.1.max(contribution.1);
        }
    }

    if complex.0 > 0.0 || complex.1 > 0.0 {
        for contribution in &mut contributions {
            contribution.0 = contribution.0.max(complex.0);
            contribution.1 = contribution.1.max(complex.1);
        }
    }

    contributions
}

fn grid_item_intrinsic_contribution(
    estimates: impl Iterator<Item = GridItemEstimate>,
) -> (f32, f32) {
    estimates.fold((0.0_f32, 0.0_f32), |(min, max), estimate| {
        (min.max(estimate.min_width), max.max(estimate.content_width))
    })
}

fn grid_child_column_is_auto(style: &ComputedStyle) -> bool {
    matches!(style.grid_column_start, css::GridPlacement::Auto)
        && matches!(style.grid_column_end, css::GridPlacement::Auto)
}

fn simple_grid_child_column_range(
    style: &ComputedStyle,
    track_count: usize,
    line_names: &[Vec<String>],
    allow_auto_placement: bool,
    auto_cursor: &mut usize,
) -> Option<std::ops::Range<usize>> {
    if track_count == 0 {
        return None;
    }
    if grid_child_column_is_auto(style) {
        if !allow_auto_placement {
            return None;
        }
        let index = *auto_cursor % track_count;
        *auto_cursor += 1;
        return Some(index..index + 1);
    }
    let start = simple_grid_line_index(&style.grid_column_start, line_names)?;
    let track_index = usize::try_from(start - 1).ok()?;
    if track_index >= track_count {
        return None;
    }
    let span = simple_grid_child_column_span(&style.grid_column_end, start, line_names)?;
    let end = track_index.checked_add(span)?;
    if end <= track_count {
        Some(track_index..end)
    } else {
        None
    }
}

fn simple_grid_line_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
) -> Option<i32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    if let Some(name) = &line.name {
        named_grid_line_index(line_names, name, line.index.unwrap_or(1))
    } else {
        explicit_grid_line_index(line.index?, line_names)
    }
}

fn explicit_grid_line_index(index: i32, line_names: &[Vec<String>]) -> Option<i32> {
    let line_count = i32::try_from(line_names.len()).ok()?;
    if index > 0 {
        (index <= line_count).then_some(index)
    } else if index < 0 {
        let resolved = line_count + index + 1;
        (1..=line_count).contains(&resolved).then_some(resolved)
    } else {
        None
    }
}

fn named_grid_line_index(line_names: &[Vec<String>], name: &str, occurrence: i32) -> Option<i32> {
    if occurrence == 0 {
        return None;
    }
    let target = occurrence.unsigned_abs();
    let mut matches_seen = 0_u32;
    if occurrence > 0 {
        for (index, names) in line_names.iter().enumerate() {
            if names.iter().any(|line_name| line_name == name) {
                matches_seen += 1;
                if matches_seen == target {
                    return i32::try_from(index + 1).ok();
                }
            }
        }
    } else {
        for (index, names) in line_names.iter().enumerate().rev() {
            if names.iter().any(|line_name| line_name == name) {
                matches_seen += 1;
                if matches_seen == target {
                    return i32::try_from(index + 1).ok();
                }
            }
        }
    }
    None
}

fn simple_grid_child_column_span(
    end: &css::GridPlacement,
    start: i32,
    line_names: &[Vec<String>],
) -> Option<usize> {
    match end {
        css::GridPlacement::Auto => Some(1),
        css::GridPlacement::Span(span) if span.name.is_none() => {
            span.span.map(usize::from).filter(|span| *span > 0)
        }
        css::GridPlacement::Line(_) => {
            let end = simple_grid_line_index(end, line_names)?;
            (end > start)
                .then_some(end - start)
                .and_then(|span| usize::try_from(span).ok())
        }
        css::GridPlacement::Span(_) => None,
    }
}

fn distribute_grid_item_contribution(
    contributions: &mut [(f32, f32)],
    range: std::ops::Range<usize>,
    contribution: (f32, f32),
    gap_width: f32,
) {
    let span = range.len();
    if span == 0 {
        return;
    }
    let crossed_gaps = span.saturating_sub(1) as f32;
    let min = (contribution.0 - gap_width * crossed_gaps).max(0.0) / span as f32;
    let max = (contribution.1 - gap_width * crossed_gaps).max(0.0) / span as f32;
    for track in &mut contributions[range] {
        track.0 = track.0.max(min);
        track.1 = track.1.max(max);
    }
}

/// Resolve repeat count for grid container intrinsic width estimates.
///
/// CSS Grid's auto-repeat expansion uses the available definite container size
/// when there is one; otherwise `auto-fill`/`auto-fit` repeat once. Container
/// intrinsic sizing is an indefinite inline-size query in this estimator, so
/// auto-repeat contributes one copy of its fixed-size repeated track list:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn intrinsic_grid_repeat_count(count: css::GridRepeatCount) -> u16 {
    match count {
        css::GridRepeatCount::Number(count) => count,
        css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => 1,
    }
}

fn grid_track_intrinsic_width(
    size: css::GridTrackSize,
    item_min: f32,
    item_max: f32,
    percentage_basis: f32,
) -> (f32, f32) {
    let min = grid_min_track_intrinsic_width(size.min, item_min, item_max, percentage_basis);
    let max = grid_max_track_intrinsic_width(size.max, item_min, item_max, percentage_basis);
    (min.min(max).max(0.0), max.max(min).max(0.0))
}

fn grid_min_track_intrinsic_width(
    breadth: css::GridMinTrackBreadth,
    item_min: f32,
    item_max: f32,
    percentage_basis: f32,
) -> f32 {
    match breadth {
        css::GridMinTrackBreadth::Auto | css::GridMinTrackBreadth::MinContent => item_min,
        css::GridMinTrackBreadth::MaxContent => item_max,
        css::GridMinTrackBreadth::LengthPercentage(value) => {
            used_length_percentage(value, percentage_basis).max(0.0)
        }
    }
}

fn grid_max_track_intrinsic_width(
    breadth: css::GridMaxTrackBreadth,
    item_min: f32,
    item_max: f32,
    percentage_basis: f32,
) -> f32 {
    match breadth {
        css::GridMaxTrackBreadth::Auto
        | css::GridMaxTrackBreadth::MaxContent
        | css::GridMaxTrackBreadth::Flex(_) => item_max,
        css::GridMaxTrackBreadth::MinContent => item_min,
        css::GridMaxTrackBreadth::LengthPercentage(value)
        | css::GridMaxTrackBreadth::FitContent(value) => {
            used_length_percentage(value, percentage_basis).max(0.0)
        }
    }
}

fn grid_item_estimate_from_intrinsic(
    style: &ComputedStyle,
    available_width: f32,
    inline_basis: Option<f32>,
    block_basis: f32,
    min_content: f32,
    max_content: f32,
    content_height: f32,
) -> GridItemEstimate {
    let max_content = max_content.max(min_content).max(0.0);
    let min_content = min_content.max(0.0);
    let content_height = content_height.max(0.0);
    let content_width =
        used_length_percentage_or_auto_with_optional_basis(style.box_values.width, inline_basis)
            .unwrap_or(max_content);
    let content_height = used_length_percentage_or_auto(style.box_values.height, block_basis)
        .unwrap_or(content_height);
    GridItemEstimate {
        width: constrain_width(style, content_width, available_width),
        height: constrain_height(style, content_height, block_basis),
        min_width: constrain_width(style, min_content, available_width),
        min_height: constrain_height(style, content_height.min(style.line_height), block_basis),
        content_width: max_content,
        content_height,
    }
}

/// Measures a leaf grid item for Taffy's Grid track-sizing algorithm.
///
/// CSS Grid track sizing asks grid items for intrinsic size contributions in
/// the inline and block axes. The measurement result is a content-box size;
/// Taffy applies padding and borders around it:
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
pub(super) fn measure_grid_item(
    known_dimensions: taffy_layout::Size<Option<f32>>,
    available_space: taffy_layout::Size<taffy_layout::AvailableSpace>,
    estimate: Option<&mut GridItemEstimate>,
) -> taffy_layout::Size<f32> {
    let estimate = estimate.copied().unwrap_or(GridItemEstimate {
        width: 0.0,
        height: 0.0,
        min_width: 0.0,
        min_height: 0.0,
        content_width: 0.0,
        content_height: 0.0,
    });
    taffy_layout::Size {
        width: known_dimensions
            .width
            .unwrap_or_else(|| {
                grid_item_measured_size(
                    available_space.width,
                    estimate.width,
                    estimate.min_width,
                    estimate.content_width,
                )
            })
            .max(0.0),
        height: known_dimensions
            .height
            .unwrap_or_else(|| {
                grid_item_measured_size(
                    available_space.height,
                    estimate.height,
                    estimate.min_height,
                    estimate.content_height,
                )
            })
            .max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_track(size: f32) -> css::GridTrackSize {
        css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(size),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(size),
            ),
        }
    }

    #[test]
    fn intrinsic_auto_repeat_expands_once_for_indefinite_queries() {
        let components = [css::GridTrackListComponent::Repeat(
            Vec::new(),
            css::GridRepeat {
                count: css::GridRepeatCount::AutoFill,
                tracks: vec![css::GridTrackListComponent::Track(
                    Vec::new(),
                    fixed_track(20.0),
                )],
                trailing_names: Vec::new(),
            },
        )];

        let expanded = expanded_grid_tracks(&components, &["end".to_string()])
            .expect("auto-repeat should expand for intrinsic sizing");
        assert_eq!(expanded.sizes, vec![fixed_track(20.0)]);
        assert_eq!(
            expanded.line_names,
            vec![Vec::<String>::new(), vec!["end".to_string()]]
        );
    }
}

fn grid_item_measured_size(
    available_space: taffy_layout::AvailableSpace,
    preferred: f32,
    min_content: f32,
    max_content: f32,
) -> f32 {
    match available_space {
        taffy_layout::AvailableSpace::MinContent => min_content,
        taffy_layout::AvailableSpace::MaxContent => max_content,
        taffy_layout::AvailableSpace::Definite(_) => preferred,
    }
}
