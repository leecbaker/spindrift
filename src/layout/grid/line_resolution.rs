use super::*;

pub(super) fn grid_line_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
) -> Option<i32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    if let Some(name) = line.name() {
        named_grid_line_index(line_names, name, line.index().unwrap_or(1))
    } else {
        explicit_grid_line_index(line.index()?, i32::try_from(line_names.len()).ok()?)
    }
}

pub(super) fn explicit_grid_line_index(index: i32, explicit_line_count: i32) -> Option<i32> {
    if index > 0 {
        (index <= explicit_line_count).then_some(index)
    } else if index < 0 {
        let resolved = explicit_line_count + index + 1;
        (1..=explicit_line_count)
            .contains(&resolved)
            .then_some(resolved)
    } else {
        None
    }
}

pub(super) fn named_grid_line_index(
    line_names: &[Vec<String>],
    name: &str,
    occurrence: i32,
) -> Option<i32> {
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

pub(super) fn explicit_grid_line_names(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
) -> Option<Vec<Vec<String>>> {
    let mut lines = Vec::new();
    let mut current_line_names = Vec::new();
    collect_explicit_grid_line_names(components, &mut current_line_names, &mut lines)?;
    current_line_names.extend(trailing_names.iter().cloned());
    lines.push(current_line_names);
    Some(lines)
}

fn collect_explicit_grid_line_names(
    components: &[css::GridTrackListComponent],
    current_line_names: &mut Vec<String>,
    lines: &mut Vec<Vec<String>>,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(names, _) => {
                current_line_names.extend(names.iter().cloned());
                lines.push(std::mem::take(current_line_names));
            }
            css::GridTrackListComponent::Repeat(names, repeat) => {
                let css::GridRepeatCount::Number(count) = repeat.count else {
                    return None;
                };
                current_line_names.extend(names.iter().cloned());
                for _ in 0..count {
                    collect_explicit_grid_line_names(&repeat.tracks, current_line_names, lines)?;
                    current_line_names.extend(repeat.trailing_names.iter().cloned());
                }
            }
        }
    }
    Some(())
}

pub(super) fn grid_line_static_offset(
    tracks: &css::GridTrackList,
    auto_tracks: &css::GridAutoTrackList,
    placement: &css::GridPlacement,
    gap: css::ComputedGap,
    content_alignment: css::ContentAlignment,
    container_size: f32,
) -> Option<f32> {
    let (components, trailing_names) = match tracks {
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } => (components.as_slice(), trailing_names.as_slice()),
        css::GridTrackList::None | css::GridTrackList::Subgrid { .. } => (&[][..], &[][..]),
    };
    let explicit_track_sizes = definite_grid_track_sizes(components, container_size)?;
    let line_names = explicit_grid_line_names(components, trailing_names)?;
    let line_index =
        grid_line_static_offset_index(placement, &line_names, auto_tracks, container_size)?;
    let gap = definite_grid_gap_size(gap, layout_pt(container_size)).points();
    let line_offsets = grid_static_line_offsets(
        &explicit_track_sizes,
        auto_tracks,
        line_index,
        gap,
        container_size,
    )?;
    content_aligned_grid_line_offset(
        content_alignment,
        container_size,
        &line_offsets.offsets,
        line_offsets.offset_index(line_index)?,
    )
}

pub(super) fn grid_line_static_offset_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
    auto_tracks: &css::GridAutoTrackList,
    container_size: f32,
) -> Option<i32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    if all_auto_tracks_are_definite(auto_tracks, container_size) {
        if line.name().is_none() {
            let index = line.index()?;
            if index == 0 {
                return None;
            }
            if index > 0 {
                return Some(index);
            }
            let explicit_line_count = i32::try_from(line_names.len()).ok()?;
            return explicit_line_count.checked_add(index)?.checked_add(1);
        }
        if let Some(name) = line.name()
            && line.index().unwrap_or(1) > 0
        {
            return positive_named_implicit_grid_line_index(
                line_names,
                name,
                line.index().unwrap_or(1),
            );
        }
        if let Some(name) = line.name()
            && line.index().unwrap_or(1) < 0
        {
            return negative_named_implicit_grid_line_index(
                line_names,
                name,
                line.index().unwrap_or(1),
            );
        }
    }
    grid_line_index(placement, line_names)
}

/// Resolve a positive named line into the after-explicit implicit grid.
///
/// CSS Grid treats implicit lines on the search side as having the requested
/// name when there are not enough explicit named lines to satisfy a positive
/// occurrence:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-slot>.
fn positive_named_implicit_grid_line_index(
    line_names: &[Vec<String>],
    name: &str,
    occurrence: i32,
) -> Option<i32> {
    let target = u32::try_from(occurrence).ok()?;
    if target == 0 {
        return None;
    }
    let mut matches_seen = 0_u32;
    for (index, names) in line_names.iter().enumerate() {
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return i32::try_from(index + 1).ok();
            }
        }
    }
    let missing = target.checked_sub(matches_seen)?;
    let explicit_line_count = i32::try_from(line_names.len()).ok()?;
    explicit_line_count.checked_add(i32::try_from(missing).ok()?)
}

/// Resolve a negative named line into the before-explicit implicit grid.
///
/// CSS Grid treats implicit lines on the search side as having the requested
/// name when there are not enough explicit named lines to satisfy a negative
/// occurrence:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-slot>.
pub(super) fn negative_named_implicit_grid_line_index(
    line_names: &[Vec<String>],
    name: &str,
    occurrence: i32,
) -> Option<i32> {
    let target = occurrence
        .checked_abs()
        .and_then(|value| u32::try_from(value).ok())?;
    if target == 0 {
        return None;
    }
    let mut matches_seen = 0_u32;
    for (index, names) in line_names.iter().enumerate().rev() {
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return i32::try_from(index + 1).ok();
            }
        }
    }
    let missing = target.checked_sub(matches_seen)?;
    1_i32.checked_sub(i32::try_from(missing).ok()?)
}

struct GridStaticLineOffsets {
    first_line_index: i32,
    offsets: Vec<f32>,
}

impl GridStaticLineOffsets {
    fn offset_index(&self, line_index: i32) -> Option<usize> {
        usize::try_from(line_index.checked_sub(self.first_line_index)?).ok()
    }
}

fn grid_static_line_offsets(
    explicit_track_sizes: &[f32],
    auto_tracks: &css::GridAutoTrackList,
    line_index: i32,
    gap: f32,
    container_size: f32,
) -> Option<GridStaticLineOffsets> {
    let explicit_line_count = i32::try_from(explicit_track_sizes.len())
        .ok()?
        .checked_add(1)?;
    let before_track_count = if line_index < 1 {
        usize::try_from(1_i32.checked_sub(line_index)?).ok()?
    } else {
        0
    };
    let after_track_count = if line_index > explicit_line_count {
        usize::try_from(line_index.checked_sub(explicit_line_count)?).ok()?
    } else {
        0
    };

    if before_track_count == 0 && after_track_count == 0 {
        return Some(GridStaticLineOffsets {
            first_line_index: 1,
            offsets: grid_line_offsets_from_track_sizes(explicit_track_sizes, gap),
        });
    }

    let mut track_sizes =
        Vec::with_capacity(before_track_count + explicit_track_sizes.len() + after_track_count);
    track_sizes.extend(
        (0..before_track_count)
            .rev()
            .map(|auto_index| {
                cycled_definite_auto_track_size_before(auto_tracks, auto_index, container_size)
            })
            .collect::<Option<Vec<_>>>()?,
    );
    track_sizes.extend_from_slice(explicit_track_sizes);
    track_sizes.extend(
        (0..after_track_count)
            .map(|auto_index| {
                cycled_definite_auto_track_size_after(auto_tracks, auto_index, container_size)
            })
            .collect::<Option<Vec<_>>>()?,
    );
    Some(GridStaticLineOffsets {
        first_line_index: 1 - i32::try_from(before_track_count).ok()?,
        offsets: grid_line_offsets_from_track_sizes(&track_sizes, gap),
    })
}

/// Build Grid line offsets from already-resolved track sizes and gutters.
///
/// Static-position fallback and ordinary line resolution share this final
/// geometry step; their track-list expansion policies deliberately remain
/// separate.
pub(super) fn grid_line_offsets_from_track_sizes(track_sizes: &[f32], gap: f32) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(track_sizes.len() + 1);
    let mut offset = 0.0;
    offsets.push(offset);
    for (index, size) in track_sizes.iter().enumerate() {
        offset += *size;
        if index + 1 < track_sizes.len() {
            offset += gap;
        }
        offsets.push(offset);
    }
    offsets
}

fn all_auto_tracks_are_definite(auto_tracks: &css::GridAutoTrackList, container_size: f32) -> bool {
    auto_tracks
        .iter()
        .all(|track| definite_grid_track_size(track.clone(), container_size).is_some())
}

pub(in crate::layout::grid) fn cycled_definite_auto_track_size_after(
    auto_tracks: &css::GridAutoTrackList,
    index: usize,
    container_size: f32,
) -> Option<f32> {
    let track = auto_tracks.get(index % auto_tracks.len())?;
    definite_grid_track_size(track.clone(), container_size)
}

pub(in crate::layout::grid) fn cycled_definite_auto_track_size_before(
    auto_tracks: &css::GridAutoTrackList,
    index: usize,
    container_size: f32,
) -> Option<f32> {
    let track_count = auto_tracks.len();
    let track = auto_tracks.get(track_count.checked_sub(1 + index % track_count)?)?;
    definite_grid_track_size(track.clone(), container_size)
}

/// Return the content-aligned offset for a grid line.
///
/// Taffy applies CSS Box Alignment to the final grid tracks, but some Quire
/// static-position paths compute line offsets directly from CSS track sizes.
/// The aligned line offset is derivable from the final track line geometry:
/// positional keywords apply one shared shift, while distributed keywords add
/// edge spacing plus one inserted interval before each later non-collapsed
/// track:
/// <https://www.w3.org/TR/css-align-3/#content-distribution>.
pub(super) fn content_aligned_grid_line_offset(
    content_alignment: css::ContentAlignment,
    container_size: f32,
    line_offsets: &[f32],
    line_index: usize,
) -> Option<f32> {
    let offset = line_offsets.get(line_index).cloned()?;
    let used_size = line_offsets.last().cloned()?;
    let free_space = container_size - used_size;
    if free_space <= 0.0 && content_alignment.safety == css::AlignmentSafety::Safe {
        return Some(offset);
    }
    let alignment_shift = match content_alignment.keyword {
        css::ContentAlignmentKeyword::Normal
        | css::ContentAlignmentKeyword::Start
        | css::ContentAlignmentKeyword::FlexStart
        | css::ContentAlignmentKeyword::Left
        | css::ContentAlignmentKeyword::Stretch
        | css::ContentAlignmentKeyword::Baseline
        | css::ContentAlignmentKeyword::LastBaseline => 0.0,
        css::ContentAlignmentKeyword::End
        | css::ContentAlignmentKeyword::FlexEnd
        | css::ContentAlignmentKeyword::Right => free_space,
        css::ContentAlignmentKeyword::Center => free_space / 2.0,
        css::ContentAlignmentKeyword::SpaceBetween => distributed_content_alignment_shift(
            line_offsets,
            line_index,
            free_space,
            DistributedContentAlignment::Between,
        )?,
        css::ContentAlignmentKeyword::SpaceAround => distributed_content_alignment_shift(
            line_offsets,
            line_index,
            free_space,
            DistributedContentAlignment::Around,
        )?,
        css::ContentAlignmentKeyword::SpaceEvenly => distributed_content_alignment_shift(
            line_offsets,
            line_index,
            free_space,
            DistributedContentAlignment::Evenly,
        )?,
    };
    Some(offset + alignment_shift)
}

enum DistributedContentAlignment {
    Between,
    Around,
    Evenly,
}

fn distributed_content_alignment_shift(
    line_offsets: &[f32],
    line_index: usize,
    free_space: f32,
    alignment: DistributedContentAlignment,
) -> Option<f32> {
    let non_collapsed_track_count = line_offsets
        .windows(2)
        .filter(|window| window[1] > window[0])
        .count();
    if non_collapsed_track_count == 0 {
        return Some(0.0);
    }
    if matches!(alignment, DistributedContentAlignment::Between) && non_collapsed_track_count < 2 {
        return Some(0.0);
    }
    let (interval_divisor, edge_interval_factor) = match alignment {
        DistributedContentAlignment::Between => (non_collapsed_track_count as f32 - 1.0, 0.0),
        DistributedContentAlignment::Around => (non_collapsed_track_count as f32, 0.5),
        DistributedContentAlignment::Evenly => (non_collapsed_track_count as f32 + 1.0, 1.0),
    };
    let interval = free_space / interval_divisor;
    let preceding_non_collapsed_tracks = line_offsets
        .windows(2)
        .take(line_index)
        .filter(|window| window[1] > window[0])
        .count();
    let between_intervals =
        preceding_non_collapsed_tracks.min(non_collapsed_track_count.saturating_sub(1));
    Some(interval * (edge_interval_factor + between_intervals as f32))
}

fn definite_grid_track_sizes(
    components: &[css::GridTrackListComponent],
    container_size: f32,
) -> Option<Vec<f32>> {
    let mut sizes = Vec::new();
    for component in components {
        collect_definite_grid_track_sizes(component, container_size, &mut sizes)?;
    }
    Some(sizes)
}

fn collect_definite_grid_track_sizes(
    component: &css::GridTrackListComponent,
    container_size: f32,
    sizes: &mut Vec<f32>,
) -> Option<()> {
    match component {
        css::GridTrackListComponent::Track(_, size) => {
            sizes.push(definite_grid_track_size(size.clone(), container_size)?);
        }
        css::GridTrackListComponent::Repeat(_, repeat) => {
            let css::GridRepeatCount::Number(count) = repeat.count else {
                return None;
            };
            for _ in 0..count {
                for repeated in &repeat.tracks {
                    collect_definite_grid_track_sizes(repeated, container_size, sizes)?;
                }
            }
        }
    }
    Some(())
}

fn definite_grid_track_size(size: css::GridTrackSize, container_size: f32) -> Option<f32> {
    match (size.min, size.max) {
        (
            css::GridMinTrackBreadth::LengthPercentage(min),
            css::GridMaxTrackBreadth::LengthPercentage(max),
        ) if min == max => min
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(container_size)))
            .map(layout_points),
        _ => None,
    }
}
/// Return used grid-line offsets from Taffy's final track layout.
///
/// CSS Grid absolute static positions are derived from the grid area in the
/// actual grid, including used track sizes, gutters, and collapsed `auto-fit`
/// repeated tracks:
/// <https://www.w3.org/TR/css-grid-1/#abspos-items> and
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
pub(super) fn grid_line_offsets_from_track_layout(sizes: &[f32], gutters: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(sizes.len() + 1);
    let mut offset = 0.0;
    offsets.push(offset);
    for (index, size) in sizes.iter().enumerate() {
        offset += *size;
        if index + 1 < sizes.len() {
            offset += gutters.get(index).cloned().unwrap_or(0.0);
        }
        offsets.push(offset);
    }
    offsets
}

/// Resolve a Grid gap against a definite content-box dimension.
///
/// Track and line-offset algorithms remain scalar coordinate arithmetic; this
/// CSS used-value boundary retains the semantic layout length until a caller
/// enters one of those algorithms.
/// <https://www.w3.org/TR/css-grid-1/#gutters>
pub(super) fn definite_grid_gap_size(
    gap: css::ComputedGap,
    container_size: LayoutLength,
) -> LayoutLength {
    match gap {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(container_size))
            .unwrap_or_else(|| layout_pt(value.length_points())),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_track(points: f32, names: &[&str]) -> css::GridTrackListComponent {
        css::GridTrackListComponent::Track(
            names.iter().map(|name| (*name).to_string()).collect(),
            css::GridTrackSize {
                min: css::GridMinTrackBreadth::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(points),
                ),
                max: css::GridMaxTrackBreadth::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(points),
                ),
            },
        )
    }

    fn line(index: i32) -> css::GridPlacement {
        css::GridPlacement::Line(css::GridLinePlacement::Number(
            std::num::NonZeroI32::new(index).unwrap(),
        ))
    }

    fn named(name: &str, occurrence: i32) -> css::GridPlacement {
        css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: name.to_string(),
            occurrence: std::num::NonZeroI32::new(occurrence),
        })
    }

    fn auto_tracks(points: &[f32]) -> css::GridAutoTrackList {
        css::GridAutoTrackList::from_tracks(
            points
                .iter()
                .map(|points| css::GridTrackSize {
                    min: css::GridMinTrackBreadth::LengthPercentage(
                        css::ComputedLengthPercentage::from_points(*points),
                    ),
                    max: css::GridMaxTrackBreadth::LengthPercentage(
                        css::ComputedLengthPercentage::from_points(*points),
                    ),
                })
                .collect(),
        )
        .expect("test grid auto-track list is non-empty")
    }

    #[test]
    fn resolves_positive_and_negative_numeric_lines() {
        let lines = vec![vec![], vec![], vec![], vec![]];
        assert_eq!(grid_line_index(&line(1), &lines), Some(1));
        assert_eq!(grid_line_index(&line(4), &lines), Some(4));
        assert_eq!(grid_line_index(&line(-1), &lines), Some(4));
        assert_eq!(grid_line_index(&line(-4), &lines), Some(1));
        assert_eq!(grid_line_index(&line(5), &lines), None);
    }

    #[test]
    fn resolves_positive_and_negative_named_lines() {
        let lines = vec![
            vec!["a".to_string()],
            vec!["b".to_string(), "a".to_string()],
            vec!["a".to_string()],
            vec!["b".to_string()],
        ];
        assert_eq!(grid_line_index(&named("a", 1), &lines), Some(1));
        assert_eq!(grid_line_index(&named("a", 2), &lines), Some(2));
        assert_eq!(grid_line_index(&named("a", -1), &lines), Some(3));
        assert_eq!(grid_line_index(&named("b", -1), &lines), Some(4));
    }

    #[test]
    fn static_offset_includes_crossed_tracks_and_gaps() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![
                fixed_track(20.0, &["a"]),
                fixed_track(30.0, &["b"]),
                fixed_track(40.0, &["a"]),
            ],
            trailing_names: vec!["end".to_string()],
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[10.0]),
                &line(3),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                200.0,
            ),
            Some(60.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[10.0]),
                &named("a", -1),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                200.0,
            ),
            Some(60.0)
        );
    }

    #[test]
    fn shared_line_offsets_preserve_track_and_gap_boundaries() {
        assert_eq!(grid_line_offsets_from_track_sizes(&[], 5.0), vec![0.0]);
        assert_eq!(
            grid_line_offsets_from_track_sizes(&[10.0, 20.0, 30.0], 5.0),
            vec![0.0, 15.0, 40.0, 70.0]
        );
    }

    #[test]
    fn static_offset_honors_content_alignment() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![fixed_track(20.0, &[]), fixed_track(20.0, &[])],
            trailing_names: Vec::new(),
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[10.0]),
                &line(2),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                70.0,
            ),
            Some(50.0)
        );
    }

    #[test]
    fn static_offset_uses_positive_numeric_implicit_auto_tracks() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![fixed_track(20.0, &[])],
            trailing_names: Vec::new(),
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &line(4),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                120.0,
            ),
            Some(100.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &line(4),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                120.0,
            ),
            Some(120.0)
        );
    }

    #[test]
    fn static_offset_uses_positive_named_implicit_auto_tracks() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![fixed_track(20.0, &["main"])],
            trailing_names: vec!["main".to_string()],
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &named("main", 4),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                120.0,
            ),
            Some(100.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &named("main", 4),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                120.0,
            ),
            Some(120.0)
        );
    }

    #[test]
    fn static_offset_uses_negative_numeric_implicit_auto_tracks() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![fixed_track(20.0, &[])],
            trailing_names: Vec::new(),
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &line(-3),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                120.0,
            ),
            Some(0.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &line(-3),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                120.0,
            ),
            Some(55.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &line(-4),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                120.0,
            ),
            Some(20.0)
        );
    }

    #[test]
    fn static_offset_uses_negative_named_implicit_auto_tracks() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![fixed_track(20.0, &["main"])],
            trailing_names: vec!["main".to_string()],
        };
        let gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &named("main", -3),
                gap.clone(),
                css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
                120.0,
            ),
            Some(0.0)
        );
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &auto_tracks(&[30.0, 40.0]),
                &named("main", -3),
                gap,
                css::ContentAlignment::new(css::ContentAlignmentKeyword::End),
                120.0,
            ),
            Some(55.0)
        );
    }
}
