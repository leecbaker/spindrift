use super::{Direction, WritingMode};

/// A CSS logical dimension.
///
/// Logical axes are resolved against a box's writing mode before they are
/// projected onto the physical page coordinate system:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicalAxis {
    Inline,
    Block,
}

/// A flow-relative edge of a CSS box.
///
/// CSS Logical Properties uses these edges for logical box, inset, border,
/// and corner-radius longhands:
/// <https://www.w3.org/TR/css-logical-1/#box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicalSide {
    InlineStart,
    InlineEnd,
    BlockStart,
    BlockEnd,
}

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
    pub(crate) const fn axis(self) -> PhysicalAxis {
        match self {
            Self::Top | Self::Bottom => PhysicalAxis::Vertical,
            Self::Right | Self::Left => PhysicalAxis::Horizontal,
        }
    }

    pub(crate) const fn is_start_edge(self) -> bool {
        matches!(self, Self::Top | Self::Left)
    }

    pub(crate) const fn is_end_edge(self) -> bool {
        matches!(self, Self::Right | Self::Bottom)
    }

    pub(crate) const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
        }
    }
}

/// A physical coordinate axis in page space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalAxis {
    Horizontal,
    Vertical,
}

/// The complete logical-to-physical axis mapping for one computed flow.
///
/// This is the sole authority for mapping CSS logical box axes and sides from
/// `writing-mode` and `direction` into physical page geometry. Layout code
/// should carry this value to its physical-geometry boundary instead of
/// independently matching a writing mode:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WritingModeAxes {
    writing_mode: WritingMode,
    direction: Direction,
}

impl WritingModeAxes {
    pub(crate) const fn new(writing_mode: WritingMode, direction: Direction) -> Self {
        Self {
            writing_mode,
            direction,
        }
    }

    pub(crate) const fn writing_mode(self) -> WritingMode {
        self.writing_mode
    }

    pub(crate) const fn direction(self) -> Direction {
        self.direction
    }

    /// Resolve a logical box side into its physical page edge.
    pub(crate) const fn physical_side(self, side: LogicalSide) -> PhysicalSide {
        match side {
            LogicalSide::BlockStart => match self.writing_mode {
                WritingMode::HorizontalTb => PhysicalSide::Top,
                WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalSide::Right,
                WritingMode::VerticalLr | WritingMode::SidewaysLr => PhysicalSide::Left,
            },
            LogicalSide::BlockEnd => match self.writing_mode {
                WritingMode::HorizontalTb => PhysicalSide::Bottom,
                WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalSide::Left,
                WritingMode::VerticalLr | WritingMode::SidewaysLr => PhysicalSide::Right,
            },
            LogicalSide::InlineStart => match (self.writing_mode, self.direction) {
                (WritingMode::HorizontalTb, Direction::Ltr) => PhysicalSide::Left,
                (WritingMode::HorizontalTb, Direction::Rtl) => PhysicalSide::Right,
                (WritingMode::SidewaysLr, Direction::Ltr) => PhysicalSide::Bottom,
                (WritingMode::SidewaysLr, Direction::Rtl) => PhysicalSide::Top,
                (_, Direction::Ltr) => PhysicalSide::Top,
                (_, Direction::Rtl) => PhysicalSide::Bottom,
            },
            LogicalSide::InlineEnd => self.physical_side(LogicalSide::InlineStart).opposite(),
        }
    }

    /// Resolve the physical axis occupied by a logical axis.
    pub(crate) const fn physical_axis(self, axis: LogicalAxis) -> PhysicalAxis {
        let side = match axis {
            LogicalAxis::Inline => LogicalSide::InlineStart,
            LogicalAxis::Block => LogicalSide::BlockStart,
        };
        self.physical_side(side).axis()
    }

    /// Resolve the start side of a logical axis.
    pub(crate) const fn physical_start_side(self, axis: LogicalAxis) -> PhysicalSide {
        let side = match axis {
            LogicalAxis::Inline => LogicalSide::InlineStart,
            LogicalAxis::Block => LogicalSide::BlockStart,
        };
        self.physical_side(side)
    }

    /// Resolve the line-relative left edge of an inline formatting context.
    ///
    /// This mapping is independent of `direction`: `direction` chooses
    /// whether inline content progresses from line-left or line-right, while
    /// the writing mode determines which physical edge represents each
    /// line-relative side.
    /// <https://drafts.csswg.org/css-writing-modes-4/#line-directions>
    pub(crate) const fn line_left_side(self) -> PhysicalSide {
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalSide::Left,
            WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => {
                PhysicalSide::Top
            }
            WritingMode::SidewaysLr => PhysicalSide::Bottom,
        }
    }

    /// Resolve the line-relative right edge of an inline formatting context.
    /// <https://drafts.csswg.org/css-writing-modes-4/#line-directions>
    pub(crate) const fn line_right_side(self) -> PhysicalSide {
        self.line_left_side().opposite()
    }

    /// Resolve the line-relative over edge.
    /// <https://drafts.csswg.org/css-writing-modes-4/#line-directions>
    pub(crate) const fn line_over_side(self) -> PhysicalSide {
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalSide::Top,
            WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => {
                PhysicalSide::Right
            }
            WritingMode::SidewaysLr => PhysicalSide::Left,
        }
    }

    /// Resolve the line-relative under edge.
    /// <https://drafts.csswg.org/css-writing-modes-4/#line-directions>
    pub(crate) const fn line_under_side(self) -> PhysicalSide {
        self.line_over_side().opposite()
    }

    /// Return the logical axis occupying a physical page axis.
    pub(crate) fn logical_axis_for_physical(self, axis: PhysicalAxis) -> LogicalAxis {
        if self.physical_axis(LogicalAxis::Inline) == axis {
            LogicalAxis::Inline
        } else {
            LogicalAxis::Block
        }
    }

    /// Whether positive logical offsets run toward a physical end edge.
    pub(crate) const fn is_reversed(self, axis: LogicalAxis) -> bool {
        self.physical_start_side(axis).is_end_edge()
    }

    /// Whether logical inline/block dimensions project to physical height/width.
    pub(crate) const fn swaps_physical_axes(self) -> bool {
        matches!(
            self.physical_axis(LogicalAxis::Inline),
            PhysicalAxis::Vertical
        )
    }

    /// Project an inline/block pair into physical width/height order.
    pub(crate) fn physical_size<T>(self, inline: T, block: T) -> (T, T) {
        if self.swaps_physical_axes() {
            (block, inline)
        } else {
            (inline, block)
        }
    }
}

/// Returns the physical side corresponding to logical block-start.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(crate) fn block_start_side(writing_mode: WritingMode) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, Direction::Ltr).physical_side(LogicalSide::BlockStart)
}

/// Returns the physical side corresponding to logical block-end.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(crate) fn block_end_side(writing_mode: WritingMode) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, Direction::Ltr).physical_side(LogicalSide::BlockEnd)
}

/// Returns the physical side corresponding to logical inline-start.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-direction>.
pub(crate) fn inline_start_side(writing_mode: WritingMode, direction: Direction) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, direction).physical_side(LogicalSide::InlineStart)
}

/// Returns the physical side corresponding to logical inline-end.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-direction>.
pub(crate) fn inline_end_side(writing_mode: WritingMode, direction: Direction) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, direction).physical_side(LogicalSide::InlineEnd)
}

/// Returns the physical side corresponding to the line-relative over side.
///
/// This is controlled by line orientation, not block flow: `vertical-lr`
/// shares the clockwise line orientation of `vertical-rl`, while
/// `sideways-lr` reverses it.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>.
pub(crate) fn line_over_side(writing_mode: WritingMode) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, Direction::Ltr).line_over_side()
}

/// Returns the physical side corresponding to the line-relative under side.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>.
pub(crate) fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    WritingModeAxes::new(writing_mode, Direction::Ltr).line_under_side()
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
    fn maps_sideways_sides_and_line_orientation() {
        let sides = [
            (
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
                PhysicalSide::Left,
                PhysicalSide::Right,
            ),
            (
                WritingMode::HorizontalTb,
                Direction::Rtl,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
                PhysicalSide::Right,
                PhysicalSide::Left,
            ),
            (
                WritingMode::VerticalRl,
                Direction::Ltr,
                PhysicalSide::Right,
                PhysicalSide::Left,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
            (
                WritingMode::VerticalRl,
                Direction::Rtl,
                PhysicalSide::Right,
                PhysicalSide::Left,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::VerticalLr,
                Direction::Ltr,
                PhysicalSide::Left,
                PhysicalSide::Right,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
            (
                WritingMode::VerticalLr,
                Direction::Rtl,
                PhysicalSide::Left,
                PhysicalSide::Right,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Ltr,
                PhysicalSide::Right,
                PhysicalSide::Left,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
            (
                WritingMode::SidewaysRl,
                Direction::Rtl,
                PhysicalSide::Right,
                PhysicalSide::Left,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Ltr,
                PhysicalSide::Left,
                PhysicalSide::Right,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Rtl,
                PhysicalSide::Left,
                PhysicalSide::Right,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
        ];
        for (writing_mode, direction, block_start, block_end, inline_start, inline_end) in sides {
            let axes = WritingModeAxes::new(writing_mode, direction);
            assert_eq!(
                block_start_side(writing_mode),
                block_start,
                "{writing_mode:?}"
            );
            assert_eq!(block_end_side(writing_mode), block_end, "{writing_mode:?}");
            assert_eq!(
                inline_start_side(writing_mode, direction),
                inline_start,
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                inline_end_side(writing_mode, direction),
                inline_end,
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                axes.physical_axis(LogicalAxis::Inline),
                inline_start.axis(),
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                axes.physical_axis(LogicalAxis::Block),
                block_start.axis(),
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                axes.is_reversed(LogicalAxis::Inline),
                matches!(inline_start, PhysicalSide::Right | PhysicalSide::Bottom),
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                axes.is_reversed(LogicalAxis::Block),
                matches!(block_start, PhysicalSide::Right | PhysicalSide::Bottom),
                "{writing_mode:?} {direction:?}"
            );
        }
        assert_eq!(line_over_side(WritingMode::SidewaysRl), PhysicalSide::Right);
        assert_eq!(line_under_side(WritingMode::SidewaysRl), PhysicalSide::Left);
        assert_eq!(line_over_side(WritingMode::SidewaysLr), PhysicalSide::Left);
        assert_eq!(
            line_under_side(WritingMode::SidewaysLr),
            PhysicalSide::Right
        );
    }

    #[test]
    fn reports_physical_axes() {
        assert_eq!(PhysicalSide::Left.axis(), PhysicalAxis::Horizontal);
        assert_eq!(PhysicalSide::Right.axis(), PhysicalAxis::Horizontal);
        assert_eq!(PhysicalSide::Top.axis(), PhysicalAxis::Vertical);
        assert_eq!(PhysicalSide::Bottom.axis(), PhysicalAxis::Vertical);
    }

    #[test]
    fn canonical_mapping_is_complete_and_orthogonal() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let axes = WritingModeAxes::new(writing_mode, direction);
                let inline_start = axes.physical_side(LogicalSide::InlineStart);
                let block_start = axes.physical_side(LogicalSide::BlockStart);

                assert_eq!(
                    axes.physical_side(LogicalSide::InlineEnd),
                    inline_start.opposite(),
                    "{writing_mode:?} {direction:?}"
                );
                assert_eq!(
                    axes.physical_side(LogicalSide::BlockEnd),
                    block_start.opposite(),
                    "{writing_mode:?} {direction:?}"
                );
                assert_ne!(
                    axes.physical_axis(LogicalAxis::Inline),
                    axes.physical_axis(LogicalAxis::Block),
                    "{writing_mode:?} {direction:?}"
                );
                assert_eq!(
                    axes.swaps_physical_axes(),
                    axes.physical_axis(LogicalAxis::Inline) == PhysicalAxis::Vertical,
                    "{writing_mode:?} {direction:?}"
                );
            }
        }
    }
}
