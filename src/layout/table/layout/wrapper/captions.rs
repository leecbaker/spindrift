use super::*;
/// Logical inline size available to a table caption's outer border box.
///
/// Captions are siblings of the table grid in the table wrapper, so their
/// auto-width resolution uses the wrapper border-box measure rather than the
/// grid content width. Keeping that distinction in the replay API prevents
/// an empty grid from silently dropping its wrapper padding and borders.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionOuterInlineSize(BorderBoxLength);

impl TableCaptionOuterInlineSize {
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
    outer_inline_size: TableCaptionOuterInlineSize,
    axes: TableAxes,
    wrapper_table_x: PageInlinePosition,
    /// Whether float avoidance moved the wrapper's margin-box inline origin
    /// away from the containing block's ordinary start edge. Only then may a
    /// caption-free vertical wrapper use the resolved wrapper X as its
    /// continuation origin; a table margin is not outer-fragmentainer
    /// progress.
    float_displaced_inline: bool,
    /// The enclosing fragmentainer selected for this wrapper sibling. This
    /// is deliberately not a `TableGridPlacement`: captions and the grid
    /// share outer continuation but not table-grid geometry.
    outer_fragmentainer: Option<TableOuterFragmentainerPlacement>,
}

impl TableCaptionContainingBlock {
    pub(in crate::layout::table) fn new(
        physical_span: PageInlineSpan,
        outer_inline_size: TableCaptionOuterInlineSize,
        axes: TableAxes,
        wrapper_table_x: PageInlinePosition,
        float_displaced_inline: bool,
        outer_fragmentainer: Option<TableOuterFragmentainerPlacement>,
    ) -> Self {
        Self {
            physical_span,
            outer_inline_size,
            axes,
            wrapper_table_x,
            float_displaced_inline,
            outer_fragmentainer,
        }
    }

    pub(in crate::layout::table) fn outer_inline_size(self) -> TableCaptionOuterInlineSize {
        self.outer_inline_size
    }

    pub(in crate::layout::table) fn axes(self) -> TableAxes {
        self.axes
    }

    pub(in crate::layout::table) fn wrapper_table_x(self) -> PageInlinePosition {
        self.wrapper_table_x
    }

    pub(in crate::layout::table) fn float_displaced_inline(self) -> bool {
        self.float_displaced_inline
    }

    /// Return the outer placement that owns the caption's continuation.
    pub(in crate::layout::table) fn outer_fragmentainer(
        self,
    ) -> Option<TableOuterFragmentainerPlacement> {
        self.outer_fragmentainer
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
struct TableCaptionConsumption {
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
        let table_width = containing_block.outer_inline_size().points();
        debug_assert_eq!(
            containing_block.axes().flow.writing_mode(),
            table_style.writing_mode,
            "caption containing block must retain its table-root axes"
        );
        let opening_content_left = self.content_left;
        // The wrapper's physical block-start origin has already been chosen
        // by normal-flow placement (including float avoidance).  The active
        // scratch column's `content_left` is merely the parent flow edge and
        // can still point at the float's occupied slab.  Starting a vertical
        // caption/grid continuation there loses the wrapper placement when
        // no caption is generated at all.
        // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
        // <https://www.w3.org/TR/css2/visuren.html#floats>
        let mut final_fragmentainer_left = if table_style.writing_mode.has_vertical_lines()
            && containing_block.float_displaced_inline()
        {
            containing_block.wrapper_table_x().points()
        } else {
            opening_content_left
        };
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
                set_style_used_logical_inline_size(
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
                        set_style_used_logical_inline_size(
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
            // Paged-media vertical roots own a physical-X page transition.
            // An anonymous multicolumn fragmentainer does not: its parent
            // retains the one source fragment and projects it to columns
            // during final replay. Materializing columns here would give the
            // caption a second, table-local column sequence.
            let caption_uses_page_fragmentation = vertical_caption
                && containing_block.outer_fragmentainer().is_none()
                && self.active_fragmentainer_kind() == FragmentainerKind::Page;
            // The table wrapper owns the caption → grid transition. In an
            // outer multicolumn context, allowing generic caption layout to
            // call `push_page` would materialize anonymous columns before
            // the wrapper records its source interval; the later table-grid
            // replay would then select a second sequence. Suppress generic
            // fragmentation for every vertical wrapper caption. Paged roots
            // are consumed explicitly below; multicol captions stay as one
            // source fragment for their enclosing sequence to replay once.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            let wrapper_owns_caption_fragmentation = vertical_caption;
            if wrapper_owns_caption_fragmentation {
                self.fragmentation_suppression_depth += 1;
            }
            // Captions resolve their inline size against the table wrapper,
            // not against the active column's physical X span. This is
            // observable in vertical writing, where that X span is the
            // wrapper's block axis.
            // <https://drafts.csswg.org/css-tables-3/#table-caption-box>
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            self.content_logical_inline_size_stack
                .push(caption_available_width);
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
            self.content_logical_inline_size_stack
                .pop()
                .expect("caption inline-size basis must be balanced");
            if !vertical_caption {
                // Generic block layout selected the active destination for a
                // horizontal caption.  Retain that typed track while it is
                // current, before restoring the caller's temporary
                // containing block for the next wrapper sibling.
                final_fragmentainer_left = self.content_left;
            }
            if wrapper_owns_caption_fragmentation {
                self.fragmentation_suppression_depth -= 1;
            }
            if caption_uses_page_fragmentation {
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
                let consumption = self.consume_table_caption_block_size(
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
                        let projected = self.project_table_caption_paint(
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
            } else if vertical_caption {
                // Generic vertical caption layout records paint in its local
                // inline coordinate. The outer column fragmentainer owns the
                // destination sequence, but it still expects that source
                // paint to be rebased to the active fragmentainer's inline
                // origin before its one final projection.
                let inline_offset =
                    (caption_inline_block_start.points() - self.current_page_context.top()).abs();
                let inline_translation = if inline_offset > 0.01 {
                    match FlowAxes::for_style(&caption_style).inline_start_side() {
                        PhysicalSide::Top => PaintTranslation::new(0.0, -inline_offset),
                        PhysicalSide::Bottom => PaintTranslation::new(0.0, inline_offset),
                        PhysicalSide::Left | PhysicalSide::Right => unreachable!(
                            "vertical caption projection has a vertical logical inline axis"
                        ),
                    }
                } else {
                    PaintTranslation::identity()
                };
                // Generic caption layout starts its vertical-rl box at the
                // scratch column's physical left edge. A wrapper sibling's
                // logical block start is instead the outer placement's right
                // edge. Rebase the complete source caption to that edge once
                // before the enclosing multicolumn formatter performs its
                // own source-to-destination replay.
                // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                let table_source_rebase = match FlowAxes::for_style(table_style).block_start_side()
                {
                    PhysicalSide::Right => {
                        let source_border_box = self
                            .last_block_layout_outcome
                            .static_border_box
                            .expect("vertical caption layout must retain its source border box");
                        let x = containing_block
                            .outer_fragmentainer()
                            .map(|placement| {
                                let destination_block_end = if side == CaptionSide::Top {
                                    placement.destination_rect().x()
                                        + placement.destination_rect().width()
                                } else {
                                    containing_block.wrapper_table_x().points()
                                };
                                destination_block_end
                                    - (source_border_box.origin.x + source_border_box.size.width)
                            })
                            .unwrap_or(0.0);
                        PaintTranslation::new(x, 0.0)
                    }
                    PhysicalSide::Left => PaintTranslation::identity(),
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("vertical table wrappers have a horizontal logical block axis")
                    }
                };
                let translation = PaintTranslation::new(
                    inline_translation.x + table_source_rebase.x,
                    inline_translation.y + table_source_rebase.y,
                );
                if translation != PaintTranslation::identity() {
                    let source = self
                        .current_page
                        .take_paint_fragment_since(caption_paint_checkpoint.clone());
                    self.current_page
                        .append_paint_fragment_owned(source, translation);
                }
                // The outer multicolumn formatter keeps this as one source
                // fragment, but the following wrapper sibling still starts
                // after the caption in the table root's continuous block
                // coordinate system.
                vertical_block_progress = TableGridLength::new(
                    vertical_block_progress.get()
                        + self
                            .last_block_layout_outcome
                            .physical_border_box_inline_span
                            .points()
                            .max(0.0),
                );
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
            if !table_style.writing_mode.has_vertical_lines()
                || !vertical_caption
                || !caption_uses_page_fragmentation
            {
                self.content_left = previous_left;
                self.content_right = previous_right;
            }
        }
        let wrapper_translation = final_fragmentainer_left - opening_content_left;
        let final_wrapper_table_x = if table_style.writing_mode.has_vertical_lines() {
            // The grid is the next wrapper-flow sibling. It therefore begins
            // in the caption's committed destination track, not at the
            // wrapper's opening source coordinate. The enclosing multicolumn
            // replay subsequently projects that selected temporary track
            // once; retaining the opening X here restarts the grid in the
            // columns already consumed by the caption.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://drafts.csswg.org/css-tables-3/#table-root>
            let progress = vertical_block_progress.get();
            // `wrapper_table_x` is the grid's established physical source
            // origin.  A caption can select a later *outer* fragmentainer,
            // whose displacement is the delta from the opening parent
            // content edge; it must not replace the source origin itself.
            // Replacing it made every unfragmented vertical table start at
            // the parent edge and dropped the table border-spacing/padding
            // inset before cell layout.
            // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
            // <https://drafts.csswg.org/css-tables-3/#table-root>
            let destination_wrapper_x =
                containing_block.wrapper_table_x().points() + wrapper_translation;
            let x = match FlowAxes::for_style(table_style).block_start_side() {
                PhysicalSide::Left => destination_wrapper_x + progress,
                PhysicalSide::Right => destination_wrapper_x - progress,
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical table wrapper has a horizontal block axis")
                }
            };
            PageInlinePosition::new(x)
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
                let destination = self.table_fragmentainer_placement(
                    table_style,
                    grid_origin.x(),
                    final_wrapper_table_x,
                    grid_origin.top_y(),
                );
                // Generic caption layout is intentionally kept on one
                // source canvas in an outer multicolumn context. Its next
                // table sibling must nevertheless carry the ordinal reached
                // by that caption's complete logical block span; selecting
                // `pages.len()` here only observes the scratch page used to
                // measure the caption and restarts the grid in an earlier
                // outer column.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                self.fragmentainer_override
                    .filter(|override_| override_.kind == FragmentainerKind::Column)
                    .map(|override_| {
                        let current = override_.placement_for_fragmentainer(self.pages.len());
                        let placement = override_.sequence.placement_for_logical_block_position(
                            current.logical_block_start() + vertical_block_progress.get(),
                        );
                        destination.select_outer_fragmentainer(
                            TableOuterFragmentainerPlacement::from_outer(placement),
                        )
                    })
                    .unwrap_or(destination)
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

    /// Consume a table-caption's logical block extent through the
    /// table wrapper's active fragmentainer sequence.
    ///
    /// This is deliberately table-local: normal block layout owns caption
    /// content, while the table wrapper owns the class-A continuation that
    /// follows a caption and precedes a grid or another caption.  In
    /// particular, a multicolumn continuation must use the same materializer
    /// as table rows so its anonymous-column page is replayed in source order.
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>
    /// <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
    fn consume_table_caption_block_size(
        &mut self,
        axes: FlowAxes,
        block_size: LayoutLength,
        source_inline_extent: f32,
        table_style: &ComputedStyle,
        containing_block: TableCaptionContainingBlock,
    ) -> Option<TableCaptionConsumption> {
        let mut remaining = block_size.points().max(0.0);
        if remaining <= 0.01 {
            return None;
        }

        let block_start_side = axes.block_start_side();
        let initial_context = self.current_page_context;
        let inline_start_inset = self.content_left - initial_context.left();
        let inline_end_inset = initial_context.right() - self.content_right;
        let mut source_block_start = 0.0;
        let mut paint_slices = Vec::new();
        loop {
            let context = self.current_page_context;
            let available = match block_start_side {
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    (self.cursor_y - self.page_bottom()).max(0.0)
                }
                PhysicalSide::Left | PhysicalSide::Right => {
                    (self.content_right - self.content_left).max(0.0)
                }
            };
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
                    self.cursor_y,
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
                PhysicalSide::Top | PhysicalSide::Bottom => self.cursor_y -= used,
            }
            if remaining <= 0.01 {
                break;
            }
            self.materialize_table_fragmentainer_advance(
                self.active_fragmentainer_kind(),
                FragmentainerAdvance::Unforced,
            )?;
            if matches!(block_start_side, PhysicalSide::Left | PhysicalSide::Right) {
                self.content_left = self.current_page_context.left() + inline_start_inset;
                self.content_right =
                    (self.current_page_context.right() - inline_end_inset).max(self.content_left);
            }
        }
        let ends_at_fragmentainer_block_end = match block_start_side {
            PhysicalSide::Top | PhysicalSide::Bottom => {
                (self.cursor_y - self.page_bottom()).abs() <= 0.01
            }
            PhysicalSide::Left | PhysicalSide::Right => {
                (self.content_right - self.content_left).abs() <= 0.01
            }
        };
        // Capture this before the caller restores any temporary generic
        // caption bounds.  The grid is the next table-wrapper sibling and
        // must inherit this precise remaining logical block capacity.
        let post_caption_destination = self.table_fragmentainer_placement(
            table_style,
            containing_block.wrapper_table_x().points(),
            containing_block.wrapper_table_x(),
            self.current_page_context.top(),
        );
        Some(TableCaptionConsumption {
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
    fn project_table_caption_paint(
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
