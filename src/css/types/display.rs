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
    /// CSS Grid Level 3's one-dimensional masonry/grid-lanes formatting
    /// context.
    /// <https://drafts.csswg.org/css-grid-3/#establishing-grid-lanes-layout>
    GridLanes,
    /// CSS Ruby's inline formatting context with attached annotation levels.
    ///
    /// Ruby is an inner display type: `inline ruby` remains non-atomic,
    /// whereas `block ruby` produces an outer block wrapper around the ruby
    /// container. <https://drafts.csswg.org/css-ruby-1/#ruby-formatting-context>
    Ruby,
    Table,
    TableCaption,
    TableColumnGroup,
    TableColumn,
    TableHeaderGroup,
    TableRowGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    /// Layout-internal ruby roles. These have no independently meaningful
    /// outer display role and are blockified to ordinary block flow.
    /// <https://drafts.csswg.org/css-display-3/#layout-internal>
    RubyBase,
    RubyText,
    RubyBaseContainer,
    RubyTextContainer,
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
    pub const GRID_LANES: Self = Self {
        outer: DisplayOuter::Block,
        inner: DisplayInner::GridLanes,
        list_item: false,
    };
    pub const INLINE_GRID_LANES: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::GridLanes,
        list_item: false,
    };
    pub const RUBY: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::Ruby,
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
    pub const RUBY_BASE: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::RubyBase,
        list_item: false,
    };
    pub const RUBY_TEXT: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::RubyText,
        list_item: false,
    };
    pub const RUBY_BASE_CONTAINER: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::RubyBaseContainer,
        list_item: false,
    };
    pub const RUBY_TEXT_CONTAINER: Self = Self {
        outer: DisplayOuter::Inline,
        inner: DisplayInner::RubyTextContainer,
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
        matches!(self.inner, DisplayInner::Grid | DisplayInner::GridLanes)
    }

    pub fn is_grid_lanes(self) -> bool {
        self.inner == DisplayInner::GridLanes
    }

    pub fn is_table(self) -> bool {
        self.inner == DisplayInner::Table
    }

    pub fn is_ruby(self) -> bool {
        self.inner == DisplayInner::Ruby
    }

    pub fn is_ruby_internal(self) -> bool {
        matches!(
            self.inner,
            DisplayInner::RubyBase
                | DisplayInner::RubyText
                | DisplayInner::RubyBaseContainer
                | DisplayInner::RubyTextContainer
        )
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

    /// Whether this is a CSS table-internal display type rather than an
    /// independently participating block-level box.
    ///
    /// Table-internal boxes use a block outer display for legacy computed
    /// display serialization, but their placement is defined by the table
    /// formatting context rather than ordinary block-flow boundaries.
    /// <https://drafts.csswg.org/css-display-3/#layout-internal>
    pub fn is_table_internal(self) -> bool {
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

    pub fn is_replaced(self) -> bool {
        self.inner == DisplayInner::Replaced
    }

    pub fn is_list_item(self) -> bool {
        self.list_item
    }

    // CSS Display 3 defines inline-block/inline-flex as inline-level boxes that
    // participate atomically in the parent inline formatting context.
    pub fn is_atomic_inline(self) -> bool {
        // CSS Ruby explicitly makes inline ruby containers non-atomic: their
        // bases take part in the surrounding inline formatting context and
        // may fragment across lines with their annotations. The other
        // non-flow inline display types are atomic formatting contexts.
        // <https://drafts.csswg.org/css-ruby-1/#ruby-formatting-context>
        self.is_inline_level() && !self.is_flow() && !self.is_ruby() && !self.is_ruby_internal()
    }

    // CSS Display: flow-root, flex, grid, table, and replaced boxes establish
    // independent formatting contexts instead of joining parent flow.
    pub fn establishes_block_formatting_context(self) -> bool {
        matches!(
            self.inner,
            DisplayInner::FlowRoot
                | DisplayInner::Flex
                | DisplayInner::Grid
                | DisplayInner::GridLanes
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
        if self.is_none() || self.is_contents() {
            self
        } else if self.is_layout_internal() {
            // CSS Display 4 blockification converts layout-internal boxes into
            // block flow containers even when their computed outer display is
            // already `block`.
            // https://www.w3.org/TR/css-display-4/#transformations
            Self::BLOCK.with_list_item(self.list_item)
        } else if self.is_block_level() {
            self
        } else if self.inner == DisplayInner::FlowRoot {
            // CSS Display 4 preserves legacy behavior: `inline flow-root` and
            // `run-in flow-root` blockify to `block flow`, not `block flow-root`.
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
        self.is_table_internal()
            || matches!(
                self.inner,
                DisplayInner::RubyBase
                    | DisplayInner::RubyText
                    | DisplayInner::RubyBaseContainer
                    | DisplayInner::RubyTextContainer
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibility {
    Visible,
    Hidden,
    Collapse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blockification_converts_block_outer_table_internal_boxes_to_flow() {
        let table_row = Display {
            outer: DisplayOuter::Block,
            inner: DisplayInner::TableRow,
            list_item: false,
        };
        let table_cell = Display {
            outer: DisplayOuter::Block,
            inner: DisplayInner::TableCell,
            list_item: true,
        };

        assert_eq!(table_row.blockified(), Display::BLOCK);
        assert_eq!(table_cell.blockified(), Display::BLOCK.with_list_item(true));
    }
}
