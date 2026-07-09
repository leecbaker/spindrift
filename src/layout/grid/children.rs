use super::*;
use std::ops::{Deref, DerefMut};

/// A grid container style after the Grid used-value boundary.
///
/// Frozen box-tree styles remain authored computed values. Grid sizing and
/// replay instead consume this marker after its one-time effective-zoom
/// conversion, so a used style cannot accidentally become a cascade parent.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>
#[derive(Debug, Clone)]
pub(super) struct GridUsedStyle(ComputedStyle);

impl GridUsedStyle {
    pub(super) fn from_normalized(style: ComputedStyle) -> Self {
        debug_assert!(style.zoom_applied);
        Self(style)
    }

    pub(super) fn as_computed(&self) -> &ComputedStyle {
        &self.0
    }
}

impl Deref for GridUsedStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for GridUsedStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// One normal-flow Grid item with its original child tree and prepared style.
///
/// The source child retains authored computed styles for descendant box-tree
/// construction. The paired style is the only style Grid measurement,
/// placement, and replay may consume.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
/// <https://www.w3.org/TR/css-grid-1/#grid-items>
#[derive(Debug, Clone)]
pub(super) struct GridUsedItem<'a> {
    source: FormattingContextChild<'a>,
    pub(super) style: GridUsedStyle,
}

impl<'a> GridUsedItem<'a> {
    pub(super) fn from_source(source: FormattingContextChild<'a>, style: ComputedStyle) -> Self {
        Self {
            source,
            style: GridUsedStyle::from_normalized(style),
        }
    }
}

impl<'a> Deref for GridUsedItem<'a> {
    type Target = FormattingContextChild<'a>;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

pub(super) type GridSourceChild<'a> = FormattingContextChild<'a>;
pub(super) type GridChild<'a> = GridUsedItem<'a>;

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
    GridUsedItem::from_source(
        FormattingContextChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child.source.style.clone(),
        },
        style.0,
    )
}

pub(super) fn grid_child_lists_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<GridSourceChild<'a>>, Vec<GridSourceChild<'a>>) {
    itemize_blockified_children(
        child_boxes,
        ItemizationOptions {
            anonymous_item_tag: "__quire_anonymous_grid_item",
            strip_blockified_inline_text_paint: true,
            establish_independent_formatting_context: true,
        },
    )
}

impl<'a> LayoutBuilder<'a> {
    /// Return the Grid used-style view for a formatting-context entrypoint.
    ///
    /// Some intrinsic probes receive a style already prepared by their parent
    /// formatting context, while standalone Grid roots start from frozen
    /// computed style. Normalize only the latter so effective zoom remains a
    /// one-time conversion.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(in crate::layout::grid) fn grid_used_style(&self, style: &ComputedStyle) -> GridUsedStyle {
        if style.zoom_applied {
            GridUsedStyle::from_normalized(style.clone())
        } else {
            GridUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style))
        }
    }

    /// Resolve a Grid item's deferred lengths and create its used-style view.
    ///
    /// This boundary is deliberately after itemization: item order and source
    /// ownership remain computed-style concerns, while Grid's sizing and
    /// replay use the effective CSS `zoom` exactly once.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/css-grid-1/#grid-items>
    pub(in crate::layout::grid) fn prepare_grid_children<'box_tree>(
        &mut self,
        children: Vec<GridSourceChild<'box_tree>>,
    ) -> Vec<GridChild<'box_tree>> {
        children
            .into_iter()
            .map(|mut source| {
                self.resolve_style_current_viewport_lengths(&mut source.style);
                let mut style = source.style.clone();
                style.apply_effective_zoom();
                GridUsedItem::from_source(source, style)
            })
            .collect()
    }
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
