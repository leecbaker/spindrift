use super::*;

pub(super) fn positioned_grid_static_probe_child<'a>(child: &GridChild<'a>) -> GridChild<'a> {
    let mut style = child.style.clone();
    style.position = Position::Static;
    if style.display.is_inline_level() {
        style.display = style.display.blockified();
    }
    style.margin = css::Edges::ZERO;
    style.box_values.width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.height = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.min_width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.min_height = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.max_width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.max_height = css::ComputedLengthPercentageOrAuto::ZERO;
    GridChild {
        kind: FormattingContextChildKind::AnonymousContent {
            children: Vec::new(),
        },
        style,
    }
}

pub(super) type GridChild<'a> = FormattingContextChild<'a>;

pub(super) fn grid_child_lists_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<GridChild<'a>>, Vec<GridChild<'a>>) {
    itemize_blockified_children(
        child_boxes,
        ItemizationOptions {
            anonymous_item_tag: "__quire_anonymous_grid_item",
            strip_blockified_inline_text_paint: true,
            establish_independent_formatting_context: true,
        },
    )
}

/// Builds the temporary style used to lay out a grid item's contents.
///
/// CSS Grid makes a grid item establish an independent formatting context:
/// <https://www.w3.org/TR/css-grid-1/#grid-items>. Ordinary `flow` items need
/// `flow-root` layout semantics here so descendant block margins stay inside
/// the grid item instead of collapsing through it:
/// <https://www.w3.org/TR/css-display-3/#valdef-display-flow-root>.
pub(super) fn grid_item_layout_style(style: &ComputedStyle) -> ComputedStyle {
    independent_formatting_context_item_style(style.clone())
}
