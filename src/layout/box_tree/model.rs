use super::*;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageBoxWith<'a, S = MutableStyle> {
    pub children: Vec<FormattingBoxWith<'a, S>>,
    pub counter_events: Vec<CounterEventNode<'a>>,
}

/// A box-generating element or tree-abiding pseudo-element in CSS tree order.
///
/// This sidecar is captured before anonymous-box and table normalization, so
/// counter scope is independent of formatting fragments and pagination replay.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CounterEventNode<'a> {
    pub element: &'a Element,
    pub source: CounterEventSource,
    pub style: ComputedStyle,
    pub children: Vec<CounterEventNode<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CounterEventSource {
    Principal,
    Marker,
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FormattingBoxWith<'a, S = MutableStyle> {
    Block(BlockBoxWith<'a, S>),
    Inline(InlineBoxWith<'a, S>),
    InlineSplitBlockContext(InlineSplitBlockContextBoxWith<'a, S>),
    AnonymousBlock(AnonymousBlockBoxWith<'a, S>),
    AtomicInline(AtomicInlineBoxWith<'a, S>),
    Text(TextBoxWith<S>),
    Table(TableBoxWith<'a, S>),
    Flex(FlexBoxWith<'a, S>),
    Replaced(ReplacedBoxWith<'a, S>),
}

pub(crate) type MutableStyle = Box<ComputedStyle>;
pub(crate) type SharedStyle = Rc<ComputedStyle>;
pub(crate) type MutablePageBox<'a> = PageBoxWith<'a, MutableStyle>;
pub(crate) type MutableFormattingBox<'a> = FormattingBoxWith<'a, MutableStyle>;
pub(crate) type MutableTableFragment<'a> = TableFragmentWith<'a, MutableStyle>;
pub(crate) type MutableBlockBox<'a> = BlockBoxWith<'a, MutableStyle>;
pub(crate) type MutableInlineBox<'a> = InlineBoxWith<'a, MutableStyle>;
pub(crate) type MutableInlineSplitBlockContextBox<'a> =
    InlineSplitBlockContextBoxWith<'a, MutableStyle>;
pub(crate) type MutableAnonymousBlockBox<'a> = AnonymousBlockBoxWith<'a, MutableStyle>;
pub(crate) type MutableAtomicInlineBox<'a> = AtomicInlineBoxWith<'a, MutableStyle>;
pub(crate) type MutableMarkerBox = MarkerBoxWith<MutableStyle>;
pub(crate) type MutableTextBox = TextBoxWith<MutableStyle>;
pub(crate) type MutableTableBox<'a> = TableBoxWith<'a, MutableStyle>;
pub(crate) type MutableFlexBox<'a> = FlexBoxWith<'a, MutableStyle>;
pub(crate) type MutableReplacedBox<'a> = ReplacedBoxWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentRow<'a> = TableFragmentRowWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentRowGroup<'a> = TableFragmentRowGroupWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentCell<'a> = TableFragmentCellWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentCaption<'a> = TableFragmentCaptionWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentColumn<'a> = TableFragmentColumnWith<'a, MutableStyle>;
pub(crate) type MutableTableFragmentColumnGroup<'a> =
    TableFragmentColumnGroupWith<'a, MutableStyle>;
pub(crate) type FrozenPageBox<'a> = PageBoxWith<'a, SharedStyle>;
pub(crate) type FrozenFormattingBox<'a> = FormattingBoxWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragment<'a> = TableFragmentWith<'a, SharedStyle>;
pub(crate) type FrozenBlockBox<'a> = BlockBoxWith<'a, SharedStyle>;
pub(crate) type FrozenInlineBox<'a> = InlineBoxWith<'a, SharedStyle>;
pub(crate) type FrozenInlineSplitBlockContextBox<'a> =
    InlineSplitBlockContextBoxWith<'a, SharedStyle>;
pub(crate) type FrozenAnonymousBlockBox<'a> = AnonymousBlockBoxWith<'a, SharedStyle>;
pub(crate) type FrozenAtomicInlineBox<'a> = AtomicInlineBoxWith<'a, SharedStyle>;
pub(crate) type FrozenMarkerBox = MarkerBoxWith<SharedStyle>;
pub(crate) type FrozenTextBox = TextBoxWith<SharedStyle>;
pub(crate) type FrozenTableBox<'a> = TableBoxWith<'a, SharedStyle>;
pub(crate) type FrozenFlexBox<'a> = FlexBoxWith<'a, SharedStyle>;
pub(crate) type FrozenReplacedBox<'a> = ReplacedBoxWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentRow<'a> = TableFragmentRowWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentRowGroup<'a> = TableFragmentRowGroupWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentCell<'a> = TableFragmentCellWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentCaption<'a> = TableFragmentCaptionWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentColumn<'a> = TableFragmentColumnWith<'a, SharedStyle>;
pub(crate) type FrozenTableFragmentColumnGroup<'a> = TableFragmentColumnGroupWith<'a, SharedStyle>;
pub(crate) type PageBox<'a> = FrozenPageBox<'a>;
pub(crate) type FormattingBox<'a> = FrozenFormattingBox<'a>;
pub(crate) type TableFragment<'a> = FrozenTableFragment<'a>;
pub(crate) type InlineSplitBlockContextBox<'a> = FrozenInlineSplitBlockContextBox<'a>;
pub(crate) type TableFragmentCell<'a> = FrozenTableFragmentCell<'a>;

pub(crate) fn freeze_page_box<'a>(page_box: MutablePageBox<'a>) -> FrozenPageBox<'a> {
    let mut freezer = StyleFreezer::default();
    FrozenPageBox {
        children: freezer.freeze_child_boxes(page_box.children),
        counter_events: page_box.counter_events,
    }
}

pub(crate) fn freeze_child_boxes<'a>(
    children: Vec<MutableFormattingBox<'a>>,
) -> Vec<FrozenFormattingBox<'a>> {
    StyleFreezer::default().freeze_child_boxes(children)
}

pub(crate) fn owned_style(style: &SharedStyle) -> ComputedStyle {
    style.as_ref().clone()
}

#[derive(Default)]
struct StyleFreezer {
    styles: Vec<SharedStyle>,
}

impl StyleFreezer {
    fn freeze_child_boxes<'a>(
        &mut self,
        children: Vec<MutableFormattingBox<'a>>,
    ) -> Vec<FrozenFormattingBox<'a>> {
        children
            .into_iter()
            .map(|box_| self.freeze_box(box_))
            .collect()
    }

    fn freeze_style(&mut self, style: MutableStyle) -> SharedStyle {
        let style = *style;
        if let Some(existing) = self
            .styles
            .iter()
            .find(|existing| existing.as_ref() == &style)
        {
            return Rc::clone(existing);
        }
        let shared = Rc::new(style);
        self.styles.push(Rc::clone(&shared));
        shared
    }

    fn freeze_optional_style(&mut self, style: Option<MutableStyle>) -> Option<SharedStyle> {
        style.map(|style| self.freeze_style(style))
    }

    fn freeze_box<'a>(&mut self, box_: MutableFormattingBox<'a>) -> FrozenFormattingBox<'a> {
        match box_ {
            MutableFormattingBox::Block(box_) => FrozenFormattingBox::Block(FrozenBlockBox {
                element: box_.element,
                signature: box_.signature,
                source: box_.source,
                style: self.freeze_style(box_.style),
                marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                run_in_children: self.freeze_child_boxes(box_.run_in_children),
                children: self.freeze_child_boxes(box_.children),
            }),
            MutableFormattingBox::Inline(box_) => FrozenFormattingBox::Inline(FrozenInlineBox {
                element: box_.element,
                signature: box_.signature,
                source: box_.source,
                style: self.freeze_style(box_.style),
                marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                fragment_edges: box_.fragment_edges,
                children: self.freeze_child_boxes(box_.children),
            }),
            MutableFormattingBox::InlineSplitBlockContext(box_) => {
                FrozenFormattingBox::InlineSplitBlockContext(FrozenInlineSplitBlockContextBox {
                    element: box_.element,
                    signature: box_.signature,
                    source: box_.source,
                    style: self.freeze_style(box_.style),
                    children: self.freeze_child_boxes(box_.children),
                })
            }
            MutableFormattingBox::AnonymousBlock(box_) => {
                FrozenFormattingBox::AnonymousBlock(FrozenAnonymousBlockBox {
                    style: self.freeze_style(box_.style),
                    children: self.freeze_child_boxes(box_.children),
                })
            }
            MutableFormattingBox::AtomicInline(box_) => {
                FrozenFormattingBox::AtomicInline(FrozenAtomicInlineBox {
                    element: box_.element,
                    signature: box_.signature,
                    source: box_.source,
                    style: self.freeze_style(box_.style),
                    marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                    children: self.freeze_child_boxes(box_.children),
                    table_fragment: box_
                        .table_fragment
                        .map(|fragment| self.freeze_table_fragment(fragment)),
                })
            }
            MutableFormattingBox::Text(box_) => {
                FrozenFormattingBox::Text(self.freeze_text_box(box_))
            }
            MutableFormattingBox::Table(box_) => FrozenFormattingBox::Table(FrozenTableBox {
                element: box_.element,
                signature: box_.signature,
                source: box_.source,
                style: self.freeze_style(box_.style),
                marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                children: self.freeze_child_boxes(box_.children),
                fragment: self.freeze_table_fragment(box_.fragment),
            }),
            MutableFormattingBox::Flex(box_) => FrozenFormattingBox::Flex(FrozenFlexBox {
                element: box_.element,
                signature: box_.signature,
                source: box_.source,
                style: self.freeze_style(box_.style),
                marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                children: self.freeze_child_boxes(box_.children),
            }),
            MutableFormattingBox::Replaced(box_) => {
                FrozenFormattingBox::Replaced(FrozenReplacedBox {
                    element: box_.element,
                    signature: box_.signature,
                    source: box_.source,
                    style: self.freeze_style(box_.style),
                    marker: box_.marker.map(|marker| self.freeze_marker(marker)),
                    children: self.freeze_child_boxes(box_.children),
                })
            }
        }
    }

    fn freeze_marker(&mut self, marker: MutableMarkerBox) -> FrozenMarkerBox {
        FrozenMarkerBox {
            style: self.freeze_style(marker.style),
        }
    }

    fn freeze_text_box(&mut self, box_: MutableTextBox) -> FrozenTextBox {
        FrozenTextBox {
            text: box_.text,
            style: self.freeze_style(box_.style),
        }
    }
}

pub(crate) fn freeze_table_fragment<'a>(
    fragment: MutableTableFragment<'a>,
) -> FrozenTableFragment<'a> {
    StyleFreezer::default().freeze_table_fragment(fragment)
}

impl StyleFreezer {
    fn freeze_table_fragment<'a>(
        &mut self,
        fragment: MutableTableFragment<'a>,
    ) -> FrozenTableFragment<'a> {
        FrozenTableFragment {
            rows: fragment
                .rows
                .into_iter()
                .map(|row| self.freeze_table_row(row))
                .collect(),
            captions: fragment
                .captions
                .into_iter()
                .map(|caption| self.freeze_table_caption(caption))
                .collect(),
            columns: fragment
                .columns
                .into_iter()
                .map(|column| self.freeze_table_column(column))
                .collect(),
            grid: fragment.grid,
        }
    }
}

fn thaw_child_boxes<'a>(children: &[FrozenFormattingBox<'a>]) -> Vec<MutableFormattingBox<'a>> {
    children.iter().map(thaw_box).collect()
}

pub(crate) fn clone_frozen_child_boxes_as_mutable<'a>(
    children: &[FrozenFormattingBox<'a>],
) -> Vec<MutableFormattingBox<'a>> {
    thaw_child_boxes(children)
}

fn thaw_style(style: &SharedStyle) -> MutableStyle {
    Box::new(owned_style(style))
}

fn thaw_optional_style(style: &Option<SharedStyle>) -> Option<MutableStyle> {
    style.as_ref().map(thaw_style)
}

fn thaw_box<'a>(box_: &FrozenFormattingBox<'a>) -> MutableFormattingBox<'a> {
    match box_ {
        FrozenFormattingBox::Block(box_) => MutableFormattingBox::Block(MutableBlockBox {
            element: box_.element,
            signature: box_.signature.clone(),
            source: box_.source.clone(),
            style: thaw_style(&box_.style),
            marker: box_.marker.as_ref().map(thaw_marker),
            run_in_children: thaw_child_boxes(&box_.run_in_children),
            children: thaw_child_boxes(&box_.children),
        }),
        FrozenFormattingBox::Inline(box_) => MutableFormattingBox::Inline(MutableInlineBox {
            element: box_.element,
            signature: box_.signature.clone(),
            source: box_.source.clone(),
            style: thaw_style(&box_.style),
            marker: box_.marker.as_ref().map(thaw_marker),
            fragment_edges: box_.fragment_edges,
            children: thaw_child_boxes(&box_.children),
        }),
        FrozenFormattingBox::InlineSplitBlockContext(box_) => {
            MutableFormattingBox::InlineSplitBlockContext(MutableInlineSplitBlockContextBox {
                element: box_.element,
                signature: box_.signature.clone(),
                source: box_.source.clone(),
                style: thaw_style(&box_.style),
                children: thaw_child_boxes(&box_.children),
            })
        }
        FrozenFormattingBox::AnonymousBlock(box_) => {
            MutableFormattingBox::AnonymousBlock(MutableAnonymousBlockBox {
                style: thaw_style(&box_.style),
                children: thaw_child_boxes(&box_.children),
            })
        }
        FrozenFormattingBox::AtomicInline(box_) => {
            MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
                element: box_.element,
                signature: box_.signature.clone(),
                source: box_.source.clone(),
                style: thaw_style(&box_.style),
                marker: box_.marker.as_ref().map(thaw_marker),
                children: thaw_child_boxes(&box_.children),
                table_fragment: box_.table_fragment.as_ref().map(thaw_table_fragment),
            })
        }
        FrozenFormattingBox::Text(box_) => MutableFormattingBox::Text(thaw_text_box(box_)),
        FrozenFormattingBox::Table(box_) => MutableFormattingBox::Table(MutableTableBox {
            element: box_.element,
            signature: box_.signature.clone(),
            source: box_.source.clone(),
            style: thaw_style(&box_.style),
            marker: box_.marker.as_ref().map(thaw_marker),
            children: thaw_child_boxes(&box_.children),
            fragment: thaw_table_fragment(&box_.fragment),
        }),
        FrozenFormattingBox::Flex(box_) => MutableFormattingBox::Flex(MutableFlexBox {
            element: box_.element,
            signature: box_.signature.clone(),
            source: box_.source.clone(),
            style: thaw_style(&box_.style),
            marker: box_.marker.as_ref().map(thaw_marker),
            children: thaw_child_boxes(&box_.children),
        }),
        FrozenFormattingBox::Replaced(box_) => MutableFormattingBox::Replaced(MutableReplacedBox {
            element: box_.element,
            signature: box_.signature.clone(),
            source: box_.source.clone(),
            style: thaw_style(&box_.style),
            marker: box_.marker.as_ref().map(thaw_marker),
            children: thaw_child_boxes(&box_.children),
        }),
    }
}

fn thaw_marker(marker: &FrozenMarkerBox) -> MutableMarkerBox {
    MutableMarkerBox {
        style: thaw_style(&marker.style),
    }
}

fn thaw_text_box(box_: &FrozenTextBox) -> MutableTextBox {
    MutableTextBox {
        text: box_.text.clone(),
        style: thaw_style(&box_.style),
    }
}

fn thaw_table_fragment<'a>(fragment: &FrozenTableFragment<'a>) -> MutableTableFragment<'a> {
    MutableTableFragment {
        rows: fragment.rows.iter().map(thaw_table_row).collect(),
        captions: fragment.captions.iter().map(thaw_table_caption).collect(),
        columns: fragment.columns.iter().map(thaw_table_column).collect(),
        grid: fragment.grid.clone(),
    }
}

fn thaw_table_row<'a>(row: &FrozenTableFragmentRow<'a>) -> MutableTableFragmentRow<'a> {
    MutableTableFragmentRow {
        element: row.element,
        signature: row.signature.clone(),
        ancestors: row.ancestors.clone(),
        row_groups: row.row_groups.iter().map(thaw_table_row_group).collect(),
        style: thaw_optional_style(&row.style),
        cells: row.cells.iter().map(thaw_table_cell).collect(),
    }
}

fn thaw_table_row_group<'a>(
    group: &FrozenTableFragmentRowGroup<'a>,
) -> MutableTableFragmentRowGroup<'a> {
    MutableTableFragmentRowGroup {
        element: group.element,
        signature: group.signature.clone(),
        style: thaw_optional_style(&group.style),
    }
}

fn thaw_table_cell<'a>(cell: &FrozenTableFragmentCell<'a>) -> MutableTableFragmentCell<'a> {
    MutableTableFragmentCell {
        element: cell.element,
        signature: cell.signature.clone(),
        style: thaw_optional_style(&cell.style),
        children: thaw_child_boxes(&cell.children),
        anonymous: cell.anonymous,
    }
}

fn thaw_table_caption<'a>(
    caption: &FrozenTableFragmentCaption<'a>,
) -> MutableTableFragmentCaption<'a> {
    MutableTableFragmentCaption {
        element: caption.element,
        signature: caption.signature.clone(),
        style: thaw_optional_style(&caption.style),
        children: thaw_child_boxes(&caption.children),
    }
}

fn thaw_table_column<'a>(column: &FrozenTableFragmentColumn<'a>) -> MutableTableFragmentColumn<'a> {
    MutableTableFragmentColumn {
        element: column.element,
        signature: column.signature.clone(),
        style: thaw_optional_style(&column.style),
        group: column.group.as_ref().map(thaw_table_column_group),
        span: column.span,
    }
}

fn thaw_table_column_group<'a>(
    group: &FrozenTableFragmentColumnGroup<'a>,
) -> MutableTableFragmentColumnGroup<'a> {
    MutableTableFragmentColumnGroup {
        element: group.element,
        signature: group.signature.clone(),
        style: thaw_optional_style(&group.style),
        span: group.span,
    }
}

impl StyleFreezer {
    fn freeze_table_row<'a>(
        &mut self,
        row: MutableTableFragmentRow<'a>,
    ) -> FrozenTableFragmentRow<'a> {
        FrozenTableFragmentRow {
            element: row.element,
            signature: row.signature,
            ancestors: row.ancestors,
            row_groups: row
                .row_groups
                .into_iter()
                .map(|group| self.freeze_table_row_group(group))
                .collect(),
            style: self.freeze_optional_style(row.style),
            cells: row
                .cells
                .into_iter()
                .map(|cell| self.freeze_table_cell(cell))
                .collect(),
        }
    }

    fn freeze_table_row_group<'a>(
        &mut self,
        group: MutableTableFragmentRowGroup<'a>,
    ) -> FrozenTableFragmentRowGroup<'a> {
        FrozenTableFragmentRowGroup {
            element: group.element,
            signature: group.signature,
            style: self.freeze_optional_style(group.style),
        }
    }

    fn freeze_table_cell<'a>(
        &mut self,
        cell: MutableTableFragmentCell<'a>,
    ) -> FrozenTableFragmentCell<'a> {
        FrozenTableFragmentCell {
            element: cell.element,
            signature: cell.signature,
            style: self.freeze_optional_style(cell.style),
            children: self.freeze_child_boxes(cell.children),
            anonymous: cell.anonymous,
        }
    }

    fn freeze_table_caption<'a>(
        &mut self,
        caption: MutableTableFragmentCaption<'a>,
    ) -> FrozenTableFragmentCaption<'a> {
        FrozenTableFragmentCaption {
            element: caption.element,
            signature: caption.signature,
            style: self.freeze_optional_style(caption.style),
            children: self.freeze_child_boxes(caption.children),
        }
    }

    fn freeze_table_column<'a>(
        &mut self,
        column: MutableTableFragmentColumn<'a>,
    ) -> FrozenTableFragmentColumn<'a> {
        FrozenTableFragmentColumn {
            element: column.element,
            signature: column.signature,
            style: self.freeze_optional_style(column.style),
            group: column
                .group
                .map(|group| self.freeze_table_column_group(group)),
            span: column.span,
        }
    }

    fn freeze_table_column_group<'a>(
        &mut self,
        group: MutableTableFragmentColumnGroup<'a>,
    ) -> FrozenTableFragmentColumnGroup<'a> {
        FrozenTableFragmentColumnGroup {
            element: group.element,
            signature: group.signature,
            style: self.freeze_optional_style(group.style),
            span: group.span,
        }
    }
}

impl<'a, S> FormattingBoxWith<'a, S>
where
    S: AsRef<ComputedStyle>,
{
    pub fn element_parts(
        &self,
    ) -> Option<(
        &'a Element,
        &ElementSignature,
        &ComputedStyle,
        &[FormattingBoxWith<'a, S>],
    )> {
        match self {
            Self::Block(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::Inline(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::InlineSplitBlockContext(_) => None,
            Self::AtomicInline(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::Table(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::Flex(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::Replaced(box_) => Some((
                box_.element,
                &box_.signature,
                box_.style.as_ref(),
                &box_.children,
            )),
            Self::AnonymousBlock(_) | Self::Text(_) => None,
        }
    }

    #[cfg(test)]
    pub fn kind(&self) -> FormattingBoxKind {
        match self {
            Self::Block(_) => FormattingBoxKind::Block,
            Self::Inline(_) => FormattingBoxKind::Inline,
            Self::InlineSplitBlockContext(_) => FormattingBoxKind::InlineSplitBlockContext,
            Self::AnonymousBlock(_) => FormattingBoxKind::AnonymousBlock,
            Self::AtomicInline(_) => FormattingBoxKind::AtomicInline,
            Self::Text(_) => FormattingBoxKind::Text,
            Self::Table(_) => FormattingBoxKind::Table,
            Self::Flex(_) => FormattingBoxKind::Flex,
            Self::Replaced(_) => FormattingBoxKind::Replaced,
        }
    }

    #[cfg(test)]
    pub fn style(&self) -> &ComputedStyle {
        match self {
            Self::Block(box_) => box_.style.as_ref(),
            Self::Inline(box_) => box_.style.as_ref(),
            Self::InlineSplitBlockContext(box_) => box_.style.as_ref(),
            Self::AnonymousBlock(box_) => box_.style.as_ref(),
            Self::AtomicInline(box_) => box_.style.as_ref(),
            Self::Text(box_) => box_.style.as_ref(),
            Self::Table(box_) => box_.style.as_ref(),
            Self::Flex(box_) => box_.style.as_ref(),
            Self::Replaced(box_) => box_.style.as_ref(),
        }
    }

    pub(crate) fn children(&self) -> &[FormattingBoxWith<'a, S>] {
        match self {
            Self::Block(box_) => &box_.children,
            Self::Inline(box_) => &box_.children,
            Self::InlineSplitBlockContext(box_) => &box_.children,
            Self::AnonymousBlock(box_) => &box_.children,
            Self::AtomicInline(box_) => &box_.children,
            Self::Table(box_) => &box_.children,
            Self::Flex(box_) => &box_.children,
            Self::Replaced(box_) => &box_.children,
            Self::Text(_) => &[],
        }
    }
}

/// Identifies whether an element-backed formatting box is an element's
/// principal box or a generated pseudo-element box.
///
/// CSS Pseudo-Elements defines `::before` and `::after` as tree-abiding
/// generated boxes whose style/content are resolved from an originating
/// element, while CSS Display applies normal box-generation transformations
/// such as positioned blockification to the generated box itself:
/// <https://www.w3.org/TR/css-pseudo-4/#generated-content> and
/// <https://www.w3.org/TR/css-display-3/#transformations>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoxSource<'a> {
    Principal,
    GeneratedPseudo(Box<GeneratedPseudoBox<'a>>),
}

/// Origin metadata for a generated `::before` or `::after` formatting box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GeneratedPseudoBox<'a> {
    pub(crate) originating_element: &'a Element,
    pub(crate) originating_signature: ElementSignature,
    pub(crate) originating_clear: Clear,
    pub(crate) kind: GeneratedPseudoKind,
}

/// Supported tree-abiding generated pseudo-element kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneratedPseudoKind {
    Before,
    After,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingBoxKind {
    Block,
    Inline,
    InlineSplitBlockContext,
    AnonymousBlock,
    AtomicInline,
    Text,
    Table,
    Flex,
    Replaced,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub run_in_children: Vec<FormattingBoxWith<'a, S>>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub fragment_edges: InlineBoxFragmentEdges,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}

/// Block-level segment generated by splitting an inline around an in-flow block.
///
/// CSS 2.2 splits an inline box that contains an in-flow block-level box, but
/// relative positioning and Appendix E stacking still apply to all generated
/// boxes for that inline. This transparent block-flow context preserves the
/// inline ancestor's visual style without adding wrapper box edges:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>,
/// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>, and
/// <https://www.w3.org/TR/CSS22/zindex.html>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineSplitBlockContextBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub children: Vec<FormattingBoxWith<'a, S>>,
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
pub(crate) struct AnonymousBlockBoxWith<'a, S = MutableStyle> {
    pub style: S,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AtomicInlineBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
    pub table_fragment: Option<TableFragmentWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkerBoxWith<S = MutableStyle> {
    pub style: S,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextBoxWith<S = MutableStyle> {
    pub text: String,
    pub style: S,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
    pub fragment: TableFragmentWith<'a, S>,
}

/// Durable CSS table formatting fragment built during box-tree construction.
///
/// CSS 2.2 table layout first constructs a table grid with row groups,
/// columns, captions, and row/column-span occupancy before width, height, and
/// border resolution:
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentWith<'a, S = MutableStyle> {
    pub rows: Vec<TableFragmentRowWith<'a, S>>,
    pub captions: Vec<TableFragmentCaptionWith<'a, S>>,
    pub columns: Vec<TableFragmentColumnWith<'a, S>>,
    pub grid: TableFragmentGrid,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentRowWith<'a, S = MutableStyle> {
    pub element: Option<&'a Element>,
    pub signature: ElementSignature,
    pub ancestors: Vec<ElementSignature>,
    pub row_groups: Vec<TableFragmentRowGroupWith<'a, S>>,
    pub style: Option<S>,
    pub cells: Vec<TableFragmentCellWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentRowGroupWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<S>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentCellWith<'a, S = MutableStyle> {
    pub element: Option<&'a Element>,
    pub signature: ElementSignature,
    pub style: Option<S>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
    pub anonymous: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentCaptionWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<S>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentColumnWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<S>,
    pub group: Option<TableFragmentColumnGroupWith<'a, S>>,
    pub span: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableFragmentColumnGroupWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub style: Option<S>,
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
pub(crate) struct FlexBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplacedBoxWith<'a, S = MutableStyle> {
    pub element: &'a Element,
    pub signature: ElementSignature,
    pub source: BoxSource<'a>,
    pub style: S,
    pub marker: Option<MarkerBoxWith<S>>,
    pub children: Vec<FormattingBoxWith<'a, S>>,
}
