use super::*;

pub(super) struct FlexLayout {
    pub(super) height: f32,
    pub(super) first_baseline: f32,
    pub(super) items: Vec<FlexItemLayout>,
}

/// Maps CSS flex main/cross axes into Reasyprint's physical layout axes.
///
/// CSS Flexbox defines `row` from the inline axis and `column` from the block
/// axis, while Taffy lays out rows on physical X and columns on physical Y.
/// CSS Writing Modes maps those logical axes to physical axes:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_direction(style: &ComputedStyle) -> FlexDirection {
    match style.writing_mode {
        WritingMode::HorizontalTb => style.flex_direction,
        WritingMode::VerticalRl => match (style.flex_direction, style.direction) {
            (FlexDirection::Row, Direction::Ltr) => FlexDirection::Column,
            (FlexDirection::Row, Direction::Rtl) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Ltr) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Rtl) => FlexDirection::Column,
            (FlexDirection::Column, _) => FlexDirection::RowReverse,
            (FlexDirection::ColumnReverse, _) => FlexDirection::Row,
        },
        WritingMode::VerticalLr => match (style.flex_direction, style.direction) {
            (FlexDirection::Row, Direction::Ltr) => FlexDirection::Column,
            (FlexDirection::Row, Direction::Rtl) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Ltr) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Rtl) => FlexDirection::Column,
            (FlexDirection::Column, _) => FlexDirection::Row,
            (FlexDirection::ColumnReverse, _) => FlexDirection::RowReverse,
        },
    }
}

/// Returns physical row/column gaps for a flex container.
///
/// CSS Box Alignment maps `row-gap` to the block axis and `column-gap` to the
/// inline axis. Taffy expects physical X/Y gap values, so vertical writing
/// modes swap the physical row and column gap inputs:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_gaps(style: &ComputedStyle) -> (css::ComputedGap, css::ComputedGap) {
    match style.writing_mode {
        WritingMode::HorizontalTb => (style.column_gap, style.row_gap),
        WritingMode::VerticalRl | WritingMode::VerticalLr => (style.row_gap, style.column_gap),
    }
}

/// Returns whether a flex item is collapsed by `visibility: collapse`.
///
/// CSS Flexbox treats collapsed flex items as removed from flex layout while
/// leaving a cross-size strut behind:
/// <https://www.w3.org/TR/css-flexbox-1/#visibility-collapse>.
pub(super) fn flex_item_is_collapsed(style: &ComputedStyle) -> bool {
    style.visibility == Visibility::Collapse
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexAvailableSpace {
    pub(super) width: f32,
    /// Whether `width` is the flex container's definite used inline size.
    ///
    /// CSS Sizing resolves percentage inline sizes only when the containing
    /// block inline size is definite. Flex intrinsic sizing may have an
    /// available width constraint without having a definite container width:
    /// <https://www.w3.org/TR/css-sizing-3/#definite>.
    pub(super) width_is_definite: bool,
    pub(super) height: Option<f32>,
    /// Whether `height` is the flex container's definite used block size.
    ///
    /// CSS Flexbox wraps against the available main size, but CSS Sizing keeps
    /// `max-height` as a constraint on an automatic used height rather than a
    /// definite height:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break> and
    /// <https://www.w3.org/TR/css-sizing-3/#preferred-size-properties>.
    pub(super) height_is_definite: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemEstimate {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) min_width: f32,
    pub(super) min_height: f32,
    pub(super) content_width: f32,
    pub(super) content_height: f32,
    pub(super) first_baseline: Option<f32>,
    pub(super) last_baseline: Option<f32>,
}

impl FlexItemEstimate {
    pub(super) fn fixed(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            min_width: width,
            min_height: height,
            content_width: width,
            content_height: height,
            first_baseline: None,
            last_baseline: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexItemLayout {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

#[derive(Debug, Clone)]
pub(super) struct StyledChild<'a> {
    pub(super) kind: StyledChildKind<'a>,
    pub(super) style: ComputedStyle,
}

/// Source kind for a flex item.
///
/// CSS Flexbox creates flex items from each in-flow child, and wraps each
/// contiguous non-collapsible text run in an anonymous flex item:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items>.
#[derive(Debug, Clone)]
pub(super) enum StyledChildKind<'a> {
    Element {
        element: &'a Element,
        signature: ElementSignature,
        children: Option<&'a [box_tree::FormattingBox<'a>]>,
    },
    AnonymousText {
        text: String,
    },
}

impl<'a> StyledChild<'a> {
    pub(super) fn element_parts(
        &self,
    ) -> Option<(
        &'a Element,
        &ElementSignature,
        Option<&'a [box_tree::FormattingBox<'a>]>,
    )> {
        match &self.kind {
            StyledChildKind::Element {
                element,
                signature,
                children,
            } => Some((*element, signature, *children)),
            StyledChildKind::AnonymousText { .. } => None,
        }
    }

    pub(super) fn anonymous_text(&self) -> Option<&str> {
        match &self.kind {
            StyledChildKind::AnonymousText { text } => Some(text),
            StyledChildKind::Element { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_vertical_writing_flex_axes_to_physical_axes() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.direction = Direction::Ltr;
        style.flex_direction = FlexDirection::Column;
        assert_eq!(physical_flex_direction(&style), FlexDirection::RowReverse);

        style.flex_direction = FlexDirection::ColumnReverse;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Row);

        style.flex_direction = FlexDirection::Row;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Column);

        style.direction = Direction::Rtl;
        assert_eq!(
            physical_flex_direction(&style),
            FlexDirection::ColumnReverse
        );

        let (physical_x_gap, physical_y_gap) = physical_flex_gaps(&style);
        assert_eq!(physical_x_gap, style.row_gap);
        assert_eq!(physical_y_gap, style.column_gap);
    }
}
