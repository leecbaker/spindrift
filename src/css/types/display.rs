#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Display {
    pub outer: DisplayOuter,
    pub inner: DisplayInner,
    pub list_item: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayOuter {
    None,
    Contents,
    Block,
    Inline,
    RunIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayInner {
    Flow,
    FlowRoot,
    Flex,
    Grid,
    Table,
    TableCaption,
    TableColumnGroup,
    TableColumn,
    TableHeaderGroup,
    TableRowGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    Replaced,
}

impl Display {
    pub const NONE: Self = Self {
        outer: DisplayOuter::None,
        inner: DisplayInner::Flow,
        list_item: false,
    };
    pub const CONTENTS: Self = Self {
        outer: DisplayOuter::Contents,
        inner: DisplayInner::Flow,
        list_item: false,
    };
    pub const BLOCK: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::Flow,
        list_item: false,
    };
    pub const INLINE: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Flow,
        list_item: false,
    };
    /// CSS Display `run-in flow`, whose principal box is inline-level until
    /// run-in layout either reparents it into a following block container or
    /// wraps it in an anonymous block fallback:
    /// <https://www.w3.org/TR/css-display-3/#run-in-layout>.
    pub const RUN_IN: Self = Self {
        outer: DisplayOuter::RunIn,
        inner: DisplayInner::Flow,
        list_item: false,
    };
    pub const INLINE_BLOCK: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::FlowRoot,
        list_item: false,
    };
    pub const FLEX: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::Flex,
        list_item: false,
    };
    pub const INLINE_FLEX: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Flex,
        list_item: false,
    };
    pub const GRID: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::Grid,
        list_item: false,
    };
    pub const INLINE_GRID: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Grid,
        list_item: false,
    };
    pub const TABLE: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::Table,
        list_item: false,
    };
    pub const INLINE_TABLE: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Table,
        list_item: false,
    };
    pub const TABLE_CAPTION: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableCaption,
        list_item: false,
    };
    pub const TABLE_COLUMN_GROUP: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableColumnGroup,
        list_item: false,
    };
    pub const TABLE_COLUMN: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableColumn,
        list_item: false,
    };
    pub const TABLE_ROW_GROUP: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableRowGroup,
        list_item: false,
    };
    pub const TABLE_HEADER_GROUP: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableHeaderGroup,
        list_item: false,
    };
    pub const TABLE_FOOTER_GROUP: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableFooterGroup,
        list_item: false,
    };
    pub const TABLE_ROW: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableRow,
        list_item: false,
    };
    pub const TABLE_CELL: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::TableCell,
        list_item: false,
    };
    pub const INLINE_REPLACED: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Replaced,
        list_item: false,
    };
    pub const BLOCK_REPLACED: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::Replaced,
        list_item: false,
    };

    pub const fn new(outer: DisplayOuter, inner: DisplayInner) -> Self {
        Self {
            outer,
            inner,
            list_item: false,
        }
    }

    pub const fn list_item(outer: DisplayOuter, inner: DisplayInner) -> Self {
        Self {
            outer,
            inner,
            list_item: true,
        }
    }

    pub const fn with_list_item(self, list_item: bool) -> Self {
        Self { list_item, ..self }
    }

    pub const fn with_inner(self, inner: DisplayInner) -> Self {
        Self { inner, ..self }
    }

    pub fn is_none(self) -> bool {
        self.outer == DisplayOuter::None
    }

    pub fn is_contents(self) -> bool {
        self.outer == DisplayOuter::Contents
    }

    pub fn is_block_level(self) -> bool {
        self.outer == DisplayOuter::Block
    }

    pub fn is_inline_level(self) -> bool {
        self.outer == DisplayOuter::Inline
    }

    pub fn is_run_in(self) -> bool {
        self.outer == DisplayOuter::RunIn
    }

    pub fn is_inline_or_run_in_level(self) -> bool {
        matches!(self.outer, DisplayOuter::Inline | DisplayOuter::RunIn)
    }

    pub fn is_flow(self) -> bool {
        self.inner == DisplayInner::Flow
    }

    pub fn is_flex(self) -> bool {
        self.inner == DisplayInner::Flex
    }

    pub fn is_grid(self) -> bool {
        self.inner == DisplayInner::Grid
    }

    pub fn is_table(self) -> bool {
        self.inner == DisplayInner::Table
    }

    pub fn is_table_caption(self) -> bool {
        self.inner == DisplayInner::TableCaption
    }

    pub fn is_table_column_group(self) -> bool {
        self.inner == DisplayInner::TableColumnGroup
    }

    pub fn is_table_column(self) -> bool {
        self.inner == DisplayInner::TableColumn
    }

    pub fn is_table_row_group(self) -> bool {
        matches!(
            self.inner,
            DisplayInner::TableHeaderGroup
                | DisplayInner::TableRowGroup
                | DisplayInner::TableFooterGroup
        )
    }

    pub fn is_table_header_group(self) -> bool {
        self.inner == DisplayInner::TableHeaderGroup
    }

    pub fn is_table_footer_group(self) -> bool {
        self.inner == DisplayInner::TableFooterGroup
    }

    pub fn is_table_row(self) -> bool {
        self.inner == DisplayInner::TableRow
    }

    pub fn is_table_cell(self) -> bool {
        self.inner == DisplayInner::TableCell
    }

    pub fn is_replaced(self) -> bool {
        self.inner == DisplayInner::Replaced
    }

    pub fn is_list_item(self) -> bool {
        self.list_item
    }

    // CSS Display 3 defines inline-block/inline-flex as inline-level boxes that
    // participate atomically in the parent inline formatting context.
    pub fn is_atomic_inline(self) -> bool {
        self.is_inline_level() && !self.is_flow()
    }

    // CSS Display: flow-root, flex, grid, table, and replaced boxes establish
    // independent formatting contexts instead of joining parent flow.
    pub fn establishes_block_formatting_context(self) -> bool {
        matches!(
            self.inner,
            DisplayInner::FlowRoot
                | DisplayInner::Flex
                | DisplayInner::Grid
                | DisplayInner::Table
                | DisplayInner::TableCaption
                | DisplayInner::TableColumnGroup
                | DisplayInner::TableColumn
                | DisplayInner::TableHeaderGroup
                | DisplayInner::TableRowGroup
                | DisplayInner::TableFooterGroup
                | DisplayInner::TableRow
                | DisplayInner::TableCell
                | DisplayInner::Replaced
        )
    }

    pub fn blockified(self) -> Self {
        if self.is_none() || self.is_contents() || self.is_block_level() {
            self
        } else if self.inner == DisplayInner::FlowRoot {
            // CSS Display 4 preserves legacy behavior: `inline flow-root` and
            // `run-in flow-root` blockify to `block flow`, not `block flow-root`.
            // https://www.w3.org/TR/css-display-4/#transformations
            Self::BLOCK.with_list_item(self.list_item)
        } else if self.is_layout_internal() {
            // CSS Display 4 blockification converts layout-internal boxes into
            // block flow containers.
            // https://www.w3.org/TR/css-display-4/#transformations
            Self::BLOCK.with_list_item(self.list_item)
        } else {
            Self {
                outer: DisplayOuter::Block,
                inner: self.inner,
                list_item: self.list_item,
            }
        }
    }

    pub fn run_in_inlinified(self) -> Self {
        if self.is_run_in() {
            Self {
                outer: DisplayOuter::Inline,
                inner: self.inner,
                list_item: self.list_item,
            }
        } else {
            self
        }
    }

    fn is_layout_internal(self) -> bool {
        matches!(
            self.inner,
            DisplayInner::TableCaption
                | DisplayInner::TableColumnGroup
                | DisplayInner::TableColumn
                | DisplayInner::TableHeaderGroup
                | DisplayInner::TableRowGroup
                | DisplayInner::TableFooterGroup
                | DisplayInner::TableRow
                | DisplayInner::TableCell
        )
    }
}
