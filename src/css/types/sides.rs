use super::{Direction, WritingMode};

/// A physical side of a rectangular CSS box.
///
/// CSS Writing Modes maps logical block/inline start and end sides to these
/// physical sides from the box's computed `writing-mode` and `direction`:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl PhysicalSide {
    pub(crate) fn axis(self) -> PhysicalAxis {
        match self {
            Self::Top | Self::Bottom => PhysicalAxis::Vertical,
            Self::Right | Self::Left => PhysicalAxis::Horizontal,
        }
    }

    pub(crate) fn is_start_edge(self) -> bool {
        matches!(self, Self::Top | Self::Left)
    }

    pub(crate) fn is_end_edge(self) -> bool {
        matches!(self, Self::Right | Self::Bottom)
    }
}

/// A physical coordinate axis in page space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalAxis {
    Horizontal,
    Vertical,
}

/// Returns the physical side corresponding to logical block-start.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(crate) fn block_start_side(writing_mode: WritingMode) -> PhysicalSide {
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalSide::Top,
        WritingMode::VerticalRl => PhysicalSide::Right,
        WritingMode::VerticalLr => PhysicalSide::Left,
    }
}

/// Returns the physical side corresponding to logical block-end.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(crate) fn block_end_side(writing_mode: WritingMode) -> PhysicalSide {
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalSide::Bottom,
        WritingMode::VerticalRl => PhysicalSide::Left,
        WritingMode::VerticalLr => PhysicalSide::Right,
    }
}

/// Returns the physical side corresponding to logical inline-start.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-direction>.
pub(crate) fn inline_start_side(writing_mode: WritingMode, direction: Direction) -> PhysicalSide {
    match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => PhysicalSide::Left,
        (WritingMode::HorizontalTb, Direction::Rtl) => PhysicalSide::Right,
        (_, Direction::Ltr) => PhysicalSide::Top,
        (_, Direction::Rtl) => PhysicalSide::Bottom,
    }
}

/// Returns the physical side corresponding to logical inline-end.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-direction>.
pub(crate) fn inline_end_side(writing_mode: WritingMode, direction: Direction) -> PhysicalSide {
    match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => PhysicalSide::Right,
        (WritingMode::HorizontalTb, Direction::Rtl) => PhysicalSide::Left,
        (_, Direction::Ltr) => PhysicalSide::Bottom,
        (_, Direction::Rtl) => PhysicalSide::Top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_horizontal_sides() {
        assert_eq!(
            block_start_side(WritingMode::HorizontalTb),
            PhysicalSide::Top
        );
        assert_eq!(
            block_end_side(WritingMode::HorizontalTb),
            PhysicalSide::Bottom
        );
        assert_eq!(
            inline_start_side(WritingMode::HorizontalTb, Direction::Ltr),
            PhysicalSide::Left
        );
        assert_eq!(
            inline_end_side(WritingMode::HorizontalTb, Direction::Ltr),
            PhysicalSide::Right
        );
        assert_eq!(
            inline_start_side(WritingMode::HorizontalTb, Direction::Rtl),
            PhysicalSide::Right
        );
        assert_eq!(
            inline_end_side(WritingMode::HorizontalTb, Direction::Rtl),
            PhysicalSide::Left
        );
    }

    #[test]
    fn maps_vertical_sides() {
        assert_eq!(
            block_start_side(WritingMode::VerticalRl),
            PhysicalSide::Right
        );
        assert_eq!(block_end_side(WritingMode::VerticalRl), PhysicalSide::Left);
        assert_eq!(
            block_start_side(WritingMode::VerticalLr),
            PhysicalSide::Left
        );
        assert_eq!(block_end_side(WritingMode::VerticalLr), PhysicalSide::Right);
        assert_eq!(
            inline_start_side(WritingMode::VerticalRl, Direction::Ltr),
            PhysicalSide::Top
        );
        assert_eq!(
            inline_end_side(WritingMode::VerticalRl, Direction::Rtl),
            PhysicalSide::Top
        );
    }

    #[test]
    fn reports_physical_axes() {
        assert_eq!(PhysicalSide::Left.axis(), PhysicalAxis::Horizontal);
        assert_eq!(PhysicalSide::Right.axis(), PhysicalAxis::Horizontal);
        assert_eq!(PhysicalSide::Top.axis(), PhysicalAxis::Vertical);
        assert_eq!(PhysicalSide::Bottom.axis(), PhysicalAxis::Vertical);
    }
}
