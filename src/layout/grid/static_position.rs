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
                area: None,
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

        let static_left = context.inner_x + hypothetical.x;
        self.layout_positioned_formatting_context_child(
            child,
            context.stylesheets,
            PositionedChildStaticRect::new(
                static_left,
                static_left + hypothetical.width,
                context.content_top - hypothetical.y,
            ),
            PositionedFormattingChildReplayMode::InlineStaticPosition,
        );
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
                            css::ComputedLengthPercentage::from_points(20.0),
                        ),
                        max: css::GridMaxTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_points(20.0),
                        ),
                    },
                ),
                css::GridTrackListComponent::Track(
                    Vec::new(),
                    css::GridTrackSize {
                        min: css::GridMinTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_points(20.0),
                        ),
                        max: css::GridMaxTrackBreadth::LengthPercentage(
                            css::ComputedLengthPercentage::from_points(20.0),
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
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0),),
                45.0,
            ),
            Some(25.0)
        );
    }

    #[test]
    fn named_line_static_offset_resolves_explicit_line_occurrences() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
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
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0),),
                45.0,
            ),
            Some(25.0)
        );
    }

    #[test]
    fn negative_numeric_line_static_offset_counts_from_explicit_grid_end() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
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
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0),),
                45.0,
            ),
            Some(45.0)
        );
    }

    #[test]
    fn negative_named_line_static_offset_counts_from_explicit_grid_end() {
        let fixed_track = css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(20.0),
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
                css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0),),
                45.0,
            ),
            Some(45.0)
        );
    }
}
