use super::*;

pub(in crate::layout) fn inline_box_edge_paint_offset(edge: InlineBoxEdgeFragment) -> f32 {
    match edge.logical_edge {
        InlineLogicalEdge::Start => edge.advance - edge.paint_extent,
        InlineLogicalEdge::End => 0.0,
    }
}

pub(in crate::layout) fn inline_box_edge_is_painted_by_adjacent_fragment(
    line: &[MeasuredInlineItem],
    item_index: usize,
    edge_style: &ComputedStyle,
    edge: InlineBoxEdgeFragment,
) -> bool {
    match edge.logical_edge {
        InlineLogicalEdge::Start => line[item_index + 1..]
            .iter()
            .find_map(|item| {
                adjacent_fragment_paints_or_stops_edge(item, edge_style, edge.logical_edge)
            })
            .unwrap_or(false),
        InlineLogicalEdge::End => line[..item_index]
            .iter()
            .rev()
            .find_map(|item| {
                adjacent_fragment_paints_or_stops_edge(item, edge_style, edge.logical_edge)
            })
            .unwrap_or(false),
    }
}

pub(in crate::layout) fn adjacent_fragment_paints_or_stops_edge(
    item: &MeasuredInlineItem,
    edge_style: &ComputedStyle,
    edge: InlineLogicalEdge,
) -> Option<bool> {
    match &item.item {
        InlineLineItem::Fragment(fragment) => {
            // Selected-line UAX #9 controls are virtual continuation syntax,
            // not inline box content. A clone edge remains outside those
            // controls and must inspect the adjoining visible fragment for
            // decoration ownership.
            if fragment.source() == InlineTextSource::BidiControl {
                return None;
            }
            if *fragment.style() != *edge_style {
                return Some(false);
            }
            Some(match edge {
                InlineLogicalEdge::Start => fragment.hanging_edges().blocks_start,
                InlineLogicalEdge::End => fragment.hanging_edges().blocks_end,
            })
        }
        InlineLineItem::Atom(atom) if atom.content().is_box_edge() => None,
        _ => Some(false),
    }
}

pub(in crate::layout) fn apply_inline_box_edge_paint_style(
    style: &mut ComputedStyle,
    edge: InlineBoxEdgeFragment,
) {
    style.margin = css::Edges::ZERO;
    let opposite = opposite_physical_side(edge.physical_side);
    clear_box_edge_side(style, opposite);
}

pub(in crate::layout) fn clear_box_edge_side(style: &mut ComputedStyle, side: PhysicalSide) {
    match side {
        PhysicalSide::Top => {
            style.border_widths.top = 0.0;
            style.border_styles.top = BorderStyle::None;
            style.padding.top = 0.0;
            style.border_radius.top_left = css::CornerRadius::ZERO;
            style.border_radius.top_right = css::CornerRadius::ZERO;
        }
        PhysicalSide::Right => {
            style.border_widths.right = 0.0;
            style.border_styles.right = BorderStyle::None;
            style.padding.right = 0.0;
            style.border_radius.top_right = css::CornerRadius::ZERO;
            style.border_radius.bottom_right = css::CornerRadius::ZERO;
        }
        PhysicalSide::Bottom => {
            style.border_widths.bottom = 0.0;
            style.border_styles.bottom = BorderStyle::None;
            style.padding.bottom = 0.0;
            style.border_radius.bottom_right = css::CornerRadius::ZERO;
            style.border_radius.bottom_left = css::CornerRadius::ZERO;
        }
        PhysicalSide::Left => {
            style.border_widths.left = 0.0;
            style.border_styles.left = BorderStyle::None;
            style.padding.left = 0.0;
            style.border_radius.top_left = css::CornerRadius::ZERO;
            style.border_radius.bottom_left = css::CornerRadius::ZERO;
        }
    }
}

pub(in crate::layout) fn opposite_physical_side(side: PhysicalSide) -> PhysicalSide {
    match side {
        PhysicalSide::Top => PhysicalSide::Bottom,
        PhysicalSide::Right => PhysicalSide::Left,
        PhysicalSide::Bottom => PhysicalSide::Top,
        PhysicalSide::Left => PhysicalSide::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_edge_paint_clears_the_opposite_corner_radii() {
        let corner = css::CornerRadius {
            horizontal: css::CornerRadiusComponent {
                value: css::ComputedLengthPercentage::from_points(5.0),
            },
            vertical: css::CornerRadiusComponent {
                value: css::ComputedLengthPercentage::from_points(5.0),
            },
        };
        let mut style = ComputedStyle::initial();
        style.border_radius = css::BorderRadius {
            top_left: corner.clone(),
            top_right: corner.clone(),
            bottom_right: corner.clone(),
            bottom_left: corner,
        };

        clear_box_edge_side(&mut style, PhysicalSide::Right);

        assert!(style.border_radius.top_right.is_zero());
        assert!(style.border_radius.bottom_right.is_zero());
        assert!(!style.border_radius.top_left.is_zero());
        assert!(!style.border_radius.bottom_left.is_zero());
    }
}
