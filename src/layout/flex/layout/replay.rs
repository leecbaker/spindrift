use super::*;

pub(in crate::layout::flex) struct SplitFlexItemPaintContext {
    /// The used physical border-box dimensions from the flex algorithm.
    /// These must not be confused with the content-box percentage bases used
    /// when replaying descendants.
    pub(in crate::layout::flex) item_width: BorderBoxLength,
    pub(in crate::layout::flex) item_height: BorderBoxLength,
    pub(in crate::layout::flex) percentage_height_basis: FlexPercentageBasis,
    pub(in crate::layout::flex) slice_border_box: PaintClip,
    /// The committed flex-content span for this destination fragment. Visible
    /// descendant overflow is clipped here rather than at the item's own
    /// border box, which may be narrower than its relative/ink overflow.
    pub(in crate::layout::flex) fragment_content_clip: PaintClip,
    pub(in crate::layout::flex) source_item_top: PageTopBlockPosition,
    /// The committed visible physical source extent. The detached source
    /// canvas is tall enough for this range without changing the item's
    /// frozen used border box or percentage basis.
    pub(in crate::layout::flex) source_height: PhysicalContentHeight,
    /// Committed source range and item-local ordinal for this replay. This is
    /// produced by the materialized flex fragment plan, rather than guessed
    /// from a page-sized source offset during painting.
    pub(in crate::layout::flex) continuation: FlexItemContinuation,
    /// Selects the coordinate system from which this committed continuation
    /// replays its child formatting context.
    pub(in crate::layout::flex) replay_origin: FlexItemReplayOrigin,
    /// Descendant paint overflow extends the flex source interval without
    /// becoming a child formatting-context continuation. Such a child keeps
    /// one source paint tree while flex clips it across its own fragments.
    pub(in crate::layout::flex) has_descendant_source_overflow: bool,
    /// The flex container's containing block expressed in the target
    /// fragmentainer. Split-item replay translates its source painting to an
    /// off-page coordinate system, so this is remapped before descendants are
    /// laid out. CSS Positioned Layout keeps an absolute descendant attached
    /// to the same containing block even when its in-flow ancestor fragments:
    /// <https://www.w3.org/TR/css-position-3/#def-cb>.
    pub(in crate::layout::flex) positioning_containing_block: Option<ContainingBlock>,
    pub(in crate::layout::flex) establishes_fixed_containing_block: bool,
    /// Fragment-local clip for descendants whose containing block is the flex
    /// container instead of the split flex item.
    pub(in crate::layout::flex) positioned_descendant_clip: Option<PaintClip>,
}

impl SplitFlexItemPaintContext {
    /// Adapt the frozen source border-box extent to the nested replay
    /// formatting-context availability. Replay applies its frozen box metrics
    /// itself, so this is intentionally not a general box-model conversion.
    pub(in crate::layout::flex) fn available_width_for_replay(&self) -> PhysicalContentWidth {
        PhysicalContentWidth::new(content_box_pt(self.item_width.points()))
    }
}

fn record_table_replay_fragment_bottom(
    bottoms: &mut Vec<Option<f32>>,
    fragmentainer_index: usize,
    fragment: &PaintFragment,
) {
    let Some(bounds) = fragment.bounds() else {
        return;
    };
    if bottoms.len() <= fragmentainer_index {
        bottoms.resize(fragmentainer_index + 1, None);
    }
    bottoms[fragmentainer_index] = Some(
        bottoms[fragmentainer_index]
            .map(|bottom| bottom.min(bounds.y()))
            .unwrap_or_else(|| bounds.y()),
    );
}

/// Mutable continuation state owned by one split flex item.
///
/// The source fragment sequence, its local layout end, and the target-page
/// cursor it contributes are one replay transaction. Keeping them together
/// prevents a caller from advancing sibling placement without retaining the
/// matching source fragment.
pub(in crate::layout::flex) struct SplitFlexItemReplayState<'a> {
    /// The one continuous descendant source canvas for visible overflow. This
    /// remains absent for a child that owns ordinary fragmentation.
    pub(in crate::layout::flex) source_replay: &'a mut Option<ContinuousSourceReplay>,
    pub(in crate::layout::flex) fragments: &'a mut Vec<PaintFragment>,
    pub(in crate::layout::flex) local_block_ends: &'a mut Vec<Option<f32>>,
    pub(in crate::layout::flex) table_fragment_bottoms: &'a mut Vec<Option<f32>>,
    pub(in crate::layout::flex) destination_block_end: &'a mut Option<f32>,
}
impl<'a> LayoutBuilder<'a> {
    /// Replay a split flex item from its original item layout and clip the
    /// selected page-local slice.
    ///
    /// CSS Fragmentation slices the visual fragment but preserves the source
    /// box's internal layout for continuations:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(in crate::layout::flex) fn paint_split_flex_item_fragment(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        context: SplitFlexItemPaintContext,
        replay: SplitFlexItemReplayState<'_>,
    ) {
        let SplitFlexItemReplayState {
            source_replay: cached_source_replay,
            fragments: table_replay_fragments,
            local_block_ends: replay_fragment_local_block_ends,
            table_fragment_bottoms: table_replay_fragment_bottoms,
            destination_block_end: replay_destination_block_end,
        } = replay;
        let slice_border_box = context.slice_border_box;
        let replay_clip = if context.has_descendant_source_overflow {
            context.fragment_content_clip
        } else {
            slice_border_box
        };
        let source_item_top = context.source_item_top.points();
        let table_replay = child.style.display.is_table();
        let child_fragment_replay = !context.has_descendant_source_overflow;
        // Flex has already resolved the table wrapper's cross size for the
        // source item.  That frozen size remains the outer flex geometry, but
        // it is not a definite CSS block-size for the table's own row
        // pagination.  Replaying it as one would turn its fragmentable body
        // into a fixed-height table and suppress repeated header state on the
        // continuation.  Keep the resolved inline size while returning the
        // table's block size to `auto` inside this isolated child replay.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-tables-3/#computing-the-table-height>
        let mut table_replay_style = placed_style.clone();
        if child_fragment_replay {
            table_replay_style
                .box_values
                .height
                .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        }
        // The continuous canvas lays out the item's descendants from the
        // authored source style, with only its frozen inline percentage basis
        // applied. Reusing the placed style's frozen block height here turns
        // the source capture back into a page-local used box and discards the
        // visible overflow that selected the continuation in the first place.
        let mut continuous_source_style = child.style.clone();
        if !child_fragment_replay {
            set_style_used_width(
                &mut continuous_source_style,
                context.available_width_for_replay().points(),
            );
            continuous_source_style.box_sizing = BoxSizing::ContentBox;
        }
        let replay_style = if child_fragment_replay {
            &table_replay_style
        } else {
            &continuous_source_style
        };
        // The parent flex fragment has already committed the ordinary
        // item's used principal decoration at each materialized slice. This
        // source-canvas replay contributes only the item's formatting
        // context; allowing block layout to paint the same principal again
        // produces duplicate background edges on continuation pages.
        // Tables and replaced elements retain their dedicated decoration
        // replay paths.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_owns_principal_decoration = !table_replay
            && !child
                .element_parts()
                .is_some_and(|(element, _, _)| is_replaced_element(element));
        let replay_has_descendant_paint = child
            .element_parts()
            .is_none_or(|(_, _, children)| children.is_some_and(|children| !children.is_empty()));
        let table_first_capacity = context.continuation.first_fragmentainer_capacity.points();
        let table_continuation_capacity = context
            .continuation
            .continuation_fragmentainer_capacity
            .points();
        let source_canvas_slice_start = match context.replay_origin {
            // Visible descendant overflow belongs to one frozen item source
            // canvas. Every committed flex continuation must translate that
            // canvas by its own recorded source start, irrespective of the
            // flex main axis or whether the container wraps.
            FlexItemReplayOrigin::SourceSlice => {
                context.continuation.source_canvas_block_start.points()
            }
            // A child that fragments on its own is replayed from its
            // committed local fragment selected by continuation ordinal.
            FlexItemReplayOrigin::ChildFragment => 0.0,
        };
        if slice_border_box.height() <= 0.0 {
            return;
        }

        let table_fragment_ordinal = context.continuation.child_fragment_replay_ordinal();
        // A child formatter owns its own pagination decisions. Once its first
        // flex slice has committed those child fragments, later flex slices
        // must replay the matching child fragment instead of laying out the
        // complete child tree again against a new scratch page. Re-running a
        // table loses its consumed row position and can duplicate or omit
        // rows/header chrome.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if child_fragment_replay
            && let Some(fragment) = table_replay_fragments.get(table_fragment_ordinal)
        {
            let fragment = fragment
                .clone()
                .translated(PaintTranslation::new(
                    slice_border_box.x(),
                    // Each cached child fragment is already expressed in its
                    // own destination fragmentainer's page-local block
                    // coordinates. Reapplying the outer flex slice's block
                    // offset shifts the final partial fragment below its
                    // clip and drops nested overflow paint.
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    0.0,
                ))
                // A primitive clip would flatten nested stacking contexts and
                // lose effects such as descendant opacity. Keep the committed
                // child paint tree intact and apply the flex slice as an
                // overflow effect scope instead.
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                // <https://www.w3.org/TR/css-color-4/#transparency>
                .with_contents_effect_scoped_to_rect(replay_clip);
            if table_replay {
                record_table_replay_fragment_bottom(
                    table_replay_fragment_bottoms,
                    context.continuation.fragmentainer_index,
                    &fragment,
                );
            }
            if !table_replay
                && let Some(Some(local_block_end)) =
                    replay_fragment_local_block_ends.get(table_fragment_ordinal)
            {
                *replay_destination_block_end = Some(slice_border_box.y() + *local_block_end);
            }
            if !replay_owns_principal_decoration || replay_has_descendant_paint {
                self.append_split_flex_item_replay(placed_style, replay_clip, fragment);
            }
            return;
        }

        // Visible descendant overflow is not an independently fragmenting
        // child. Its first replay captures one continuous source canvas; all
        // later flex fragments only select a committed interval from that
        // artifact. In particular, a continuation must not lay the nested
        // size-contained wrapper out again against a fresh page-height box.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        if !child_fragment_replay && let Some(source_replay) = cached_source_replay.as_ref() {
            let source_slice = context.continuation.source_content_slice;
            if source_replay.source_height.points() >= source_slice.block_start.points() - 0.01
                && source_replay.source_height.points() <= source_slice.block_end.points() + 0.01
            {
                *replay_destination_block_end = Some(
                    slice_border_box.y() + slice_border_box.height()
                        - (source_replay.source_height.points()
                            - source_slice.block_start.points())
                        .max(0.0),
                );
            }
            let fragment = source_replay
                .paint
                .clone()
                .translated(source_slice_replay_translation(
                    slice_border_box,
                    source_item_top,
                    source_replay.scratch_top,
                    source_canvas_slice_start,
                ))
                .with_primitives_sliced_to_fragmentainer_rect_preserving_structure(replay_clip)
                .clipped_to_rect(replay_clip);
            if !replay_owns_principal_decoration || replay_has_descendant_paint {
                self.append_split_flex_item_replay(placed_style, replay_clip, fragment);
            }
            return;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let offpage_top = if child_fragment_replay {
            table_first_capacity.max(1.0)
        } else {
            context.source_height.points().max(1.0)
        };
        // A split-item replay uses a local off-page canvas. Its page context
        // must describe that same zero-inset coordinate system: retaining the
        // document page context causes a nested multicolumn replay to clip a
        // document-canvas inset and then apply that inset again on translation.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        let replay_page_context = PageContext {
            size: PageSize::from_points(context.item_width.points().max(1.0), offpage_top),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: snapshot.current_page_context().rotation,
        };
        if child_fragment_replay {
            self.current_page = page_for_context(replay_page_context);
            self.current_page_context = replay_page_context;
        } else {
            // The backing page is a tall paint canvas, while the inherited
            // page context remains the CSS viewport for `vh`, page-area
            // percentages, and page-relative descendants.
            self.current_page = Page::new(context.item_width.points().max(1.0), offpage_top);
            self.current_page_context = snapshot.current_page_context();
        }
        if child_fragment_replay {
            let continuation_context = PageContext {
                size: PageSize::from_points(
                    context.item_width.points().max(1.0),
                    table_continuation_capacity.max(1.0),
                ),
                margins: PageMargins::all_points(0.0),
                edges: PageBoxEdges::ZERO,
                rotation: snapshot.current_page_context().rotation,
            };
            self.fragmentainer_override = Some(FragmentainerOverride {
                kind: FragmentainerKind::Page,
                initial_context: replay_page_context,
                initial_fragmentainer_count: 1,
                context: continuation_context,
                relax_widows_orphans: false,
            });
        }
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();
        if !child_fragment_replay {
            // The source canvas has no page boundary of its own. The outer
            // flex fragment plan is solely responsible for slicing it.
            self.fragmentainer_override = None;
            self.fragmentation_suppression_depth += 1;
        }

        // Replay is laid out in an off-page coordinate system and translated
        // back to the selected source slice below. A positioned descendant of
        // a fragmented flex item must nevertheless resolve its insets against
        // the flex container's *fragment-local* containing block, rather than
        // against the original page coordinates retained by the outer layout.
        //
        // The transformed containing block keeps normal positioned layout
        // responsible for sizing and inset resolution; this function only
        // changes coordinate spaces before replay.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_positioning_containing_block =
            context
                .positioning_containing_block
                .map(|containing_block| {
                    ContainingBlock::from_page_top_rect(PageTopRect::new(
                        containing_block.x() - slice_border_box.x(),
                        offpage_top + containing_block.top_y() - source_item_top
                            + source_canvas_slice_start,
                        containing_block.width(),
                        containing_block.height(),
                    ))
                });
        let replay_positioned_containing_block_scope =
            replay_positioning_containing_block.map(|containing_block| {
                self.push_positioned_containing_block(
                    if context.establishes_fixed_containing_block {
                        PositionedContainingBlockMode::FixedAndAbsolute
                    } else {
                        PositionedContainingBlockMode::AbsoluteOnly
                    },
                    containing_block,
                )
            });

        self.with_placed_formatting_context(
            PlacedFormattingContext {
                content_left: 0.0,
                content_width: context.available_width_for_replay(),
                // The frozen used height remains on `replay_style` and is
                // still supplied as `percentage_height_basis` below.  A
                // continuous source canvas itself has no available block-end:
                // treating its used box as a fragmentainer cap would clip
                // visible descendant overflow before Flex can select slices.
                content_height: child_fragment_replay.then(|| {
                    Definite::new(PhysicalContentHeight::new(content_box_pt(
                        table_first_capacity,
                    )))
                }),
                table_wrapper_border_box_block_size: (!table_replay)
                    .then(|| {
                        auto_table_wrapper_block_size_override(&child.style, context.item_height)
                    })
                    .flatten(),
                replay_logical_inline_size: child.anonymous_content().is_some().then(|| {
                    LogicalInlineContentSize::new(
                        context.available_width_for_replay().content_box_length(),
                    )
                }),
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
                float_scope: ReplayFloatScope::IsolatedFormattingContext,
            },
            replay_style,
            |layout| {
                let principal_box_paint_mode = if replay_owns_principal_decoration {
                    PrincipalBoxPaintMode::ParentPaints
                } else {
                    PrincipalBoxPaintMode::RootPaints
                };
                if child_fragment_replay {
                    layout.layout_split_flex_item_continuation_contents(
                        child,
                        replay_style,
                        stylesheets,
                        context.percentage_height_basis,
                        principal_box_paint_mode,
                    );
                } else {
                    layout.layout_flex_item_contents(
                        child,
                        replay_style,
                        stylesheets,
                        context.percentage_height_basis,
                        principal_box_paint_mode,
                    );
                }
            },
        );

        if !child_fragment_replay {
            self.fragmentation_suppression_depth -= 1;
        }

        // A descendant-overflow replay lays the complete child tree once on
        // its off-page source canvas, then lets the flex fragment plan select
        // visible slices. Unlike an independently fragmenting child it does
        // not populate `replay_fragment_local_block_ends`, but its final
        // logical block end is still available from the scratch cursor. Export
        // it for the one destination slice that contains that end so the
        // automatic flex wrapper and following normal flow do not claim the
        // unused tail of the final page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if !table_replay && !child_fragment_replay {
            let source_end = offpage_top - self.cursor_y;
            let source_slice = context.continuation.source_content_slice;
            if source_end >= source_slice.block_start.points() - 0.01
                && source_end <= source_slice.block_end.points() + 0.01
            {
                *replay_destination_block_end = Some(
                    slice_border_box.y() + slice_border_box.height()
                        - (source_end - source_slice.block_start.points()).max(0.0),
                );
            }
        }

        if child_fragment_replay && table_replay_fragments.is_empty() {
            table_replay_fragments.extend(
                self.pages
                    .iter()
                    .chain(std::iter::once(&self.current_page))
                    .map(Page::paint_fragment),
            );
            replay_fragment_local_block_ends.resize(table_replay_fragments.len(), None);
            // Only the active local page exposes its final layout cursor.
            // Earlier pages may be fully occupied, but their exact cursor is
            // neither needed nor recoverable from paint bounds alone.
            if !table_replay && let Some(last) = replay_fragment_local_block_ends.last_mut() {
                *last = Some(self.cursor_y);
            }
        }

        // Positioned descendants whose containing block is the flex
        // container escape the item border box. Keep their layers separate
        // from the in-flow replay so they are clipped by the container's
        // fragment, not by a zero-sized or otherwise split item.
        // A nested speculative layout may restore a checkpoint taken before
        // this replay and therefore discard the replay's provisional layers.
        // That is a valid empty result, not an invalid layer checkpoint: only
        // layers still owned by this replay may be extracted.
        let mut escaped_positioned_layers = if positioned_layer_start < self.positioned_layers.len()
        {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        escaped_positioned_layers.sort_by_key(|layer| layer.stack_level.sort_key());

        if let Some(scope) = replay_positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }

        // A table's replay page is a real local fragmentainer. Its first
        // page starts at the first remaining capacity, while every committed
        // continuation starts at a full continuation capacity.  Translate
        // from that selected page's local origin rather than retaining the
        // first fragment's origin for every slice.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let fragment_translation = if child_fragment_replay {
            PaintTranslation::new(slice_border_box.x(), slice_border_box.y())
        } else {
            source_slice_replay_translation(
                slice_border_box,
                source_item_top,
                offpage_top,
                source_canvas_slice_start,
            )
        };
        let source_fragment = if child_fragment_replay {
            table_replay_fragments
                .get(table_fragment_ordinal)
                .cloned()
                .unwrap_or_else(|| self.current_page.paint_fragment())
        } else {
            self.current_page
                .paint_fragment()
                .into_fragmentable_source_canvas()
        };
        if !child_fragment_replay {
            *cached_source_replay = Some(ContinuousSourceReplay {
                paint: source_fragment.clone(),
                effects: self
                    .take_positioned_scratch_side_effects()
                    .into_continuous_source_effects(),
                source_height: context.source_height,
                scratch_top: offpage_top,
            });
        }
        let fragment = source_fragment.translated(fragment_translation);
        // A descendant-overflow source replay contains only the item's
        // independently formatted subtree; its principal flex decoration was
        // committed separately from the materialized item record above.
        // Clip that whole subtree structurally so descendant backgrounds are
        // sliced with their content while nested stacking contexts (notably
        // opacity) retain their effects. A contents-only overflow scope
        // deliberately leaves BackgroundBorder paint outside the clip, which
        // lets a tall descendant background cover a later flex item's owner.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-color-4/#transparency>
        // The selected flex slice constrains every descendant paint band,
        // including an ordinary nested block's background. A contents-only
        // overflow scope leaves that nested BackgroundBorder band outside the
        // continuation clip, so a final partial slice either disappears under
        // the flex container's background or paints through later siblings.
        // Preserve the nested paint-tree structure while clipping it at the
        // common fragment-span boundary.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let fragment = fragment
            .with_primitives_sliced_to_fragmentainer_rect_preserving_structure(replay_clip)
            // Source-canvas descendants can carry nested stacking contexts
            // whose primitive descendants are structurally retained above.
            // Enforce the same committed fragmentainer bound on that retained
            // tree so visible overflow reaches sibling columns but cannot
            // escape the flex container's selected destination fragment.
            .clipped_to_rect(replay_clip);
        // The geometric fragmentainer slice above has already trimmed the
        // captured primitive tree. Do not wrap it in a second PDF overflow
        // clip: applying one around the complete stacking context
        // antialiases a background at every page edge, unlike the equivalent
        // ordinary block which paints each used slice directly.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        if table_replay {
            record_table_replay_fragment_bottom(
                table_replay_fragment_bottoms,
                context.continuation.fragmentainer_index,
                &fragment,
            );
        }
        if !table_replay
            && let Some(Some(local_block_end)) =
                replay_fragment_local_block_ends.get(table_fragment_ordinal)
        {
            *replay_destination_block_end = Some(slice_border_box.y() + *local_block_end);
        }
        self.restore(snapshot);

        // An empty size-contained flex item has no descendant paint to replay:
        // its durable flex fragment span has already materialized the full
        // principal border-box decoration for every destination page. Replaying
        // the scratch tree would add that same decoration a second time, with
        // a different local fragmentainer origin on continuations.
        // <https://drafts.csswg.org/css-contain-1/#containment-size>
        // <https://drafts.csswg.org/css-break-3/#box-splitting>
        if !replay_owns_principal_decoration || replay_has_descendant_paint {
            self.append_split_flex_item_replay(placed_style, replay_clip, fragment);
        }

        // A layer which escaped the split item belongs to the flex
        // container's containing block. Positioned layout has therefore
        // already expressed its inline coordinate in the source page's
        // coordinate space; translating it by the item slice's inline origin
        // would apply that origin twice. Its block coordinate is still on the
        // off-page replay canvas and needs the selected source-slice mapping.
        // <https://www.w3.org/TR/css-position-3/#def-cb>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_translation = PaintTranslation::new(0.0, fragment_translation.y);
        for layer in escaped_positioned_layers {
            let layer_fragment = positioned_layer_fragment(&layer);
            let mut fragment = layer_fragment.translated(replay_translation);
            if let Some(clip) = context.positioned_descendant_clip {
                fragment = fragment.clipped_to_rect(clip);
            }
            if !fragment.is_empty() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
        }
    }

    fn append_split_flex_item_replay(
        &mut self,
        placed_style: &ComputedStyle,
        slice_border_box: PaintClip,
        fragment: PaintFragment,
    ) {
        if fragment.is_empty() {
            return;
        }
        let policy =
            StackingContextPolicy::for_fragmented_flex_item(placed_style, slice_border_box);
        let effects = policy.effects;
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(slice_border_box);
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(policy.parent_band, context),
            PaintTranslation::identity(),
        );
    }

    pub(in crate::layout::flex) fn resolve_styled_children_used_lengths(
        &mut self,
        children: &mut [StyledChild<'_>],
    ) {
        for child in children {
            child.style = self
                .style_with_current_used_lengths(&child.style)
                .clone_for_legacy_used_consumer();
        }
    }
}
/// Translate a frozen descendant source canvas into one flex continuation.
///
/// Page-top coordinates decrease in the block direction. Advancing the
/// source-slice start therefore moves the source canvas toward the page top
/// relative to the destination item origin, which adds the source offset to
/// the final paint translation.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
pub(in crate::layout::flex) fn source_slice_replay_translation(
    slice_border_box: PaintClip,
    source_item_top: f32,
    offpage_top: f32,
    source_canvas_slice_start: f32,
) -> PaintTranslation {
    PaintTranslation::new(
        slice_border_box.x(),
        source_item_top - offpage_top + source_canvas_slice_start,
    )
}
