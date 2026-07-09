use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Lay out in-flow block descendants inside a table cell content box.
    ///
    /// CSS 2.2 says a table-cell box contains a block container, and its
    /// in-flow descendants therefore participate in normal block formatting
    /// inside the cell after row and column sizing:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_table_cell_flow_children(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        row_sizing_style: &ComputedStyle,
        table_style: &ComputedStyle,
        table_height_is_definite: bool,
        stylesheets: &[Stylesheet],
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let built_children;
        let children = if let Some(children) = cell.children.as_deref() {
            children
        } else if let Some(element) = cell.element {
            built_children = self.build_frozen_child_boxes_with_font_metrics(
                element,
                stylesheets,
                cell_style,
                &self.table_cell_child_ancestors(cell, row),
            );
            &built_children
        } else {
            return;
        };
        if !children.iter().any(table_cell_has_in_flow_layout_child) {
            return;
        }

        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            cell_borders,
            content_offset,
            content_x_offset,
        );
        let cell_content_height = content_box.height();
        let percentage_height_basis = table_cell_percentage_height_basis(
            row_sizing_style,
            table_style,
            cell_content_height,
            cell_borders,
            table_height_is_definite,
        );
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            content_box,
            self.table_cell_child_ancestors(cell, row),
            percentage_height_basis,
        );
        // Table-row fragmentation owns the fragmentainer boundary for its
        // cells. Descendant block layout is a replay into that committed row
        // fragment, so it must not create a second, page-relative break of
        // its own.
        // <https://drafts.csswg.org/css-tables-3/#table-layout>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        self.fragmentation_suppression_depth += 1;
        self.push_float_context();
        if formatting_box_has_inline_content(children) && !has_non_inline_formatting_box(children) {
            // Cell positioning is resolved after the table grid establishes
            // its final geometry. The anonymous inline collector still builds
            // the in-flow line items here, but must not create a provisional
            // page-relative layer for an out-of-flow child; the dedicated cell
            // positioned pass below owns that child exactly once.
            // <https://www.w3.org/TR/css-position-3/#def-cb>
            self.positioned_inline_layout_suppression_depth += 1;
            self.layout_anonymous_block(cell_style, children, stylesheets, None);
            self.positioned_inline_layout_suppression_depth -= 1;
        } else {
            let mut float_run = self.float_run_state();
            for child_box in children {
                if table_cell_child_is_in_flow_float(child_box) {
                    let Some((child_element, child_signature, child_style, child_children)) =
                        child_box.element_parts()
                    else {
                        continue;
                    };
                    let table_fragment = if let box_tree::FormattingBox::Table(box_) = child_box {
                        Some(&box_.fragment)
                    } else {
                        None
                    };
                    if self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        child_style,
                        Some(child_children),
                        table_fragment,
                        stylesheets,
                        &mut float_run,
                    ) {
                        continue;
                    }
                }
                if table_cell_has_in_flow_layout_child(child_box) {
                    self.layout_formatting_box(child_box, stylesheets);
                }
            }
            self.flush_float_run(&mut float_run);
        }
        self.pop_float_context();
        self.fragmentation_suppression_depth -= 1;

        self.restore_table_cell_content_scope(content_scope);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_table_cell_positioned_children(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        row_containing_block: Option<ContainingBlock>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        _content_offset: f32,
        content_x_offset: f32,
    ) {
        // A positioned descendant's static position is the position it would
        // have in normal flow before cell vertical alignment moves in-flow
        // content within the final row. The alignment offset is therefore not
        // part of this positioning scope.
        // <https://drafts.csswg.org/css-tables-3/#abspos-boxes-in-table-internal>
        let static_content_box = border_box.content_box(
            placement,
            cell_style.padding,
            cell_borders,
            0.0,
            content_x_offset,
        );
        let cell_containing_block_scope = self.push_table_cell_containing_block_if_positioned(
            cell_style,
            border_box,
            placement,
            cell_borders,
        );
        let row_containing_block_scope = if cell_containing_block_scope.is_none() {
            self.push_table_row_containing_block_if_positioned(row_style, row_containing_block)
        } else {
            None
        };

        let child_ancestors = self.table_cell_child_ancestors(cell, row);
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            static_content_box,
            child_ancestors.clone(),
            PercentageBasis::indefinite(),
        );
        if let Some(children) = cell.children.as_deref() {
            self.layout_table_cell_positioned_boxes(children, stylesheets);
        } else if let Some(element) = cell.element {
            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            for child in &element.children {
                let NodeKind::Element(child_element) = &child.kind else {
                    continue;
                };
                let child_signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = self
                    .style_for_layout_element_with_parent_font_metrics_and_ancestors(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(cell_style),
                        &child_ancestors,
                    );
                if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                self.push_ancestor_signature(child_signature);
                self.layout_element(child_element, &child_style, stylesheets);
                self.ancestors.pop();
            }
        }

        if let Some(scope) = cell_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        } else if let Some(scope) = row_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.restore_table_cell_content_scope(content_scope);
    }

    /// Lay out table-cell positioned descendants that have passed through
    /// anonymous block construction. These wrappers are formatting structure,
    /// not DOM ancestors, and therefore must not prevent a direct positioned
    /// descendant from reaching the table-internal containing-block pass.
    /// <https://www.w3.org/TR/css-display-3/#anonymous>
    fn layout_table_cell_positioned_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
    ) {
        for child_box in children {
            match child_box {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    self.layout_table_cell_positioned_boxes(&box_.children, stylesheets);
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    self.layout_table_cell_positioned_boxes(&box_.children, stylesheets);
                }
                _ => {
                    let Some((child, child_signature, child_style, child_children)) =
                        child_box.element_parts()
                    else {
                        continue;
                    };
                    if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                        continue;
                    }
                    self.push_ancestor_signature(child_signature.clone());
                    self.layout_element_with_child_boxes(
                        child,
                        child_style,
                        stylesheets,
                        Some(child_children),
                    );
                    self.ancestors.pop();
                }
            }
        }
    }

    pub(in crate::layout::table) fn push_table_cell_containing_block_if_positioned(
        &mut self,
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        cell_borders: css::Edges,
    ) -> Option<PositionedContainingBlockScope> {
        let mode = PositionedContainingBlockMode::for_style(cell_style)?;
        let containing_block = placement.containing_block_for(border_box, cell_borders);
        Some(self.push_positioned_containing_block(mode, containing_block))
    }

    /// Table rows are table-internal boxes, but a positioned row still
    /// establishes the containing block for absolutely positioned descendants
    /// of its cells. The row's final grid-piece geometry is therefore kept
    /// separate from the cell's own containing block.
    /// <https://drafts.csswg.org/css-tables-3/#abspos-boxes-in-table-internal>
    fn push_table_row_containing_block_if_positioned(
        &mut self,
        row_style: &ComputedStyle,
        containing_block: Option<ContainingBlock>,
    ) -> Option<PositionedContainingBlockScope> {
        if !matches!(row_style.position, Position::Relative | Position::Sticky) {
            return None;
        }
        let containing_block = containing_block?;
        Some(self.push_positioned_containing_block(
            PositionedContainingBlockMode::AbsoluteOnly,
            containing_block,
        ))
    }

    pub(in crate::layout::table) fn table_cell_child_ancestors(
        &self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
    ) -> Vec<ElementSignature> {
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        ancestors.push(row.signature.clone());
        ancestors.push(cell.signature.clone());
        ancestors
    }
}
