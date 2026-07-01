use super::*;

/// Input geometry for an abspos grid child's static-position calculation.
///
/// CSS Grid derives the static position of an absolutely positioned child
/// from the grid container and the child's grid-placement properties:
/// <https://www.w3.org/TR/css-grid-1/#abspos-items>.
pub(super) struct PositionedGridStaticContext<'a> {
    pub(super) container_style: &'a ComputedStyle,
    pub(super) stylesheets: &'a [Stylesheet],
    pub(super) inner_x: f32,
    pub(super) inner_width: f32,
    pub(super) content_top: f32,
    pub(super) definite_content_height: Option<f32>,
}

impl<'a> LayoutBuilder<'a> {
    /// Lay out an absolutely positioned grid child from its grid static position.
    ///
    /// CSS Grid says an absolutely positioned child does not participate in
    /// normal grid layout, but its static-position rectangle is derived from
    /// the grid area it would occupy:
    /// <https://www.w3.org/TR/css-grid-1/#abspos-items>.
    pub(super) fn layout_positioned_grid_child<'grid>(
        &mut self,
        child: &GridChild<'grid>,
        in_flow_children: &[GridChild<'grid>],
        context: PositionedGridStaticContext<'_>,
    ) {
        let hypothetical_child = positioned_grid_static_probe_child(child);
        let mut hypothetical_children = Vec::with_capacity(in_flow_children.len() + 1);
        hypothetical_children.extend_from_slice(in_flow_children);
        hypothetical_children.push(hypothetical_child);
        let mut hypothetical = self
            .compute_grid_layout(
                context.container_style,
                &hypothetical_children,
                context.stylesheets,
                context.inner_width,
                context.definite_content_height,
            )
            .and_then(|layout| layout.items.into_iter().nth(in_flow_children.len()))
            .unwrap_or(GridItemLayout {
                x: 0.0,
                y: 0.0,
                width: context.inner_width,
                height: child.style.line_height,
            });
        if let Some(x) = grid_line_static_offset(
            &context.container_style.grid_template_columns,
            &child.style.grid_column_start,
            context.container_style.column_gap,
            context.inner_width,
        ) {
            hypothetical.x = x;
        }
        if let Some(y) = grid_line_static_offset(
            &context.container_style.grid_template_rows,
            &child.style.grid_row_start,
            context.container_style.row_gap,
            context
                .definite_content_height
                .unwrap_or(context.inner_width),
        ) {
            hypothetical.y = y;
        }

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;

        self.content_left = context.inner_x + hypothetical.x;
        self.content_right = self.content_left + hypothetical.width.max(1.0);
        self.cursor_y = context.content_top - hypothetical.y;

        let mut positioned_style = child.style.clone();
        if positioned_style.display.is_inline_level() {
            positioned_style.display = positioned_style.display.blockified();
        }
        positioned_style.abspos_static_source_was_inline_level = true;
        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.layout_positioned_block_with_inline_static_position(
                child_element,
                &positioned_style,
                context.stylesheets,
                child_boxes,
                None,
                InlineStaticPosition {
                    start_x: self.content_left,
                    baseline_y: self.cursor_y,
                },
            );
            self.ancestors.pop();
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }
}

fn grid_line_static_offset(
    tracks: &css::GridTrackList,
    placement: &css::GridPlacement,
    gap: css::ComputedGap,
    container_size: f32,
) -> Option<f32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        return None;
    };
    let track_sizes = definite_grid_track_sizes(components, container_size)?;
    let explicit_line_count = i32::try_from(track_sizes.len() + 1).ok()?;
    let line_index = if let Some(name) = &line.name {
        named_grid_line_index(components, trailing_names, name, line.index.unwrap_or(1))?
    } else {
        explicit_grid_line_index(line.index?, explicit_line_count)?
    };
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

fn explicit_grid_line_index(index: i32, explicit_line_count: i32) -> Option<i32> {
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

fn named_grid_line_index(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
    name: &str,
    occurrence: i32,
) -> Option<i32> {
    if occurrence == 0 {
        return None;
    }
    let lines = explicit_grid_line_names(components, trailing_names)?;
    let target = occurrence.unsigned_abs();
    let mut matches_seen = 0_u32;
    if occurrence > 0 {
        for (index, names) in lines.iter().enumerate() {
            if names.iter().any(|line_name| line_name == name) {
                matches_seen += 1;
                if matches_seen == target {
                    return i32::try_from(index + 1).ok();
                }
            }
        }
    } else {
        for (index, names) in lines.iter().enumerate().rev() {
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

fn explicit_grid_line_names(
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

    #[test]
    fn numeric_line_static_offset_includes_track_gaps() {
        let tracks = css::GridTrackList::Tracks {
            components: vec![
                css::GridTrackListComponent::Track(
                    Vec::new(),
                    css::GridTrackSize {
                        min: css::GridMinTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_length(20.0),
                        ),
                        max: css::GridMaxTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_length(20.0),
                        ),
                    },
                ),
                css::GridTrackListComponent::Track(
                    Vec::new(),
                    css::GridTrackSize {
                        min: css::GridMinTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_length(20.0),
                        ),
                        max: css::GridMaxTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_length(20.0),
                        ),
                    },
                ),
            ],
            trailing_names: Vec::new(),
        };
        let placement = css::GridPlacement::Line(css::GridLinePlacement {
            name: None,
            index: Some(2),
        });
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &placement,
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_length(5.0),),
                45.0,
            ),
            Some(25.0)
        );
    }

    #[test]
    fn named_line_static_offset_resolves_explicit_line_occurrences() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
        };
        let tracks = css::GridTrackList::Tracks {
            components: vec![
                css::GridTrackListComponent::Track(vec!["main".to_string()], fixed_track),
                css::GridTrackListComponent::Track(vec!["main".to_string()], fixed_track),
            ],
            trailing_names: vec!["main".to_string()],
        };
        let placement = css::GridPlacement::Line(css::GridLinePlacement {
            name: Some("main".to_string()),
            index: Some(2),
        });
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &placement,
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_length(5.0),),
                45.0,
            ),
            Some(25.0)
        );
    }

    #[test]
    fn negative_numeric_line_static_offset_counts_from_explicit_grid_end() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
        };
        let tracks = css::GridTrackList::Tracks {
            components: vec![
                css::GridTrackListComponent::Track(Vec::new(), fixed_track),
                css::GridTrackListComponent::Track(Vec::new(), fixed_track),
            ],
            trailing_names: Vec::new(),
        };
        let placement = css::GridPlacement::Line(css::GridLinePlacement {
            name: None,
            index: Some(-1),
        });
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &placement,
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_length(5.0),),
                45.0,
            ),
            Some(45.0)
        );
    }

    #[test]
    fn negative_named_line_static_offset_counts_from_explicit_grid_end() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_length(20.0),
            ),
        };
        let tracks = css::GridTrackList::Tracks {
            components: vec![
                css::GridTrackListComponent::Track(vec!["main".to_string()], fixed_track),
                css::GridTrackListComponent::Track(vec!["main".to_string()], fixed_track),
            ],
            trailing_names: vec!["main".to_string()],
        };
        let placement = css::GridPlacement::Line(css::GridLinePlacement {
            name: Some("main".to_string()),
            index: Some(-1),
        });
        assert_eq!(
            grid_line_static_offset(
                &tracks,
                &placement,
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_length(5.0),),
                45.0,
            ),
            Some(45.0)
        );
    }
}
