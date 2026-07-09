use super::*;

/// Return the block-size basis used by CSS Tables 3 table-cell content relayout.
///
/// Percentage heights on table-cell descendants are resolved during the second
/// content layout pass only when the cell itself has an explicit length height,
/// or when its table root has a length or percentage height:
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
pub(in crate::layout::table) fn table_cell_percentage_height_basis(
    cell_style: &ComputedStyle,
    table_style: &ComputedStyle,
    final_content_height: f32,
    border_insets: css::Edges,
    table_height_is_definite: bool,
) -> BlockSizePercentageBasis {
    if table_cell_content_relayout_policy(cell_style, table_style, table_height_is_definite)
        != TableCellContentSizingPolicy::FinalRelayout
    {
        return PercentageBasis::indefinite();
    }
    if cell_style
        .box_values
        .height
        .length_if_no_percent()
        .is_some()
    {
        return table_cell_explicit_content_height_basis(cell_style, border_insets);
    }
    PercentageBasis::definite_from(
        content_box_pt(final_content_height),
        BlockSizeBasisSource::TableCell,
    )
}

/// Return a table cell's own explicit content-box height as a percentage basis.
///
/// A length-sized cell establishes a definite block-size containing block for
/// its contents even while row sizing is measuring its intrinsic minimum.
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>
pub(in crate::layout::table) fn table_cell_explicit_content_height_basis(
    cell_style: &ComputedStyle,
    border_insets: css::Edges,
) -> BlockSizePercentageBasis {
    let vertical_non_content = cell_style.padding.top
        + cell_style.padding.bottom
        + border_insets.top
        + border_insets.bottom;
    used_content_box_height_or_auto_with_basis(
        cell_style,
        percentage_basis_from_points(None),
        non_content_pt(vertical_non_content),
    )
    .map(|height| PercentageBasis::definite_from(height, BlockSizeBasisSource::TableCell))
    .unwrap_or_else(PercentageBasis::indefinite)
}

pub(in crate::layout::table) fn table_cell_content_relayout_policy(
    cell_style: &ComputedStyle,
    table_style: &ComputedStyle,
    table_height_is_definite: bool,
) -> TableCellContentSizingPolicy {
    if cell_style
        .box_values
        .height
        .length_if_no_percent()
        .is_some()
    {
        return TableCellContentSizingPolicy::FinalRelayout;
    }
    if table_height_is_definite
        && (matches!(
            table_style.box_values.height.clone(),
            css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        ) || matches!(
            table_style.box_values.min_height.clone(),
            css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        ))
    {
        return TableCellContentSizingPolicy::FinalRelayout;
    }
    TableCellContentSizingPolicy::RowMinimum
}

pub(in crate::layout::table) fn apply_table_cell_content_sizing_policy(
    style: &mut ComputedStyle,
    policy: TableCellContentSizingPolicy,
) {
    if policy != TableCellContentSizingPolicy::RowMinimum {
        return;
    }
    if table_cell_block_size_depends_on_parent_percentage(style.box_values.height.clone()) {
        set_style_auto_height(style);
    }
    if table_cell_block_size_depends_on_parent_percentage(style.box_values.min_height.clone()) {
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
    }
    if table_cell_block_size_depends_on_parent_percentage(style.box_values.max_height.clone()) {
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;
    }
}

pub(in crate::layout::table) fn table_cell_block_size_depends_on_parent_percentage(
    value: css::ComputedLengthPercentageOrAuto,
) -> bool {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
        | css::ComputedLengthPercentageOrAuto::FitContent(Some(value)) => {
            value.needs_percentage_basis()
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(None)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => false,
    }
}

pub(in crate::layout::table) fn table_cell_style_has_parent_percentage_block_size(
    style: &ComputedStyle,
) -> bool {
    table_cell_block_size_depends_on_parent_percentage(style.box_values.height.clone())
        || table_cell_block_size_depends_on_parent_percentage(style.box_values.min_height.clone())
        || table_cell_block_size_depends_on_parent_percentage(style.box_values.max_height.clone())
}

pub(in crate::layout::table) fn table_cell_formatting_child_has_parent_percentage_block_size(
    child: &box_tree::FormattingBox<'_>,
) -> bool {
    match child {
        box_tree::FormattingBox::Block(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .children
                    .iter()
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::Table(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .fragment
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .flat_map(|cell| cell.children.iter())
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::Flex(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .children
                    .iter()
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::Inline(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .children
                    .iter()
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::AtomicInline(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .children
                    .iter()
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::Replaced(box_) => {
            table_cell_style_has_parent_percentage_block_size(&box_.style)
                || box_
                    .children
                    .iter()
                    .any(table_cell_formatting_child_has_parent_percentage_block_size)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(table_cell_formatting_child_has_parent_percentage_block_size),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .children
            .iter()
            .any(table_cell_formatting_child_has_parent_percentage_block_size),
        box_tree::FormattingBox::Text(_) => false,
    }
}

/// Return whether a cell joins its row baseline-sharing group.
///
/// CSS Box Alignment only lets first/last baseline-aligned table cells
/// participate when the cell inline axis is parallel to the table row's inline
/// axis; orthogonal cells use baseline fallback instead:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
pub(in crate::layout::table) fn table_cell_participates_in_baseline(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
) -> bool {
    if inline_start_side(cell_style.writing_mode, cell_style.direction).axis()
        != inline_start_side(row_style.writing_mode, row_style.direction).axis()
    {
        return false;
    }

    if cell_style.align_content.keyword != ContentAlignmentKeyword::Normal {
        return matches!(
            cell_style.align_content.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        );
    }

    matches!(
        cell_style.vertical_align.table_cell_align,
        TableCellVerticalAlign::Baseline
    )
}

pub(in crate::layout::table) fn table_cell_participates_in_row_baseline(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
    placement: &TableCellPlacement,
) -> bool {
    if !table_cell_participates_in_baseline(cell_style, row_style) {
        return false;
    }
    // CSS Align assigns row-spanning cells to the start-most row for first
    // baseline alignment and to the end-most row for last baseline alignment.
    // TableGrid stores a spanning cell on its start-most row; the end-most row
    // target for last baseline is resolved later when painting the origin row.
    // <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
    placement.rowspan == 1
        || table_cell_alignment_baseline_set(cell_style) == TableCellBaselineSet::First
}

/// Return whether a row baseline can be consumed as a physical Y offset.
///
/// CSS Tables aligns table-cell baselines along the row's baseline-sharing
/// axis, while CSS Writing Modes maps a vertical-writing cell baseline onto
/// the horizontal block axis. Quire's current row-height and `content_offset`
/// paths store only physical Y offsets, so horizontal-axis baselines must not
/// inflate row heights or move content vertically:
/// <https://drafts.csswg.org/css-tables-3/#table-cell-baseline> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(in crate::layout::table) fn table_cell_participates_in_physical_y_row_baseline(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
    placement: &TableCellPlacement,
) -> bool {
    table_cell_participates_in_row_baseline(cell_style, row_style, placement)
        && table_cell_baseline_offset_axis(cell_style) == PhysicalAxis::Vertical
}

pub(in crate::layout::table) fn table_cell_can_consume_physical_y_row_baseline_for_alignment(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
) -> bool {
    table_cell_participates_in_baseline(cell_style, row_style)
        && table_cell_baseline_offset_axis(cell_style) == PhysicalAxis::Vertical
}

pub(in crate::layout::table) fn table_cell_baseline_offset_axis(
    cell_style: &ComputedStyle,
) -> PhysicalAxis {
    block_start_side(cell_style.writing_mode).axis()
}

pub(in crate::layout::table) fn table_cell_alignment_baseline_set(
    style: &ComputedStyle,
) -> TableCellBaselineSet {
    if style.align_content.keyword == ContentAlignmentKeyword::LastBaseline {
        TableCellBaselineSet::Last
    } else {
        TableCellBaselineSet::First
    }
}

/// Return whether inline table-cell contents expose a textual line baseline.
///
/// CSS table cells fall back to the bottom content edge when no in-flow line
/// box baseline is available; atomic inline-only content must therefore not be
/// mistaken for text-baseline content:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
pub(in crate::layout::table) fn formatting_boxes_have_textual_baseline(
    children: &[box_tree::FormattingBox<'_>],
) -> bool {
    children.iter().any(|child| match child {
        box_tree::FormattingBox::Text(_) => !box_tree::formatting_box_is_collapsible_space(child),
        box_tree::FormattingBox::Inline(box_) => {
            formatting_boxes_have_textual_baseline(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            formatting_boxes_have_textual_baseline(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            formatting_boxes_have_textual_baseline(&box_.children)
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}

pub(in crate::layout::table) fn table_cell_textual_baseline_style<'a>(
    children: &'a [box_tree::FormattingBox<'_>],
    baseline_set: TableCellBaselineSet,
) -> Option<&'a ComputedStyle> {
    match baseline_set {
        TableCellBaselineSet::First => children.iter().find_map(table_cell_first_textual_style),
        TableCellBaselineSet::Last => children
            .iter()
            .rev()
            .find_map(table_cell_last_textual_style),
    }
}

pub(in crate::layout::table) fn table_cell_textual_children_match_baseline_style(
    children: &[box_tree::FormattingBox<'_>],
    style: &ComputedStyle,
) -> bool {
    children.iter().all(|child| match child {
        box_tree::FormattingBox::Text(box_) => {
            box_tree::formatting_box_is_collapsible_space(child)
                || table_cell_text_style_matches_baseline_style(&box_.style, style)
        }
        box_tree::FormattingBox::Inline(box_) => {
            table_cell_textual_children_match_baseline_style(&box_.children, style)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_textual_children_match_baseline_style(&box_.children, style)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            table_cell_textual_children_match_baseline_style(&box_.children, style)
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Replaced(_) => true,
    })
}

pub(in crate::layout::table) fn table_cell_text_style_matches_baseline_style(
    text_style: &ComputedStyle,
    baseline_style: &ComputedStyle,
) -> bool {
    text_style.color == baseline_style.color
        || ((text_style.font_size - baseline_style.font_size).abs() < 0.01
            && (text_style.line_height - baseline_style.line_height).abs() < 0.01)
}

pub(in crate::layout::table) fn table_cell_first_textual_style<'a>(
    child: &'a box_tree::FormattingBox<'_>,
) -> Option<&'a ComputedStyle> {
    match child {
        box_tree::FormattingBox::Text(box_) => {
            (!box_tree::formatting_box_is_collapsible_space(child)).then_some(&*box_.style)
        }
        box_tree::FormattingBox::Inline(box_) => box_
            .children
            .iter()
            .find_map(table_cell_first_textual_style),
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .find_map(table_cell_first_textual_style),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .children
            .iter()
            .find_map(table_cell_first_textual_style),
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Replaced(_) => None,
    }
}

pub(in crate::layout::table) fn table_cell_last_textual_style<'a>(
    child: &'a box_tree::FormattingBox<'_>,
) -> Option<&'a ComputedStyle> {
    match child {
        box_tree::FormattingBox::Text(box_) => {
            (!box_tree::formatting_box_is_collapsible_space(child)).then_some(&*box_.style)
        }
        box_tree::FormattingBox::Inline(box_) => box_
            .children
            .iter()
            .rev()
            .find_map(table_cell_last_textual_style),
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .rev()
            .find_map(table_cell_last_textual_style),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .children
            .iter()
            .rev()
            .find_map(table_cell_last_textual_style),
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Replaced(_) => None,
    }
}

/// Return whether a table-cell child needs a nested formatting-context pass.
///
/// CSS 2.2 table cells contain a block container. Anonymous blocks that hold
/// text runs, atomic inline boxes, and floated inline boxes still create
/// formatting content and must be laid out after table row sizing rather than
/// being treated as empty cells. Floated boxes are blockified and placed as
/// out-of-flow floats in that block formatting context:
/// <https://www.w3.org/TR/CSS22/tables.html#model>,
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>, and
/// <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>.
pub(in crate::layout::table) fn table_cell_has_in_flow_layout_child(
    child_box: &box_tree::FormattingBox<'_>,
) -> bool {
    match child_box {
        box_tree::FormattingBox::Block(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Table(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Flex(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(table_cell_has_in_flow_layout_child),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .children
            .iter()
            .any(table_cell_has_in_flow_layout_child),
        box_tree::FormattingBox::Inline(box_) => {
            box_.style.float != Float::None
                || box_
                    .children
                    .iter()
                    .any(table_cell_has_in_flow_layout_child)
        }
        box_tree::FormattingBox::AtomicInline(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Replaced(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Text(_) => {
            !box_tree::formatting_box_is_collapsible_space(child_box)
        }
    }
}

pub(in crate::layout::table) fn table_cell_child_is_in_flow_float(
    child_box: &box_tree::FormattingBox<'_>,
) -> bool {
    child_box.element_parts().is_some_and(|(_, _, style, _)| {
        style.float != Float::None
            && !style.display.is_none()
            && !matches!(style.position, Position::Absolute | Position::Fixed)
    })
}

pub(in crate::layout::table) fn table_cell_measured_inline_outer_height_without_policy(
    child: &box_tree::FormattingBox<'_>,
    available_width: f32,
) -> Option<f32> {
    match child {
        box_tree::FormattingBox::Inline(box_) => {
            if matches!(box_.style.position, Position::Absolute | Position::Fixed) {
                Some(0.0)
            } else {
                Some(table_cell_formatting_child_outer_height(child).points())
            }
        }
        box_tree::FormattingBox::AtomicInline(box_)
            if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Canvas) =>
        {
            Some(table_cell_canvas_first_pass_outer_height(
                box_.element,
                &box_.style,
                available_width,
            ))
        }
        box_tree::FormattingBox::Replaced(box_)
            if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Canvas) =>
        {
            Some(table_cell_canvas_first_pass_outer_height(
                box_.element,
                &box_.style,
                available_width,
            ))
        }
        box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => {
            Some(table_cell_formatting_child_outer_height(child).points())
        }
        box_tree::FormattingBox::AnonymousBlock(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Text(_) => None,
    }
}

/// Measure a canvas replaced element for first-pass table row layout.
///
/// CSS Tables 3 says table-cell descendants whose height depends on
/// percentages of the parent cell are treated as auto during first-pass row
/// layout; the real percentage is handled in table-cell content relayout.
/// <https://drafts.csswg.org/css-tables-3/#row-layout>.
pub(in crate::layout::table) fn table_cell_canvas_first_pass_outer_height(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    let canvas = used_canvas(
        element,
        style,
        available_width,
        BlockSizePercentageBasis::indefinite(),
    );
    canvas.border_box_size.height + style.margin.top + style.margin.bottom
}

pub(in crate::layout::table) fn table_cell_child_fragment_kind(
    child_box: &box_tree::FormattingBox<'_>,
) -> Option<TableCellChildFragmentKind> {
    match child_box {
        box_tree::FormattingBox::Block(_) => Some(TableCellChildFragmentKind::Block),
        box_tree::FormattingBox::InlineSplitBlockContext(_) => {
            Some(TableCellChildFragmentKind::Block)
        }
        box_tree::FormattingBox::AnonymousBlock(_) => {
            Some(TableCellChildFragmentKind::AnonymousBlock)
        }
        box_tree::FormattingBox::Inline(_) => Some(TableCellChildFragmentKind::Inline),
        box_tree::FormattingBox::Text(_) => Some(TableCellChildFragmentKind::Text),
        box_tree::FormattingBox::AtomicInline(_) => Some(TableCellChildFragmentKind::AtomicInline),
        box_tree::FormattingBox::Replaced(_) => Some(TableCellChildFragmentKind::Replaced),
        box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Flex(_) => {
            Some(TableCellChildFragmentKind::NestedFormattingContext)
        }
    }
}

pub(in crate::layout::table) fn table_cell_children_can_use_inline_line_sequence(
    children: &[box_tree::FormattingBox<'_>],
) -> bool {
    children.iter().all(|child| match child {
        box_tree::FormattingBox::Text(_) => true,
        box_tree::FormattingBox::Inline(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
                && box_.style.float == Float::None
                && table_cell_children_can_use_inline_line_sequence(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_children_can_use_inline_line_sequence(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(_) => false,
        box_tree::FormattingBox::AtomicInline(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
                && box_.style.float == Float::None
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_element_by_tag<'a>(node: &'a Node, tag: &str) -> Option<&'a Element> {
        match &node.kind {
            NodeKind::Text(_) => None,
            NodeKind::Element(element) => {
                if element.tag == tag {
                    return Some(element);
                }
                element
                    .children
                    .iter()
                    .find_map(|child| first_element_by_tag(child, tag))
            }
        }
    }

    #[tokio::test]
    async fn table_row_style_reconstruction_uses_measured_parent_ch_for_font_size() {
        let root = dom::parse("<html><body><table><tr><td>Cell</td></tr></table></body></html>");
        let row_element = first_element_by_tag(&root, "tr").expect("expected table row");
        let stylesheet = css::parse_stylesheet(
            &css::Css::from_string(
                r#"@font-face {
                    font-family: MetricProbe;
                    src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
                }
                tr { font-size: 2ch }"#,
            )
            .with_base_path(".")
            .expect("current directory should be a valid file URL"),
        );
        let font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(std::slice::from_ref(&stylesheet))
            .finish()
            .await;
        let options = RenderOptions::default();
        let stylesheets = vec![stylesheet];
        let resource_cache = ResourceCache::default();
        let iframe_documents = HashMap::new();
        let mut builder = LayoutBuilder::new(LayoutBuilderConfig {
            options: &options,
            stylesheets: &stylesheets,
            base_url: None,
            root_url: None,
            resource_cache: &resource_cache,
            iframe_documents: &iframe_documents,
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system,
        });
        let mut table_style = ComputedStyle {
            font_family: css::FontFamily::Names(vec!["MetricProbe".to_string()]),
            font_size: 40.0,
            line_height: 40.0,
            ..ComputedStyle::initial()
        };
        table_style.line_height_value = css::ComputedLineHeight::from_points(40.0);
        let parent_ch_advance = builder.font_system.ch_advance(&table_style);
        assert!(
            (parent_ch_advance.points() - table_style.font_size * 0.5).abs() > 0.01,
            "fixture must differ from the generic 0.5em ch fallback"
        );
        let row = TableRow {
            element: Some(row_element),
            signature: ElementSignature::new("tr", row_element.attrs.clone()),
            ancestors: Vec::new(),
            row_groups: Vec::new(),
            style: None,
            cells: Vec::new(),
            running_cells: Vec::new(),
        };

        let row_style = builder.style_for_table_row(&row, &table_style, &stylesheets);

        assert!((row_style.font_size - parent_ch_advance.points() * 2.0).abs() < 0.01);
    }
}
