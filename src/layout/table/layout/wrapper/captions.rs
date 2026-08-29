use super::*;
/// Physical width available to a table caption's outer border box.
///
/// Captions are siblings of the table grid in the table wrapper, so their
/// auto-width resolution uses the wrapper border-box measure rather than the
/// grid content width. Keeping that distinction in the replay API prevents
/// an empty grid from silently dropping its wrapper padding and borders.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionOuterWidth(BorderBoxLength);

impl TableCaptionOuterWidth {
    pub(in crate::layout::table) fn from_border_box(width: BorderBoxLength) -> Self {
        Self(width)
    }

    pub(in crate::layout::table) fn points(self) -> f32 {
        self.0.points()
    }
}

/// The table-wrapper frame projected into the legacy caption block-layout
/// boundary.
///
/// Captions are wrapper siblings, not table-grid children.  This composite
/// keeps their physical containing span and the table root's logical axes
/// together so a caller cannot pass an unrelated grid X coordinate to a
/// vertical caption layout entry point.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionContainingBlock {
    physical_span: PageInlineSpan,
    outer_width: TableCaptionOuterWidth,
    axes: TableAxes,
    wrapper_table_x: PageInlinePosition,
}

impl TableCaptionContainingBlock {
    pub(in crate::layout::table) fn new(
        physical_span: PageInlineSpan,
        outer_width: TableCaptionOuterWidth,
        axes: TableAxes,
        wrapper_table_x: PageInlinePosition,
    ) -> Self {
        Self {
            physical_span,
            outer_width,
            axes,
            wrapper_table_x,
        }
    }

    pub(in crate::layout::table) fn physical_span(self) -> PageInlineSpan {
        self.physical_span
    }

    pub(in crate::layout::table) fn outer_width(self) -> TableCaptionOuterWidth {
        self.outer_width
    }

    pub(in crate::layout::table) fn axes(self) -> TableAxes {
        self.axes
    }

    pub(in crate::layout::table) fn wrapper_table_x(self) -> PageInlinePosition {
        self.wrapper_table_x
    }

    /// Return the physical span which the legacy generic block entry may use
    /// as its horizontal containing-block coordinate.
    ///
    /// A horizontal table wrapper owns that span directly. A vertical table
    /// instead fragments along physical X, so replacing the active
    /// fragmentainer span with the wrapper's complete block extent would
    /// silently make a split caption fit in one column. The caller must keep
    /// the active fragmentainer bounds in that case; the table wrapper's
    /// logical inline measure is resolved independently by the caption style.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn legacy_horizontal_span(self) -> Option<PageInlineSpan> {
        (!self.axes.flow.writing_mode().has_vertical_lines()).then_some(self.physical_span)
    }
}

/// The committed destination state of a table-caption layout pass.
///
/// Generic block layout remains responsible for laying out caption contents,
/// but a table wrapper owns the transition which follows it.
/// `final_destination` is the authoritative post-caption destination,
/// including its remaining logical block capacity. Returning it prevents the
/// wrapper from synthesizing its grid start later from a stale `table_x` and
/// a cursor that belongs to an earlier fragmentainer.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCaptionLayoutOutcome {
    final_destination: TableFragmentainerPlacement,
    /// Retained source-local slices for caption paint.  They deliberately use
    /// wrapper-local intervals rather than table-grid offsets.
    caption_paint_slices: Vec<TableCaptionPaintSlice>,
    consumed_wrapper_interval: TableWrapperBlockInterval,
    /// The final caption exactly consumed its destination block track.  The
    /// following wrapper part must select a successor rather than inheriting
    /// an exhausted zero-width track.
    next_part_requires_successor: bool,
}

/// One retained caption slice in caption-local source coordinates.
///
/// The parent multicolumn formatter sees only the completed temporary
/// fragment to which this slice was appended.  This record remains table
/// local, preventing parent replay from interpreting caption progress as
/// table-grid source geometry.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionPaintSlice {
    pub(in crate::layout::table) page_index: usize,
    pub(in crate::layout::table) source_block_start: LayoutLength,
    pub(in crate::layout::table) block_size: LayoutLength,
    /// Table-wrapper destination selected while this source interval was
    /// consumed.  Parent multicolumn replay never reads this table-local
    /// record; it is solely the wrapper ledger's placement contract.
    pub(in crate::layout::table) destination: TableFragmentainerPlacement,
    pub(in crate::layout::table) destination_context: PageContext,
    pub(in crate::layout::table) destination_origin: PageTopPoint,
    pub(in crate::layout::table) destination_extent: LogicalSize,
    pub(in crate::layout::table) destination_block_start: LayoutLength,
}

impl TableCaptionLayoutOutcome {
    pub(in crate::layout::table) fn new(
        final_destination: TableFragmentainerPlacement,
        caption_paint_slices: Vec<TableCaptionPaintSlice>,
        consumed_wrapper_interval: TableWrapperBlockInterval,
        next_part_requires_successor: bool,
    ) -> Self {
        Self {
            final_destination,
            caption_paint_slices,
            consumed_wrapper_interval,
            next_part_requires_successor,
        }
    }

    pub(in crate::layout::table) fn final_destination(&self) -> TableFragmentainerPlacement {
        self.final_destination
    }

    pub(in crate::layout::table) fn caption_paint_slices(&self) -> &[TableCaptionPaintSlice] {
        &self.caption_paint_slices
    }

    pub(in crate::layout::table) fn consumed_wrapper_interval(&self) -> TableWrapperBlockInterval {
        self.consumed_wrapper_interval
    }

    /// Whether the next wrapper-flow part needs a fresh fragmentainer.
    pub(in crate::layout::table) fn next_part_requires_successor(&self) -> bool {
        self.next_part_requires_successor
    }
}

// Caption layout belongs to the table wrapper rather than cell flow.
/// Result of assigning one continuous vertical caption box to table-wrapper
/// fragmentainers. The final-block-boundary flag is kept separate from the
/// paint slices: the next wrapper part, rather than caption layout itself,
/// decides whether an empty successor must be materialized.
struct VerticalTableCaptionConsumption {
    /// The table wrapper's authoritative continuation after this caption's
    /// final slice.  It carries both the destination origin and the remaining
    /// logical block track; callers must not rebuild either from a restored
    /// generic-caption containing block.
    post_caption_destination: TableFragmentainerPlacement,
    ends_at_fragmentainer_block_end: bool,
    paint_slices: Vec<TableCaptionPaintSlice>,
}

impl<'a> LayoutBuilder<'a> {
    /// Advance table-wrapper flow to the next destination fragmentainer.
    ///
    /// Captions, table chrome, and rows share this operation at their wrapper
    /// boundary.  It deliberately returns a `TableFragmentainerPlacement`,
    /// rather than exposing a raw content track, so an exhausted vertical
    /// caption cannot be handed to the grid as a zero-width destination.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn advance_table_wrapper_fragmentainer(
        &mut self,
        table_style: &ComputedStyle,
        containing_block: TableCaptionContainingBlock,
    ) -> Option<TableFragmentainerPlacement> {
        self.materialize_table_fragmentainer_advance(
            self.active_fragmentainer_kind(),
            FragmentainerAdvance::Unforced,
        )?;
        // This table-local transition selects the next temporary parent
        // fragment, but it does not select a physical parent multicolumn
        // destination.  The parent formatter performs that replay once.  In
        // particular, a vertical caption's block direction cannot move the
        // following grid to a different parent column.
        let grid_origin =
            PageTopPoint::new(containing_block.wrapper_table_x().points(), self.cursor_y);
        Some(self.table_fragmentainer_placement(
            table_style,
            grid_origin.x(),
            containing_block.wrapper_table_x(),
            grid_origin.top_y(),
        ))
    }
    pub(in crate::layout::table) fn layout_table_captions(
        &mut self,
        captions: &[TableCaption<'_>],
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        containing_block: TableCaptionContainingBlock,
        side: CaptionSide,
    ) -> TableCaptionLayoutOutcome {
        let table_width = containing_block.outer_width().points();
        let table_span = containing_block.physical_span();
        debug_assert_eq!(
            containing_block.axes().flow.writing_mode(),
            table_style.writing_mode,
            "caption containing block must retain its table-root axes"
        );
        if std::env::var_os("QUIRE_TRACE_TABLE_CAPTION").is_some() {
            eprintln!(
                "table caption container: side={side:?} input_x={} input_width={table_width} parent=({}, {}) cursor={}",
                table_span.left_x(),
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
        }
        let opening_content_left = self.content_left;
        let mut final_fragmentainer_left = self.content_left;
        // A vertical table wrapper advances along physical X.  Keep this
        // typed destination while the wrapper track still reflects the
        // consumed caption, rather than synthesizing one after generic
        // caption layout restores its temporary containing block.
        let mut post_caption_destination = None;
        let mut vertical_block_progress = TableGridLength::new(0.0);
        let mut ends_at_fragmentainer_block_end = false;
        let mut caption_paint_slices = Vec::new();
        for caption in captions {
            let mut caption_style = self.style_for_table_caption(caption, table_style, stylesheets);
            if caption_style.caption_side != side || caption_style.display.is_none() {
                continue;
            }
            let caption_available_width = if has_auto_width(&caption_style) {
                // An auto-width caption uses the table measure for its outer
                // border box. `width` itself sizes the content box, so remove
                // the caption's padding and borders before freezing that used
                // value; otherwise thick caption borders spuriously widen the
                // table wrapper.
                // <https://www.w3.org/TR/CSS22/tables.html#model>
                let horizontal_non_content = caption_style.padding.left
                    + caption_style.padding.right
                    + horizontal_border_width(&caption_style);
                set_style_used_width(
                    &mut caption_style,
                    (table_width - horizontal_non_content).max(0.0),
                );
                if used_property_containment(caption.element, &caption_style).size
                    && caption_style.writing_mode == WritingMode::HorizontalTb
                {
                    // Size containment fixes the caption's principal used
                    // size independently of its descendants. Those
                    // descendants still format as visual overflow, anchored
                    // at the principal border edge rather than after a
                    // zero-extent side border. Preserve the same outer table
                    // measure by transferring that start inset from the
                    // internal overflow origin to the used content width.
                    // <https://www.w3.org/TR/css-contain-1/#containment-size>
                    let start_border = used_border_widths(&caption_style).left;
                    if start_border != 0.0 {
                        // The principal block has zero used block extent, so
                        // this side has no visible block-axis edge. Removing
                        // it from descendant layout exposes the border-box
                        // overflow origin without changing the painted top
                        // and bottom edges.
                        // `style_with_current_used_lengths` resolves the
                        // durable border length values again before block
                        // geometry is built. Keep that source value in sync
                        // with this temporary used-edge adjustment instead of
                        // letting late font-metric resolution restore the
                        // authored left border for descendant overflow.
                        caption_style.border_width_values.left =
                            css::ComputedLengthPercentage::from_points(0.0);
                        caption_style.border_widths.left = 0.0;
                        set_style_used_width(
                            &mut caption_style,
                            (table_width - horizontal_non_content + start_border).max(0.0),
                        );
                    }
                }
                table_width
            } else {
                let horizontal_non_content = caption_style.padding.left
                    + caption_style.padding.right
                    + horizontal_border_width(&caption_style);
                let caption_content_width = used_content_box_width_or_auto(
                    &caption_style,
                    layout_pt(table_width),
                    non_content_pt(horizontal_non_content),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or(table_width);
                table_width.max(
                    caption_style.margin.left
                        + caption_content_width
                        + horizontal_non_content
                        + caption_style.margin.right,
                )
            };
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            if std::env::var_os("QUIRE_TRACE_TABLE_CAPTION").is_some() {
                eprintln!(
                    " caption: style={:?}/{:?} available_width={caption_available_width}",
                    caption_style.writing_mode, caption_style.caption_side,
                );
            }
            if let Some(horizontal_span) = containing_block.legacy_horizontal_span() {
                self.content_left = horizontal_span.left_x();
                self.content_right = horizontal_span.right_x();
            }
            self.push_float_context();
            let caption_inline_block_start = PageTopBlockPosition::new(self.cursor_y);
            // The caption's inherited writing mode is the formatting root
            // that establishes its logical block extent.  The wrapper axis
            // is retained for destination projection below, but using it to
            // classify generic caption content loses inherited vertical
            // writing modes at the anonymous table-wrapper boundary.
            let vertical_caption = caption_style.writing_mode.has_vertical_lines();
            // A caption is a wrapper-flow sibling, not an independently
            // paginated physical block.  For a vertical table, first lay out
            // one continuous source subtree and let the table wrapper assign
            // its logical block slices to the same fragmentainer sequence as
            // the row grid.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            let caption_paint_checkpoint = self.current_page.paint_checkpoint();
            if vertical_caption {
                self.fragmentation_suppression_depth += 1;
            }
            // The table-part adapter has already applied the caption's
            // effective zoom. Its generic block replay consumes this value
            // only as used geometry, so prevent that nested entry from
            // scaling the same fixed lengths again.
            caption_style.effective_zoom = css::EffectiveZoom::NORMAL;
            if let Some(children) = caption.children.as_deref() {
                self.layout_element_box(
                    caption.element,
                    &caption_style,
                    stylesheets,
                    caption.signature.clone(),
                    &box_tree::BoxSource::Principal,
                    &[],
                    children,
                );
            } else {
                self.layout_element(caption.element, &caption_style, stylesheets);
            }
            if !vertical_caption {
                // Generic block layout selected the active destination for a
                // horizontal caption.  Retain that typed track while it is
                // current, before restoring the caller's temporary
                // containing block for the next wrapper sibling.
                final_fragmentainer_left = self.content_left;
            }
            if vertical_caption {
                self.fragmentation_suppression_depth -= 1;
                let caption_block_size = layout_pt(
                    self.last_block_layout_outcome
                        .physical_border_box_inline_span
                        .points()
                        .max(0.0),
                );
                // Keep caption paint in its own wrapper-source coordinate
                // space.  Captions are not table-grid source paint, but they
                // must still be replayed through the same committed parent
                // multicol projections as the following grid.  Projecting
                // them eagerly through root pages used a second destination
                // sequence and made a vertical caption reappear in the wrong
                // anonymous columns.
                let caption_source = if let Some(source_border_rect) =
                    self.last_block_layout_outcome.static_border_box
                {
                    let source = self
                        .current_page
                        .take_paint_fragment_since(caption_paint_checkpoint.clone());
                    let source_bounds = PaintClip::from_paint_rect(source_border_rect);
                    let caption_policy = StackingContextPolicy::for_atomic(
                        &caption_style,
                        PaintBand::InFlowBlock,
                        source_bounds,
                    );
                    self.scope_current_page_fragment_with_policy(
                        &caption_paint_checkpoint,
                        caption_policy,
                        source_bounds,
                        source,
                        Vec::new(),
                    );
                    Some((
                        self.current_page
                            .take_paint_fragment_since(caption_paint_checkpoint.clone()),
                        PageTopPoint::new(
                            source_border_rect.origin.x,
                            source_border_rect.origin.y + source_border_rect.size.height,
                        ),
                        LogicalSize {
                            inline: source_border_rect.size.height,
                            block: source_border_rect.size.width,
                        },
                    ))
                } else {
                    None
                };
                // Generic layout temporarily uses the caption's source box as
                // its physical X containing block. Restore the active table
                // fragmentainer track before consuming wrapper progress.
                self.content_left = previous_left;
                self.content_right = previous_right;
                let consumption = self.consume_vertical_table_caption_block_size(
                    FlowAxes::for_style(&caption_style),
                    caption_block_size,
                    caption_source
                        .as_ref()
                        .map(|(_, _, source_extent)| source_extent.inline)
                        .unwrap_or(0.0),
                    table_style,
                    containing_block,
                );
                if let Some(consumption) = consumption {
                    ends_at_fragmentainer_block_end = consumption.ends_at_fragmentainer_block_end;
                    post_caption_destination = Some(consumption.post_caption_destination);
                    caption_paint_slices.extend(consumption.paint_slices.iter().map(|slice| {
                        let mut slice = *slice;
                        slice.source_block_start = layout_pt(
                            slice.source_block_start.points() + vertical_block_progress.get(),
                        );
                        slice
                    }));
                    if let Some((source, source_origin, source_extent)) = caption_source {
                        let projected = self.project_vertical_table_caption_paint(
                            source,
                            &consumption.paint_slices,
                            FlowAxes::for_style(&caption_style),
                            source_origin,
                            source_extent,
                        );
                        let first_fragment_inline_offset = (caption_inline_block_start.points()
                            - self.current_page_context.top())
                        .abs();
                        let first_page_index = consumption
                            .paint_slices
                            .first()
                            .map(|slice| slice.page_index);
                        let first_fragment_translation = if first_fragment_inline_offset > 0.01 {
                            match FlowAxes::for_style(&caption_style).inline_start_side() {
                                PhysicalSide::Top => {
                                    PaintTranslation::new(0.0, -first_fragment_inline_offset)
                                }
                                PhysicalSide::Bottom => {
                                    PaintTranslation::new(0.0, first_fragment_inline_offset)
                                }
                                PhysicalSide::Left | PhysicalSide::Right => unreachable!(
                                    "vertical caption projection has a vertical logical inline axis"
                                ),
                            }
                        } else {
                            PaintTranslation::identity()
                        };
                        for (page_index, fragment) in projected {
                            let fragment = if Some(page_index) == first_page_index {
                                fragment.translated(first_fragment_translation)
                            } else {
                                fragment
                            };
                            if page_index < self.pages.len() {
                                self.pages[page_index].append_paint_fragment_owned(
                                    fragment,
                                    PaintTranslation::identity(),
                                );
                            } else {
                                self.current_page.append_paint_fragment_owned(
                                    fragment,
                                    PaintTranslation::identity(),
                                );
                            }
                        }
                    }
                }
                vertical_block_progress = TableGridLength::new(
                    vertical_block_progress.get()
                        + self
                            .last_block_layout_outcome
                            .physical_border_box_inline_span
                            .points()
                            .max(0.0),
                );
                final_fragmentainer_left = self.content_left;
            }
            self.pop_float_context();
            // The generic caption entry temporarily owns its containing
            // bounds.  A vertical table wrapper then replaces those bounds
            // with the post-caption fragmentainer track in
            // `consume_vertical_table_caption_block_size`.  Preserve that
            // track for the following caption and the table grid; restoring
            // `previous_*` here would make both start before the caption.
            // A vertical caption in a horizontal table remains generic
            // caption content, so it retains the existing restoration.
            if !table_style.writing_mode.has_vertical_lines() || !vertical_caption {
                self.content_left = previous_left;
                self.content_right = previous_right;
            }
        }
        let wrapper_translation = final_fragmentainer_left - opening_content_left;
        let final_wrapper_table_x = if table_style.writing_mode.has_vertical_lines() {
            // A vertical caption's local block slices may consume temporary
            // parent tracks, but those tracks are not the table grid's local
            // inline coordinate. The enclosing multicolumn formatter owns
            // their final physical replay; leaking `content_left` here moves
            // the grid away from its immutable source frame.
            containing_block.wrapper_table_x()
        } else {
            PageInlinePosition::new(
                containing_block.wrapper_table_x().points() + wrapper_translation,
            )
        };
        let grid_origin = PageTopPoint::new(final_wrapper_table_x.points(), self.cursor_y);
        // For a vertical root this is the exact placement captured after the
        // final caption slice consumed its destination track.  Horizontal
        // caption layout retains its longstanding physical-cursor contract.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        let final_destination = if table_style.writing_mode.has_vertical_lines() {
            post_caption_destination.unwrap_or_else(|| {
                self.table_fragmentainer_placement(
                    table_style,
                    grid_origin.x(),
                    final_wrapper_table_x,
                    grid_origin.top_y(),
                )
            })
        } else {
            self.table_fragmentainer_placement(
                table_style,
                grid_origin.x(),
                final_wrapper_table_x,
                grid_origin.top_y(),
            )
        };
        TableCaptionLayoutOutcome::new(
            final_destination,
            caption_paint_slices,
            TableWrapperBlockInterval::new(
                TableWrapperBlockOffset::zero(),
                vertical_block_progress,
            ),
            ends_at_fragmentainer_block_end,
        )
    }

    /// Consume a vertical table-caption's logical block extent through the
    /// table wrapper's active fragmentainer sequence.
    ///
    /// This is deliberately table-local: normal block layout owns caption
    /// content, while the table wrapper owns the class-A continuation that
    /// follows a caption and precedes a grid or another caption.  In
    /// particular, a multicolumn continuation must use the same materializer
    /// as table rows so its anonymous-column page is replayed in source order.
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>
    /// <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
    fn consume_vertical_table_caption_block_size(
        &mut self,
        axes: FlowAxes,
        block_size: LayoutLength,
        source_inline_extent: f32,
        table_style: &ComputedStyle,
        containing_block: TableCaptionContainingBlock,
    ) -> Option<VerticalTableCaptionConsumption> {
        let mut remaining = block_size.points().max(0.0);
        if remaining <= 0.01 {
            return None;
        }

        let block_start_side = axes.block_start_side();
        debug_assert!(matches!(
            block_start_side,
            PhysicalSide::Left | PhysicalSide::Right
        ));
        let initial_context = self.current_page_context;
        let inline_start_inset = self.content_left - initial_context.left();
        let inline_end_inset = initial_context.right() - self.content_right;
        let mut source_block_start = 0.0;
        let mut paint_slices = Vec::new();
        loop {
            let context = self.current_page_context;
            let available = (self.content_right - self.content_left).max(0.0);
            if available <= 0.01 {
                break;
            }
            let used = remaining.min(available);
            paint_slices.push(TableCaptionPaintSlice {
                page_index: self.pages.len(),
                source_block_start: layout_pt(source_block_start),
                block_size: layout_pt(used),
                destination: self.table_fragmentainer_placement(
                    table_style,
                    containing_block.wrapper_table_x().points(),
                    containing_block.wrapper_table_x(),
                    context.top(),
                ),
                destination_context: context,
                // This is a selected temporary parent fragment, not the
                // continuous caption source canvas.  Packing each caption
                // source slice at this fragmentainer's physical origin makes
                // the enclosing multicolumn replay apply its projection once
                // (the same contract used by generic vertical root blocks).
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: source_inline_extent,
                    block: available,
                },
                destination_block_start: layout_pt(0.0),
            });
            remaining -= used;
            source_block_start += used;
            match block_start_side {
                PhysicalSide::Left => {
                    self.content_left = (self.content_left + used).min(self.content_right);
                }
                PhysicalSide::Right => {
                    self.content_right = (self.content_right - used).max(self.content_left);
                }
                PhysicalSide::Top | PhysicalSide::Bottom => unreachable!(),
            }
            if remaining <= 0.01 {
                break;
            }
            self.materialize_table_fragmentainer_advance(
                self.active_fragmentainer_kind(),
                FragmentainerAdvance::Unforced,
            )?;
            self.content_left = self.current_page_context.left() + inline_start_inset;
            self.content_right =
                (self.current_page_context.right() - inline_end_inset).max(self.content_left);
        }
        let ends_at_fragmentainer_block_end =
            (self.content_right - self.content_left).abs() <= 0.01;
        // Capture this before the caller restores any temporary generic
        // caption bounds.  The grid is the next table-wrapper sibling and
        // must inherit this precise remaining logical block capacity.
        let post_caption_destination = self.table_fragmentainer_placement(
            table_style,
            containing_block.wrapper_table_x().points(),
            containing_block.wrapper_table_x(),
            self.current_page_context.top(),
        );
        Some(VerticalTableCaptionConsumption {
            post_caption_destination,
            ends_at_fragmentainer_block_end,
            paint_slices,
        })
    }

    /// Project retained caption paint through table-selected destinations.
    ///
    /// The parent multicolumn formatter receives the completed fragments as
    /// ordinary parent paint and therefore applies its own projection once.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    fn project_vertical_table_caption_paint(
        &self,
        source: PaintFragment,
        slices: &[TableCaptionPaintSlice],
        axes: FlowAxes,
        source_origin: PageTopPoint,
        source_extent: LogicalSize,
    ) -> Vec<(usize, PaintFragment)> {
        let parent_slices = slices
            .iter()
            .map(|slice| VerticalRootPageFragmentSlice {
                page_index: slice.page_index,
                source_block_start: slice.source_block_start,
                block_size: slice.block_size,
                destination_context: slice.destination_context,
                destination_origin: slice.destination_origin,
                destination_extent: slice.destination_extent,
                destination_block_start: slice.destination_block_start,
            })
            .collect::<Vec<_>>();
        self.project_vertical_root_fragment_paint(
            source,
            &parent_slices,
            axes,
            source_origin,
            source_extent,
        )
    }
}
