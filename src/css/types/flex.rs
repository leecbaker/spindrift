#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    /// Returns whether the flex container's main axis is the inline/row axis.
    ///
    /// CSS Flexbox defines `row` and `row-reverse` as opposite directions on
    /// the same main axis:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
    pub(crate) fn is_row_axis(self) -> bool {
        matches!(self, Self::Row | Self::RowReverse)
    }

    /// Returns whether the flex container's main axis is the block/column axis.
    ///
    /// CSS Flexbox defines `column` and `column-reverse` as opposite directions
    /// on the same main axis:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
    pub(crate) fn is_column_axis(self) -> bool {
        matches!(self, Self::Column | Self::ColumnReverse)
    }

    /// Returns whether two flex-direction values share the same physical axis.
    ///
    /// CSS Flexbox reverses item order for `*-reverse` values without changing
    /// which physical size is the main size:
    /// <https://www.w3.org/TR/css-flexbox-1/#flow-order>.
    pub(crate) fn shares_axis_with(self, other: Self) -> bool {
        (self.is_row_axis() && other.is_row_axis())
            || (self.is_column_axis() && other.is_column_axis())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
    /// CSS Flexbox Level 2 balanced wrapping.
    ///
    /// `balance` is a wrapping mode, rather than an alignment distribution:
    /// <https://drafts.csswg.org/css-flexbox-2/#flex-wrap-property>.
    Balance,
    /// Balanced wrapping with cross-axis reversal.
    BalanceReverse,
}

impl FlexWrap {
    pub(crate) const fn wraps(self) -> bool {
        !matches!(self, Self::NoWrap)
    }

    pub(crate) const fn reverses_cross_axis(self) -> bool {
        matches!(self, Self::WrapReverse | Self::BalanceReverse)
    }

    pub(crate) const fn balances_lines(self) -> bool {
        matches!(self, Self::Balance | Self::BalanceReverse)
    }
}
