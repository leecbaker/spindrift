use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Return min-content and max-content grid widths for a durable table fragment.
    ///
    /// CSS Tables computes intrinsic table widths from the row/column grid and
    /// cell min/max-content measures. Reusing the durable fragment keeps
    /// inline-table and positioned sizing aligned with the table object
    /// construction used for normal layout:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
    pub(in crate::layout) fn table_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            // An empty grid has zero intrinsic column width. A preferred
            // table `width` is resolved later as a preferred/flex main size,
            // not as its min-content contribution; otherwise an empty
            // `width: 500px; flex-basis: 100px` table cannot shrink to its
            // flex basis.
            // <https://drafts.csswg.org/css-tables/#computing-the-table-width>
            return (0.0, 0.0);
        }

        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    grid.column_count,
                )
            });
        let measures = self.table_column_measures(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width.points(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry.as_ref(),
        );
        let min_content = measures.table_min_content_width().max(0.0);
        let max_content = measures.table_max_content_width().max(min_content);
        (min_content, max_content)
    }

    /// Return parent-facing content-box intrinsic widths for a table fragment.
    ///
    /// CSS Tables computes grid min/max-content widths from column measures,
    /// but CSS Sizing intrinsic contributions also honor a non-auto preferred
    /// size. For auto-layout tables, the used table content box is clamped so
    /// it is not smaller than the grid min-content width:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_parent_intrinsic_content_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        self.table_parent_intrinsic_content_widths_with_percentage_resolution(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
            true,
        )
    }

    /// Return table intrinsic widths when percentage sizes have no containing
    /// block basis.
    ///
    /// CSS Sizing treats a percentage preferred size as `auto` for intrinsic
    /// sizing against an indefinite containing block. Flexbox needs this
    /// query for the automatic minimum, while its definite main-size probe
    /// uses [`Self::table_parent_intrinsic_content_widths_from_fragment`]:
    /// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing> and
    /// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    pub(in crate::layout) fn table_parent_intrinsic_content_widths_with_indefinite_percentage_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        self.table_parent_intrinsic_content_widths_with_percentage_resolution(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
            false,
        )
    }

    /// Compute parent-facing table intrinsic widths with explicit percentage
    /// resolution.
    ///
    /// The table grid's min/max measures are independent from whether the
    /// wrapper's preferred width can resolve a percentage. Keeping the basis
    /// at this boundary prevents callers from accidentally treating a
    /// definite preferred width as an intrinsic minimum.
    /// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
    fn table_parent_intrinsic_content_widths_with_percentage_resolution(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
        resolve_percentage: bool,
    ) -> (f32, f32) {
        let (min_content, max_content) = self.table_intrinsic_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_non_content = table_horizontal_non_content_width(style, table_width);
        let percentage_basis = resolve_percentage
            .then(|| content_box_pt(available_table_width))
            .map(PercentageBasis::definite)
            .unwrap_or_else(PercentageBasis::indefinite);
        let resolved_width = used_content_box_width_or_auto_with_basis(
            style,
            percentage_basis,
            non_content_pt(horizontal_non_content),
        )
        .map(SemanticLengthExt::points)
        .or_else(|| {
            intrinsic::intrinsic_content_box_width_keyword(
                table_root_inline_size(style),
                content_box_pt(min_content),
                content_box_pt(max_content),
                layout_pt(available_table_width),
                non_content_pt(horizontal_non_content),
            )
            .map(SemanticLengthExt::points)
        })
        .map(|width| {
            constrain_content_width(
                style,
                content_box_pt(width),
                PercentageBasis::definite(layout_pt(available_table_width.max(style.font_size))),
            )
            .points()
        });

        if let Some(width) = resolved_width {
            let width = table_content_width_clamped_to_min_content(style, width, min_content);
            (width, width)
        } else {
            (min_content, max_content)
        }
    }

    /// Return parent-facing margin-box intrinsic widths for a table fragment.
    ///
    /// Table parents consume the table wrapper/margin box, while table layout
    /// itself consumes the grid/content width. Keep this conversion separate so
    /// grid sizing remains available for column layout:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_outer_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if style.writing_mode.has_vertical_lines() {
            return self.table_vertical_outer_intrinsic_widths_from_fragment(
                element,
                style,
                stylesheets,
                fragment,
                available_outer_width,
            );
        }
        let (min_content, max_content) = self.table_parent_intrinsic_content_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_extras = table_horizontal_non_content_width(style, table_width)
            + style.margin.left
            + style.margin.right;
        (
            min_content + horizontal_extras,
            max_content + horizontal_extras,
        )
    }

    /// Return the physical-width contribution of a vertical table wrapper.
    ///
    /// Table columns run on the root table's logical inline axis.  In a
    /// vertical writing mode that axis projects to physical height, so the
    /// column min/max contribution cannot be handed directly to a horizontal
    /// float or parent block as its width.  Measure the table's logical block
    /// tracks and project that extent at this parent-facing boundary instead.
    ///
    /// <https://drafts.csswg.org/css-tables-3/#table-layout>
    /// <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
    fn table_vertical_outer_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let mut table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            let content = used_empty_table_grid_width(style, available_table_width, table_width);
            let physical_width = table_width.wrapper_border_box_width(content).points()
                + style.margin.left
                + style.margin.right;
            return (physical_width, physical_width);
        }

        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    grid.column_count,
                )
            });
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            available_table_width,
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width.points(),
            !table_root_inline_size(style).is_auto(),
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
        );
        if let Some(geometry) = &collapsed_geometry {
            table_width.border_widths = geometry.outer_insets;
        }
        let table_used_style = self.table_used_style(style);
        let context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: &table_used_style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics: table_metrics.clone(),
            collapsed_geometry: collapsed_geometry.as_ref(),
            wrapper_border_box_block_size: None,
            wrapper_non_grid_block_size: layout_pt(0.0),
        };
        let height_plan = self.table_height_plan(&context);
        let content_block_size = table_content_height(
            &height_plan.final_row_heights(),
            &height_plan.row_occupancy(),
            table_metrics,
        );
        let physical_width = content_block_size
            + table_width.padding.left
            + table_width.padding.right
            + table_width.border_widths.left
            + table_width.border_widths.right
            + style.margin.left
            + style.margin.right;
        (physical_width, physical_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inline_table_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        // CSS Display 3 maps `inline-table` to an inline-level atomic box whose
        // contents establish a table formatting context.
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        if rows.is_empty() {
            return None;
        }
        let grid = table_grid(rows);
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut table_width = used_table_width(style, available_width);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    grid.column_count,
                )
            });
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            available_width,
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width.points(),
            !table_root_inline_size(style).is_auto(),
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
        );
        let content_width = column_plan
            .total_width()
            .min(available_width)
            .max(style.font_size);
        let top = 10_000.0;
        let table_used_style = self.table_used_style(style);
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: &table_used_style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics: table_metrics.clone(),
            collapsed_geometry: collapsed_geometry.as_ref(),
            wrapper_border_box_block_size: None,
            wrapper_non_grid_block_size: layout_pt(0.0),
        };
        let table_height_plan = self.table_height_plan(&table_context);
        let planned_row_heights = table_height_plan.final_row_heights();
        let planned_row_occupancy = table_height_plan.row_occupancy();
        let top_caption_height = self.estimate_table_captions_height(
            &input.captions,
            style,
            stylesheets,
            content_width,
            CaptionSide::Top,
        );
        let first_row_baseline_range = inline_table_first_occupying_row_range(
            top,
            top_caption_height,
            table_width.border_widths,
            table_width.padding,
            &planned_row_heights,
            &planned_row_occupancy,
            table_metrics,
        );
        let table_strut_baseline_offset = self
            .font_system
            .rendered_first_line_baseline_offset(style)
            .points();

        let snapshot = self.snapshot();
        let mut table_style = style.clone();
        table_style.margin = css::Edges::ZERO;
        set_style_used_width(&mut table_style, content_width);
        table_style.break_before = PageBreak::Auto;
        table_style.break_after = PageBreak::Auto;

        self.current_page = Page::new(content_width, top);
        self.content_left = 0.0;
        self.content_right = content_width;
        self.cursor_y = top;
        self.truncate_page_start_margins = false;
        let _ = children;
        let scratch_page_count = self.pages.len();
        // An inline-table is an atomic inline-level box. Its isolated canvas
        // measures and paints the complete table atom; it is not a sequence of
        // document fragmentainers, so table rows must not materialize page
        // continuations that would be discarded when this scratch layout is
        // restored.
        // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
        // <https://www.w3.org/TR/css-break-3/#monolithic>
        self.fragmentation_suppression_depth += 1;
        self.layout_table(element, &table_style, stylesheets, fragment);
        self.fragmentation_suppression_depth -= 1;
        debug_assert_eq!(
            self.pages.len(),
            scratch_page_count,
            "atomic inline-table layout must not create scratch page continuations"
        );
        let content_height = (top - self.cursor_y).max(style.line_height);
        let fragment_bottom = top - content_height;
        // CSS 2.2 defines an `inline-table` baseline as the baseline of the
        // first row. In the current paint model, align against the first
        // rendered row line's actual shaped-font alignment coordinate.
        // https://www.w3.org/TR/CSS22/tables.html#table-display
        let baseline_offset = if style.contain.layout {
            // Layout-contained atomic boxes expose no internal baseline and
            // therefore synthesize one from their block-end border edge.
            // <https://www.w3.org/TR/css-contain-1/#containment-layout>
            content_height
        } else {
            first_row_baseline_range
                .and_then(|(row_top, row_bottom)| {
                    self.inline_table_baseline_offset_from_fragment(
                        row_top,
                        row_bottom,
                        fragment_bottom,
                        content_height,
                        table_strut_baseline_offset,
                    )
                })
                .unwrap_or(content_height)
        };
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintTranslation::new(0.0, -fragment_bottom));
        self.restore(snapshot);

        let mut atom_style = style.clone();
        atom_style.background_color = None;
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.padding = css::Edges::ZERO;

        Some(InlineAtom::new(
            InlineAtomContent::InlineFragment(Box::new(fragment)),
            atom_style,
            None,
            InlineSize::new(
                content_width + style.margin.left + style.margin.right,
                content_height + style.margin.top + style.margin.bottom,
            ),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        ))
    }

    /// Return the inline-table first-row baseline offset from the fragment top edge.
    ///
    /// CSS 2.2 defines an `inline-table` baseline as the baseline of the first
    /// table row. `quire` stores text using PDF baseline-adjusted glyph
    /// coordinates, so this maps the rendered line inside the first occupying
    /// row to the atom baseline offset from the table wrapper top edge used by
    /// the line builder.
    /// https://www.w3.org/TR/CSS22/tables.html#table-display
    pub(in crate::layout::table) fn inline_table_baseline_offset_from_fragment(
        &self,
        row_top: f32,
        row_bottom: f32,
        fragment_bottom: f32,
        content_height: f32,
        table_strut_baseline_offset: f32,
    ) -> Option<f32> {
        self.current_page
            .lines
            .iter()
            .map(|line| self.font_system.rendered_line_alignment_y(line).points())
            .find(|alignment_y| *alignment_y <= row_top + 0.01 && *alignment_y >= row_bottom - 0.01)
            .map(|alignment_y| {
                (content_height - (alignment_y - fragment_bottom) + table_strut_baseline_offset)
                    .clamp(0.0, content_height + table_strut_baseline_offset)
            })
    }

    pub(crate) fn estimate_table_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        fragment: &box_tree::TableFragment<'_>,
    ) -> f32 {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let captions = input.captions.as_slice();
        let columns = input.columns.as_slice();

        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            return self.estimate_empty_table_height(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
            );
        }
        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    columns,
                    grid.column_count,
                )
            });
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            table_width.content_width.points(),
            !table_root_inline_size(style).is_auto(),
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry.as_ref(),
        );

        let mut total = style.margin.top;
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width.points(),
            CaptionSide::Top,
        );
        let table_used_style = self.table_used_style(style);
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: &table_used_style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics: table_metrics.clone(),
            collapsed_geometry: collapsed_geometry.as_ref(),
            wrapper_border_box_block_size: None,
            wrapper_non_grid_block_size: layout_pt(0.0),
        };
        let table_height_plan = self.table_height_plan(&table_context);
        let row_heights = table_height_plan.final_row_heights();
        let row_occupancy = table_height_plan.row_occupancy();
        total += table_content_height(&row_heights, &row_occupancy, table_metrics);
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width.points(),
            CaptionSide::Bottom,
        );
        total + style.margin.bottom
    }
}
