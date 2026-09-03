use super::*;
use crate::units::LayoutSize;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn resolve_font_metric_lengths_in_page_box(
        &mut self,
        page_box: &mut box_tree::MutablePageBox<'_>,
        _parent_style: &ComputedStyle,
    ) {
        // The document root has no element parent. Its font-relative
        // `font-size` terms must therefore use the CSS initial values, not
        // Spindrift's outer rendering defaults. This mirrors the root cascade
        // base and keeps `rem` in a root `font-size` anchored to the initial
        // font size:
        // <https://www.w3.org/TR/css-cascade-5/#root-element> and
        // <https://www.w3.org/TR/css-values-4/#em>.
        let document_root_parent = ComputedStyle::initial();
        let parent_ch_advance = self.ch_advance_for_style(
            &document_root_parent,
            page_box.children.iter().any(|child| {
                self.box_requires_parent_ch_advance(child, document_root_parent.font_size)
            }),
        );
        self.root_metrics_require_selected_font = page_box
            .children
            .iter()
            .any(Self::box_requires_root_font_metrics);
        let mut root_metrics = self.root_metric_state;
        let document_root_parent_metrics = css::FontRelativeLengthBasis::new(
            layout_pt(document_root_parent.font_size),
            parent_ch_advance,
        );
        for child in &mut page_box.children {
            self.resolve_deferred_font_metrics_in_box(
                child,
                document_root_parent_metrics,
                &mut root_metrics,
            );
        }
        self.root_metric_state = root_metrics;
    }

    fn resolve_deferred_font_metrics_in_style(
        &mut self,
        style: &mut ComputedStyle,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) -> css::FontRelativeLengthBasis {
        let establishes_root_metrics = matches!(*root_metrics, RootMetricState::Bootstrapping);
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            parent_metrics,
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            root_metrics.font_size_basis(),
        );
        style
            .line_height_value
            .resolve_em_relative_lengths(layout_pt(style.font_size));
        let (line_height, _, _) = style.line_height_value.clone().projected(style.font_size);
        style.line_height = line_height;
        style.root_font_size = root_metrics
            .font_size_basis()
            .map_or(style.font_size, |basis| basis.font_size.points());
        style.finalize_computed_font_relative_lengths();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            (establishes_root_metrics && self.root_metrics_require_selected_font)
                || style.requires_ch_advance()
                || pseudo_requires_parent_ch,
        );
        // A selected-font metric lookup interns that font in the document.
        // Do not perform one for an otherwise metric-free style: an empty
        // block with the initial `normal` line-height must not retain a font.
        // The existing metric-dependency traversal covers every `ch`-based
        // term, and selected-font metric expressions share that used-value
        // resolution path.
        let requires_selected_font_metrics = (establishes_root_metrics
            && self.root_metrics_require_selected_font)
            || style.requires_selected_font_metrics();
        let ic_advance = if requires_selected_font_metrics {
            self.font_system.ic_advance_for_style(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        };
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_selected_font_metric_lengths(css::SelectedFontMetricLengthBasis::new(
            ch_advance,
            ic_advance,
            layout_pt(x_height),
            layout_pt(cap_height),
        ));
        if let RootMetricState::Resolved(root_metrics) = self.root_metric_state {
            style.root_font_size = root_metrics.basis().font_size.points();
            style.resolve_root_font_metric_lengths(root_metrics.basis());
        }
        style.resolve_line_height_relative_lengths();
        if establishes_root_metrics {
            root_metrics.establish(ResolvedRootFontMetrics::measured_for_document_root(
                css::RootFontMetricLengthBasis {
                    font_size: layout_pt(style.font_size),
                    ch_advance,
                    x_height: layout_pt(x_height),
                    cap_height: layout_pt(cap_height),
                    ic_advance,
                    line_height: layout_pt(style.line_height),
                },
            ));
        }
        let root_font_metrics = root_metrics.resolved().basis();
        style.root_font_size = root_font_metrics.font_size.points();
        style.resolve_root_font_metric_lengths(root_font_metrics);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        let font_metrics =
            css::FontRelativeLengthBasis::new(layout_pt(style.font_size), ch_advance)
                .with_selected_font_metrics(layout_pt(x_height), layout_pt(cap_height), ic_advance)
                .with_line_height(layout_pt(style.line_height));
        if let Some(style) = &mut style.marker_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_deferred_font_metrics_in_style(style, font_metrics, root_metrics);
        }
        // All font-, root-font-, viewport-, and selected-font-metric terms
        // above are still in ordinary CSS units. Apply zoom only once they
        // have their concrete used-length values, so inherited text styles
        // and percentage bases retain their CSS semantics.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        font_metrics
    }

    pub(in crate::layout) fn ch_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        required: bool,
    ) -> LayoutLength {
        if required {
            self.font_system.ch_advance(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        }
    }

    fn box_requires_parent_ch_advance(
        &self,
        formatting_box: &box_tree::MutableFormattingBox<'_>,
        parent_font_size: f32,
    ) -> bool {
        formatting_box
            .style()
            .deferred_font_size
            .requires_parent_ch_advance(parent_font_size)
    }

    fn children_require_parent_ch_advance(
        &self,
        children: &[box_tree::MutableFormattingBox<'_>],
        parent_font_size: f32,
    ) -> bool {
        children
            .iter()
            .any(|child| self.box_requires_parent_ch_advance(child, parent_font_size))
    }

    fn children_require_parent_selected_font_metrics(
        &self,
        children: &[box_tree::MutableFormattingBox<'_>],
    ) -> bool {
        children
            .iter()
            .any(Self::box_requires_parent_selected_font_metrics)
    }

    fn box_requires_parent_selected_font_metrics(
        formatting_box: &box_tree::MutableFormattingBox<'_>,
    ) -> bool {
        let children_require = |children: &[box_tree::MutableFormattingBox<'_>]| {
            children
                .iter()
                .any(Self::box_requires_parent_selected_font_metrics)
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.run_in_children)
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                box_.style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.children)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Text(box_) => box_
                .style
                .deferred_font_size
                .requires_parent_selected_font_metrics(),
            box_tree::MutableFormattingBox::Table(box_) => {
                box_.core
                    .style
                    .deferred_font_size
                    .requires_parent_selected_font_metrics()
                    || children_require(&box_.core.children)
            }
        }
    }

    fn selected_font_metric_basis_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> css::FontRelativeLengthBasis {
        let ch_advance = self.font_system.ch_advance(style);
        let x_height = self.font_system.used_x_height_for_style(style);
        let cap_height = self.font_system.used_cap_height_for_style(style);
        let ic_advance = self.font_system.ic_advance_for_style(style);
        css::FontRelativeLengthBasis::new(layout_pt(style.font_size), ch_advance)
            .with_selected_font_metrics(x_height, cap_height, ic_advance)
            .with_line_height(layout_pt(style.line_height))
    }

    /// Finds root-relative selected-font units before resolving the root
    /// style, so a metric-free document does not intern a font merely to
    /// create a fallback snapshot.
    fn box_requires_root_font_metrics(formatting_box: &box_tree::MutableFormattingBox<'_>) -> bool {
        let children_require = |children: &[box_tree::MutableFormattingBox<'_>]| {
            children.iter().any(Self::box_requires_root_font_metrics)
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.run_in_children)
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                box_.style.requires_root_font_metrics() || children_require(&box_.children)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
                    || box_
                        .table_fragment
                        .as_ref()
                        .is_some_and(Self::table_fragment_requires_root_font_metrics)
            }
            box_tree::MutableFormattingBox::Text(box_) => box_.style.requires_root_font_metrics(),
            box_tree::MutableFormattingBox::Table(box_) => {
                box_.core.style.requires_root_font_metrics()
                    || children_require(&box_.core.children)
                    || Self::table_fragment_requires_root_font_metrics(&box_.fragment)
            }
        }
    }

    fn table_fragment_requires_root_font_metrics(
        fragment: &box_tree::MutableTableFragment<'_>,
    ) -> bool {
        fragment.rows.iter().any(|row| {
            row.row_groups
                .iter()
                .filter_map(|group| group.style.as_deref())
                .any(ComputedStyle::requires_root_font_metrics)
                || row
                    .style
                    .as_deref()
                    .is_some_and(ComputedStyle::requires_root_font_metrics)
                || row.cells.iter().any(|cell| {
                    cell.style
                        .as_deref()
                        .is_some_and(ComputedStyle::requires_root_font_metrics)
                        || cell
                            .children
                            .iter()
                            .any(Self::box_requires_root_font_metrics)
                })
        }) || fragment.captions.iter().any(|caption| {
            caption
                .style
                .as_deref()
                .is_some_and(ComputedStyle::requires_root_font_metrics)
                || caption
                    .children
                    .iter()
                    .any(Self::box_requires_root_font_metrics)
        }) || fragment.columns.iter().any(|column| {
            column
                .group
                .as_ref()
                .and_then(|group| group.style.as_deref())
                .is_some_and(ComputedStyle::requires_root_font_metrics)
                || column
                    .style
                    .as_deref()
                    .is_some_and(ComputedStyle::requires_root_font_metrics)
        })
    }

    fn resolve_deferred_font_metrics_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) {
        let mut recurse = |builder: &mut Self,
                           children: &mut Vec<box_tree::MutableFormattingBox<'_>>,
                           style: &mut ComputedStyle| {
            let font_metrics =
                builder.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics);
            let font_size = font_metrics.font_size().points();
            let child_requires_selected_metrics =
                builder.children_require_parent_selected_font_metrics(children);
            let ch_advance = builder.ch_advance_for_style(
                style,
                child_requires_selected_metrics
                    || builder.children_require_parent_ch_advance(children, font_size),
            );
            let child_metrics = if child_requires_selected_metrics {
                builder.selected_font_metric_basis_for_style(style)
            } else {
                font_metrics.with_ch_advance(ch_advance)
            };
            for child in children {
                builder.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
            }
        };
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch = self
                    .children_require_parent_ch_advance(&box_.run_in_children, font_size)
                    || self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics = self
                    .children_require_parent_selected_font_metrics(&box_.run_in_children)
                    || self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                for child in &mut box_.run_in_children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                recurse(self, &mut box_.children, &mut box_.style)
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics =
                    self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_deferred_font_metrics_in_table_fragment(
                        fragment,
                        child_metrics,
                        root_metrics,
                    );
                }
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_deferred_font_metrics_in_style(
                    &mut box_.style,
                    parent_metrics,
                    root_metrics,
                );
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                let font_metrics = self.resolve_deferred_font_metrics_in_style(
                    &mut box_.core.style,
                    parent_metrics,
                    root_metrics,
                );
                let font_size = font_metrics.font_size().points();
                let child_requires_parent_ch =
                    self.children_require_parent_ch_advance(&box_.core.children, font_size);
                let child_requires_selected_metrics =
                    self.children_require_parent_selected_font_metrics(&box_.core.children);
                let ch_advance = self.ch_advance_for_style(
                    &box_.core.style,
                    child_requires_parent_ch || child_requires_selected_metrics,
                );
                let child_metrics = if child_requires_selected_metrics {
                    self.selected_font_metric_basis_for_style(&box_.core.style)
                } else {
                    font_metrics.with_ch_advance(ch_advance)
                };
                self.resolve_deferred_font_metrics_in_table_fragment(
                    &mut box_.fragment,
                    child_metrics,
                    root_metrics,
                );
                for child in &mut box_.core.children {
                    self.resolve_deferred_font_metrics_in_box(child, child_metrics, root_metrics);
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                recurse(self, &mut box_.core.children, &mut box_.core.style)
            }
        }
    }

    /// Resolve a table fragment in table-tree inheritance order.
    ///
    /// A row group is the parent of its rows, and a row is the parent of its
    /// cells, including the anonymous wrappers generated by the table fixup
    /// algorithm. Resolving an anonymous row before its row group would make
    /// `font-size: inherit` use the table's font instead of the row group's.
    /// <https://drafts.csswg.org/css-tables/#fixup-algorithm>
    fn resolve_deferred_font_metrics_in_table_fragment(
        &mut self,
        fragment: &mut box_tree::MutableTableFragment<'_>,
        parent_metrics: css::FontRelativeLengthBasis,
        root_metrics: &mut RootMetricState,
    ) {
        for row in &mut fragment.rows {
            let mut row_parent_metrics = parent_metrics;
            for group in &mut row.row_groups {
                if let Some(style) = &mut group.style {
                    row_parent_metrics = self.resolve_deferred_font_metrics_in_style(
                        style,
                        row_parent_metrics,
                        root_metrics,
                    );
                }
            }
            let row_metrics = row
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(
                        style,
                        row_parent_metrics,
                        root_metrics,
                    )
                })
                .unwrap_or(row_parent_metrics);
            for cell in &mut row.cells {
                let cell_metrics = cell
                    .style
                    .as_deref_mut()
                    .map(|style| {
                        self.resolve_deferred_font_metrics_in_style(
                            style,
                            row_metrics,
                            root_metrics,
                        )
                    })
                    .unwrap_or(row_metrics);
                for child in &mut cell.children {
                    self.resolve_deferred_font_metrics_in_box(child, cell_metrics, root_metrics);
                }
            }
        }
        for caption in &mut fragment.captions {
            let caption_metrics = caption
                .style
                .as_deref_mut()
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics)
                })
                .unwrap_or(parent_metrics);
            for child in &mut caption.children {
                self.resolve_deferred_font_metrics_in_box(child, caption_metrics, root_metrics);
            }
        }
        for column in &mut fragment.columns {
            let group_metrics = column
                .group
                .as_mut()
                .and_then(|group| group.style.as_deref_mut())
                .map(|style| {
                    self.resolve_deferred_font_metrics_in_style(style, parent_metrics, root_metrics)
                })
                .unwrap_or(parent_metrics);
            if let Some(style) = &mut column.style {
                self.resolve_deferred_font_metrics_in_style(style, group_metrics, root_metrics);
            }
        }
    }

    pub(in crate::layout) fn resolve_font_metric_lengths_in_box(
        &mut self,
        formatting_box: &mut box_tree::MutableFormattingBox<'_>,
    ) {
        match formatting_box {
            box_tree::MutableFormattingBox::Block(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.run_in_children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Inline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::InlineSplitBlockContext(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::AnonymousBlock(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::AtomicInline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                if let Some(fragment) = &mut box_.table_fragment {
                    self.resolve_font_metric_lengths_in_table_fragment(fragment);
                }
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Text(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
            }
            box_tree::MutableFormattingBox::Table(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                self.resolve_font_metric_lengths_in_table_fragment(&mut box_.fragment);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Flex(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::MutableFormattingBox::Replaced(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.core.style);
                for child in &mut box_.core.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
        }
    }

    pub(in crate::layout) fn resolve_font_metric_lengths_in_table_fragment(
        &mut self,
        fragment: &mut box_tree::MutableTableFragment<'_>,
    ) {
        for row in &mut fragment.rows {
            if let Some(style) = &mut row.style {
                self.resolve_style_font_metric_lengths(style);
            }
            for group in &mut row.row_groups {
                if let Some(style) = &mut group.style {
                    self.resolve_style_font_metric_lengths(style);
                }
            }
            for cell in &mut row.cells {
                if let Some(style) = &mut cell.style {
                    self.resolve_table_cell_style_font_metric_lengths(style);
                }
                for child in &mut cell.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
        }
        for caption in &mut fragment.captions {
            if let Some(style) = &mut caption.style {
                self.resolve_style_font_metric_lengths(style);
            }
            for child in &mut caption.children {
                self.resolve_font_metric_lengths_in_box(child);
            }
        }
        for column in &mut fragment.columns {
            if let Some(style) = &mut column.style {
                self.resolve_style_font_metric_lengths(style);
            }
            if let Some(group) = &mut column.group
                && let Some(style) = &mut group.style
            {
                self.resolve_style_font_metric_lengths(style);
            }
        }
    }

    pub(in crate::layout) fn build_frozen_child_boxes_with_font_metrics<'b>(
        &mut self,
        element: &'b Element,
        stylesheets: &Stylesheets<'_>,
        parent_style: &impl css::CascadedStyleSource,
        ancestors: &[ElementSignature],
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
        let parent_style = css::CascadedStyleSource::cascaded_style(parent_style);
        let mut child_boxes = box_tree::build_child_boxes_with_font_metrics(
            element,
            stylesheets,
            parent_style,
            ancestors,
            &mut self.font_system,
        );
        for child in &mut child_boxes {
            self.resolve_font_metric_lengths_in_box(child);
        }
        box_tree::freeze_child_boxes(child_boxes)
    }

    pub(in crate::layout) fn build_frozen_child_boxes_with_current_ancestors<'b>(
        &mut self,
        element: &'b Element,
        stylesheets: &Stylesheets<'_>,
        parent_style: &impl css::CascadedStyleSource,
    ) -> Vec<box_tree::FrozenFormattingBox<'b>> {
        let parent_style = css::CascadedStyleSource::cascaded_style(parent_style);
        let mut child_boxes = {
            let ancestors = &self.ancestors;
            let font_system = &mut self.font_system;
            box_tree::build_child_boxes_with_font_metrics(
                element,
                stylesheets,
                parent_style,
                ancestors,
                font_system,
            )
        };
        for child in &mut child_boxes {
            self.resolve_font_metric_lengths_in_box(child);
        }
        box_tree::freeze_child_boxes(child_boxes)
    }

    pub(in crate::layout) fn resolve_style_font_metric_lengths(
        &mut self,
        style: &mut ComputedStyle,
    ) {
        self.resolve_deferred_root_font_metric_font_size(style);
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            style.requires_ch_advance() || pseudo_requires_parent_ch,
        );
        // Styles created during layout (notably positioned descendants) pass
        // through this late resolution path. Resolve every selected-font box
        // metric here, not only `ch`, so their used sizes agree with styles
        // prepared during the structural font-metric pass.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        let requires_selected_font_metrics = style.requires_selected_font_metrics();
        let ic_advance = if requires_selected_font_metrics {
            self.font_system.ic_advance_for_style(style)
        } else {
            css::fallback_ch_advance_for_style(style)
        };
        let x_height = if requires_selected_font_metrics {
            self.font_system.used_x_height_for_style(style).points()
        } else {
            style.font_size * 0.5
        };
        let cap_height = if requires_selected_font_metrics {
            self.font_system.used_cap_height_for_style(style).points()
        } else {
            style.font_size * 0.7
        };
        style.resolve_selected_font_metric_lengths(css::SelectedFontMetricLengthBasis::new(
            ch_advance,
            ic_advance,
            layout_pt(x_height),
            layout_pt(cap_height),
        ));
        if let RootMetricState::Resolved(root_metrics) = self.root_metric_state {
            style.root_font_size = root_metrics.basis().font_size.points();
            style.resolve_root_font_metric_lengths(root_metrics.basis());
        }
        style.resolve_line_height_relative_lengths();
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        if let Some(style) = &mut style.marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
    }

    /// Correct a lazily built descendant's provisional `font-size` once the
    /// document root has established its used font-size and selected-font
    /// metric snapshot.
    ///
    /// CSS cascade intentionally retains a deferred font size while a box is
    /// being built. A child constructed after the structural prepass has not
    /// passed through that prepass, so root-relative terms must consume the
    /// typed snapshot retained by the builder instead of their provisional
    /// parent-sized fallback.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(in crate::layout) fn resolve_deferred_root_font_metric_font_size(
        &mut self,
        style: &mut ComputedStyle,
    ) {
        if !style.deferred_font_size.requires_root_font_metrics()
            && !style.deferred_font_size.requires_document_root_font_size()
        {
            return;
        }
        let RootMetricState::Resolved(root_metrics) = self.root_metric_state else {
            // The structural traversal has not yet reached the document root's
            // used font-size boundary. The bootstrap path intentionally uses
            // the CSS initial root fallback; the normal recursive pass will
            // revisit this descendant after establishing the snapshot.
            return;
        };
        let provisional_parent_font_size = style.font_size;
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            css::FontRelativeLengthBasis::new(
                layout_pt(provisional_parent_font_size),
                css::fallback_ch_advance_for_style(style),
            ),
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            Some(root_metrics.basis()),
        );
    }

    /// Resolves lazily cascaded `font-size` values that depend on the parent
    /// selected font. Structural traversal normally performs this work, but
    /// positioned and replayed boxes are constructed after that traversal.
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>
    pub(in crate::layout) fn resolve_deferred_parent_font_metric_font_size(
        &mut self,
        style: &mut ComputedStyle,
        parent_style: &ComputedStyle,
    ) {
        if !style
            .deferred_font_size
            .requires_parent_selected_font_metrics()
        {
            return;
        }
        let parent_metrics = self.selected_font_metric_basis_for_style(parent_style);
        style.resolve_deferred_font_size_with_viewport_and_root_metrics(
            parent_metrics,
            LayoutSize::new(
                self.initial_viewport_context.area_width(),
                self.current_page_context.area_height(),
            ),
            self.root_metric_state.font_size_basis(),
        );
    }

    pub(in crate::layout) fn resolve_table_cell_style_font_metric_lengths(
        &mut self,
        style: &mut ComputedStyle,
    ) {
        let box_edges_require_ch_advance = style.box_values.requires_ch_advance();
        let pseudo_requires_parent_ch = [
            style.marker_style.as_deref(),
            style.before_style.as_deref(),
            style.after_style.as_deref(),
            style.first_line_style.as_deref(),
            style.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(style.font_size)
        });
        let ch_advance = self.ch_advance_for_style(
            style,
            style.requires_ch_advance() || pseudo_requires_parent_ch,
        );
        style.resolve_font_metric_lengths_preserving_box_block_sizes(ch_advance);
        if box_edges_require_ch_advance {
            synchronize_resolved_fixed_box_edge_cache(style);
        }
        style.rebuild_own_text_decoration_origin();
        if let Some(style) = &mut style.marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_line_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.first_letter_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_call_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.footnote_marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
    }
}
