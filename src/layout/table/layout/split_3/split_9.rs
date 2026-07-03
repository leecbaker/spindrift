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
        stylesheets: &[Stylesheet],
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let Some(children) = cell.children.as_deref() else {
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
        );
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            content_box,
            self.table_cell_child_ancestors(cell, row),
            percentage_height_basis,
        );

        self.push_float_context();
        if formatting_box_has_inline_content(children) && !has_non_inline_formatting_box(children) {
            self.layout_anonymous_block(cell_style, children, stylesheets, None);
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

        self.restore_table_cell_content_scope(content_scope);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_table_cell_positioned_children(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            cell_borders,
            content_offset,
            content_x_offset,
        );
        let containing_block_pushed = self.push_table_cell_containing_block_if_positioned(
            cell_style,
            border_box,
            placement,
            cell_borders,
        );

        let child_ancestors = self.table_cell_child_ancestors(cell, row);
        let content_scope = self.enter_table_cell_content_scope(
            cell_style,
            content_box,
            child_ancestors.clone(),
            None,
        );
        if let Some(children) = cell.children.as_deref() {
            for child_box in children {
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

        if containing_block_pushed {
            self.containing_blocks.pop();
        }
        self.restore_table_cell_content_scope(content_scope);
    }

    pub(in crate::layout::table) fn push_table_cell_containing_block_if_positioned(
        &mut self,
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        cell_borders: css::Edges,
    ) -> bool {
        if !matches!(cell_style.position, Position::Relative | Position::Sticky) {
            return false;
        }
        self.containing_blocks
            .push(placement.containing_block_for(border_box, cell_borders));
        true
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
