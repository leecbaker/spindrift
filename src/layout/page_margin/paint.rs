use super::*;

/// Replays page-margin boxes into the page display list in stacking order.
///
/// CSS Paged Media paints generated page-margin boxes using clockwise tree
/// order by default, but each page-margin box establishes a stacking context
/// and honors `z-index` relative to the document canvas/content stack:
/// <https://www.w3.org/TR/css-page-3/#painting>.
pub(super) fn replay_page_margin_box_fragments(
    page: &mut Page,
    mut boxes: Vec<PageMarginPaintedBox>,
) {
    boxes.sort_by_key(|box_| (box_.z_index, box_.order));

    for box_ in boxes {
        let context = PaintStackingContext::new(box_.z_index, box_.fragment, Vec::new())
            .with_effects(box_.effects)
            .with_bounds(box_.bounds)
            .with_source_order(box_.order);
        if box_.z_index < 0 {
            let fragment =
                PaintFragment::from_stacking_context_in_band(PaintBand::PageBackground, context);
            page.append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
        } else {
            let fragment = PaintFragment::from_stacking_context(context);
            page.append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
        }
    }
}

pub(super) fn page_margin_box_paint_order(name: &str) -> usize {
    PAGE_MARGIN_BOX_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(PAGE_MARGIN_BOX_NAMES.len())
}
