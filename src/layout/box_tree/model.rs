use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageBox<'a> {
    pub children: Vec<FormattingBox<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FormattingBox<'a> {
    Block(BlockBox<'a>),
    Inline(InlineBox<'a>),
    AnonymousBlock(AnonymousBlockBox<'a>),
    AtomicInline(AtomicInlineBox<'a>),
    #[allow(dead_code)]
    Line(LineBox),
    Text(TextBox),
    Table(TableBox<'a>),
    Flex(FlexBox<'a>),
    Replaced(ReplacedBox<'a>),
}

impl<'a> FormattingBox<'a> {
    pub fn element_parts(
        &self,
    ) -> Option<(
        &'a Element,
        &ElementSignature,
        &ComputedStyle,
        &[FormattingBox<'a>],
    )> {
        match self {
            Self::Block(box_) => Some((box_.element, &box_.signature, &box_.style, &box_.children)),
            Self::Inline(box_) => {
                Some((box_.element, &box_.signature, &box_.style, &box_.children))
            }
            Self::AtomicInline(box_) => {
                Some((box_.element, &box_.signature, &box_.style, &box_.children))
            }
            Self::Table(box_) => Some((box_.element, &box_.signature, &box_.style, &box_.children)),
            Self::Flex(box_) => Some((box_.element, &box_.signature, &box_.style, &box_.children)),
            Self::Replaced(box_) => {
                Some((box_.element, &box_.signature, &box_.style, &box_.children))
            }
            Self::AnonymousBlock(_) | Self::Line(_) | Self::Text(_) => None,
        }
    }

    #[cfg(test)]
    pub fn kind(&self) -> FormattingBoxKind {
        match self {
            Self::Block(_) => FormattingBoxKind::Block,
            Self::Inline(_) => FormattingBoxKind::Inline,
            Self::AnonymousBlock(_) => FormattingBoxKind::AnonymousBlock,
            Self::AtomicInline(_) => FormattingBoxKind::AtomicInline,
            Self::Line(_) => FormattingBoxKind::Line,
            Self::Text(_) => FormattingBoxKind::Text,
            Self::Table(_) => FormattingBoxKind::Table,
            Self::Flex(_) => FormattingBoxKind::Flex,
            Self::Replaced(_) => FormattingBoxKind::Replaced,
        }
    }

    #[cfg(test)]
    pub fn style(&self) -> &ComputedStyle {
        match self {
            Self::Block(box_) => &box_.style,
            Self::Inline(box_) => &box_.style,
            Self::AnonymousBlock(box_) => &box_.style,
            Self::AtomicInline(box_) => &box_.style,
            Self::Line(box_) => &box_.style,
            Self::Text(box_) => &box_.style,
            Self::Table(box_) => &box_.style,
            Self::Flex(box_) => &box_.style,
            Self::Replaced(box_) => &box_.style,
        }
    }

    pub(crate) fn children(&self) -> &[FormattingBox<'a>] {
        match self {
            Self::Block(box_) => &box_.children,
            Self::Inline(box_) => &box_.children,
            Self::AnonymousBlock(box_) => &box_.children,
            Self::AtomicInline(box_) => &box_.children,
            Self::Table(box_) => &box_.children,
            Self::Flex(box_) => &box_.children,
            Self::Replaced(box_) => &box_.children,
            Self::Line(_) | Self::Text(_) => &[],
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingBoxKind {
    Block,
    Inline,
    AnonymousBlock,
    AtomicInline,
    Line,
    Text,
    Table,
    Flex,
    Replaced,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub run_in_children: Vec<FormattingBox<'a>>,
    pub children: Vec<FormattingBox<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub fragment_edges: InlineBoxFragmentEdges,
    pub children: Vec<FormattingBox<'a>>,
}

/// Inline-start and inline-end decorations owned by one inline box fragment.
///
/// CSS 2.2 splits an inline box containing in-flow block-level descendants
/// into separate inline fragments around those blocks. The generated fragments
/// share one logical inline box, so only the first fragment owns the
/// inline-start margin/border/padding and only the last fragment owns the
/// inline-end margin/border/padding:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineBoxFragmentEdges {
    pub(crate) owns_start: bool,
    pub(crate) owns_end: bool,
}

impl InlineBoxFragmentEdges {
    pub(crate) const ALL: Self = Self {
        owns_start: true,
        owns_end: true,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnonymousBlockBox<'a> {
    pub style: ComputedStyle,
    pub children: Vec<FormattingBox<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AtomicInlineBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub children: Vec<FormattingBox<'a>>,
    pub table_fragment: Option<TableFragment<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkerBox {
    pub style: ComputedStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineBox {
    pub style: ComputedStyle,
    pub children: Vec<TextBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextBox {
    pub text: String,
    pub style: ComputedStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub children: Vec<FormattingBox<'a>>,
    pub fragment: TableFragment<'a>,
}

/// Durable CSS table formatting fragment built during box-tree construction.
///
/// CSS 2.2 table layout first constructs a table grid with row groups,
/// columns, captions, and row/column-span occupancy before width, height, and
/// border resolution:
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragment<'a> {
    pub rows: Vec<TableFragmentRow<'a>>,
    pub captions: Vec<TableFragmentCaption<'a>>,
    pub columns: Vec<TableFragmentColumn<'a>>,
    pub grid: TableFragmentGrid,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentRow<'a> {
    pub element: Option<&'a Element>,
    pub signature: ElementSignature,
    pub ancestors: Vec<ElementSignature>,
    pub row_groups: Vec<TableFragmentRowGroup<'a>>,
    pub style: Option<ComputedStyle>,
    pub cells: Vec<TableFragmentCell<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentRowGroup<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<ComputedStyle>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentCell<'a> {
    pub element: Option<&'a Element>,
    pub signature: ElementSignature,
    pub children: Vec<FormattingBox<'a>>,
    pub anonymous: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentCaption<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<ComputedStyle>,
    pub children: Vec<FormattingBox<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentColumn<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<ComputedStyle>,
    pub group: Option<TableFragmentColumnGroup<'a>>,
    pub span: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentColumnGroup<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<ComputedStyle>,
    pub span: usize,
}

/// Row/column-span-aware occupancy grid for a table fragment.
///
/// HTML tables define `rowspan=0` as spanning to the end of the row group, and
/// CSS table layout consumes the resulting occupied slots when assigning cell
/// positions:
/// <https://html.spec.whatwg.org/multipage/tables.html#attr-tdth-rowspan>
/// and <https://www.w3.org/TR/CSS22/tables.html#table-layout>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableFragmentGrid {
    pub rows: Vec<Vec<TableFragmentCellPlacement>>,
    pub column_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TableFragmentCellPlacement {
    pub cell: usize,
    pub column: usize,
    pub colspan: usize,
    pub rowspan: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FlexBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub children: Vec<FormattingBox<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplacedBox<'a> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: ComputedStyle,
    pub marker: Option<MarkerBox>,
    pub children: Vec<FormattingBox<'a>>,
}
