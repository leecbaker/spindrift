#![allow(dead_code)]

use super::*;

/// Maps CSS Grid logical placement into physical grid-container coordinates.
///
/// CSS Grid places items by logical grid lines, then CSS Writing Modes maps
/// the inline and block axes into physical directions. Quire does not yet have
/// a CSS Grid layout algorithm, but this is the intended single boundary for
/// `writing-mode` and `direction` once grid layout is implemented:
/// <https://www.w3.org/TR/css-grid-2/#grid-model> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridAxes {
    pub(super) flow: FlowAxes,
}

impl GridAxes {
    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            flow: FlowAxes::for_style(style),
        }
    }
}

/// A resolved CSS grid line index.
///
/// CSS Grid addresses tracks and item placement with integer grid lines. This
/// is a logical grid-line coordinate, not a physical `x` or `y` value:
/// <https://www.w3.org/TR/css-grid-2/#grid-lines>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridLine {
    pub(super) index: i32,
}

impl GridLine {
    pub(super) const fn new(index: i32) -> Self {
        Self { index }
    }
}

/// A logical CSS grid area bounded by row and column grid lines.
///
/// The line indices are in CSS grid placement space. Physical item rectangles
/// must be projected through track sizing and [`GridAxes`], then into
/// [`GridContainerPlacement`] for page paint:
/// <https://www.w3.org/TR/css-grid-2/#grid-placement>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridArea {
    pub(super) row_start: GridLine,
    pub(super) row_end: GridLine,
    pub(super) column_start: GridLine,
    pub(super) column_end: GridLine,
}

impl GridArea {
    pub(super) const fn new(
        row_start: GridLine,
        row_end: GridLine,
        column_start: GridLine,
        column_end: GridLine,
    ) -> Self {
        Self {
            row_start,
            row_end,
            column_start,
            column_end,
        }
    }
}

/// Physical bounds of one resolved grid track span.
///
/// `start` is an offset in [`GridSpace`] from the grid container's physical
/// top-left origin; `size` is the resolved track-span size after CSS Grid track
/// sizing:
/// <https://www.w3.org/TR/css-grid-2/#track-sizing>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GridTrackBounds {
    pub(super) start: f32,
    pub(super) size: f32,
}

impl GridTrackBounds {
    pub(super) fn new(start: f32, size: f32) -> Self {
        Self {
            start,
            size: size.max(0.0),
        }
    }
}

/// A grid item border box in physical grid-container coordinates.
///
/// The origin is the grid container's physical top-left corner, `x` increases
/// rightward, and `y` increases downward. This is not page paint space; callers
/// must project through [`GridContainerPlacement`] before creating paint or PDF
/// geometry:
/// <https://www.w3.org/TR/css-grid-2/#grid-items>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GridItemBorderBox {
    rect: GridRect,
}

impl GridItemBorderBox {
    pub(super) fn from_tracks(inline: GridTrackBounds, block: GridTrackBounds) -> Self {
        Self {
            rect: GridRect::new(
                GridPoint::new(inline.start, block.start),
                GridSize::new(inline.size, block.size),
            ),
        }
    }

    pub(super) fn rect(self) -> GridRect {
        self.rect
    }
}

/// Places physical grid-container coordinates onto the current page.
///
/// Grid layout should keep item geometry in [`GridSpace`] until this boundary.
/// The projection converts grid-local top-left/downward coordinates into
/// Quire's page top-edge rectangle, which then feeds paint and PDF output:
/// <https://www.w3.org/TR/css-grid-2/#layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GridContainerPlacement {
    /// Physical top-left origin of the grid container in page-top coordinates.
    ///
    /// CSS Grid container-local coordinates increase downward in the block
    /// direction after track sizing, while Quire's block/page layout records a
    /// physical top edge before paint projection:
    /// <https://www.w3.org/TR/css-grid-2/#grid-containers>.
    origin: PageTopPoint,
}

impl GridContainerPlacement {
    pub(super) fn new(origin: PageTopPoint) -> Self {
        Self { origin }
    }

    pub(super) fn page_top_rect_for(self, rect: GridRect) -> PageTopRect {
        PageTopRect::new(
            self.origin.x() + rect.origin.x,
            self.origin.top_y() - rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
    }

    pub(super) fn paint_clip_for(self, rect: GridRect) -> PaintClip {
        self.page_top_rect_for(rect).paint_clip()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_item_border_box_projects_to_page_top_rect() {
        let item = GridItemBorderBox::from_tracks(
            GridTrackBounds::new(15.0, 40.0),
            GridTrackBounds::new(25.0, 30.0),
        );
        let placement = GridContainerPlacement::new(PageTopPoint::new(100.0, 300.0));

        let page_rect = placement.page_top_rect_for(item.rect());
        assert_eq!(page_rect, PageTopRect::new(115.0, 275.0, 40.0, 30.0));
        assert_eq!(
            page_rect.paint_rect(),
            paint_space_rect(115.0, 245.0, 40.0, 30.0)
        );
    }
}
