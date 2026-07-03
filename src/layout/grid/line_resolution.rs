use super::*;

pub(super) fn grid_line_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
) -> Option<i32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    if let Some(name) = &line.name {
        named_grid_line_index(line_names, name, line.index.unwrap_or(1))
    } else {
        explicit_grid_line_index(line.index?, i32::try_from(line_names.len()).ok()?)
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
    placement: &css::GridPlacement,
    gap: css::ComputedGap,
    container_size: f32,
) -> Option<f32> {
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        return None;
    };
    let track_sizes = definite_grid_track_sizes(components, container_size)?;
    let line_names = explicit_grid_line_names(components, trailing_names)?;
    let line_index = grid_line_index(placement, &line_names)?;
    if line_index <= 1 {
        return None;
    }
    let track_count = usize::try_from(line_index - 1).ok()?;
    if track_count > track_sizes.len() {
        return None;
    }
    let gap = definite_grid_gap_size(gap, container_size);
    let crossed_gaps = track_count.min(track_sizes.len().saturating_sub(1));
    Some(track_sizes[..track_count].iter().sum::<f32>() + gap * crossed_gaps as f32)
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
            sizes.push(definite_grid_track_size(*size, container_size)?);
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
        ) if min == max => min.used_length_with_percentage_basis(container_size),
        _ => None,
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
        css::GridPlacement::Line(css::GridLinePlacement {
            name: None,
            index: Some(index),
        })
    }

    fn named(name: &str, occurrence: i32) -> css::GridPlacement {
        css::GridPlacement::Line(css::GridLinePlacement {
            name: Some(name.to_string()),
            index: Some(occurrence),
        })
    }

    #[test]
    fn resolves_positive_and_negative_numeric_lines() {
        let lines = vec![vec![], vec![], vec![], vec![]];
        assert_eq!(grid_line_index(&line(1), &lines), Some(1));
        assert_eq!(grid_line_index(&line(4), &lines), Some(4));
        assert_eq!(grid_line_index(&line(-1), &lines), Some(4));
        assert_eq!(grid_line_index(&line(-4), &lines), Some(1));
        assert_eq!(grid_line_index(&line(0), &lines), None);
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
        assert_eq!(grid_line_index(&named("a", 0), &lines), None);
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
            grid_line_static_offset(&tracks, &line(3), gap, 200.0),
            Some(60.0)
        );
        assert_eq!(
            grid_line_static_offset(&tracks, &named("a", -1), gap, 200.0),
            Some(60.0)
        );
    }
}
