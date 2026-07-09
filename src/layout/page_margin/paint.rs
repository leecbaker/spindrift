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
    // Margin boxes are generated after the document page tree has been laid
    // out.  Non-negative boxes occupy the post-document generated-content
    // slot, while their relative ordering remains the page-margin tree order.
    // Give that slot source-order values after any document stacking context;
    // `order` is only sixteen entries, so the subtraction cannot underflow.
    const POST_DOCUMENT_SOURCE_ORDER: usize = usize::MAX - 16;
    boxes.sort_by_key(|box_| (box_.z_index, box_.order));

    for box_ in boxes {
        let context = PaintStackingContext::new(box_.z_index, box_.fragment, Vec::new())
            .with_effects(box_.effects)
            .with_bounds(box_.bounds)
            .with_source_order(POST_DOCUMENT_SOURCE_ORDER + box_.order);
        if box_.z_index < 0 {
            let fragment =
                PaintFragment::from_stacking_context_in_band(PaintBand::PageBackground, context);
            page.prepend_paint_fragment_owned(fragment, PaintTranslation::identity());
        } else {
            let fragment = PaintFragment::from_stacking_context(context);
            page.append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
    }
    // Margin boxes are replayed after normal document layout, so their
    // stacking contexts must be ordered here rather than inheriting the
    // declaration/hash-map insertion order used while collecting them.
    // https://www.w3.org/TR/css-page-3/#painting
    page.sort_paint_tree_stacking_contexts();
}

pub(super) fn page_margin_box_paint_order(name: &str) -> usize {
    // CSS Paged Media's page-margin tree paints in one continuous clockwise
    // walk. This is independent of author declaration order.
    // <https://www.w3.org/TR/css-page-3/#painting>
    const PAINT_ORDER: &[&str] = &[
        "top-left-corner",
        "top-left",
        "top-center",
        "top-right",
        "top-right-corner",
        "right-top",
        "right-middle",
        "right-bottom",
        "bottom-right-corner",
        "bottom-right",
        "bottom-center",
        "bottom-left",
        "bottom-left-corner",
        "left-bottom",
        "left-middle",
        "left-top",
    ];
    PAINT_ORDER
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(PAINT_ORDER.len())
}

#[cfg(test)]
mod tests {
    use super::page_margin_box_paint_order;

    #[test]
    fn page_margin_tree_order_is_clockwise() {
        let names = [
            "top-left-corner",
            "top-left",
            "top-center",
            "top-right",
            "top-right-corner",
            "right-top",
            "right-middle",
            "right-bottom",
            "bottom-right-corner",
            "bottom-right",
            "bottom-center",
            "bottom-left",
            "bottom-left-corner",
            "left-bottom",
            "left-middle",
            "left-top",
        ];

        for (expected, name) in names.into_iter().enumerate() {
            assert_eq!(page_margin_box_paint_order(name), expected);
        }
    }
}
