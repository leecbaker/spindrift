use super::*;

fn equivalent_positioned_layer(left: &PositionedPaintLayer, right: &PositionedPaintLayer) -> bool {
    let mut left_context = left.context.clone();
    let mut right_context = right.context.clone();
    left_context.source_order = 0;
    right_context.source_order = 0;
    left.page_index == right.page_index
        && left.source_element == right.source_element
        && left.source_style == right.source_style
        && left.source_is_target == right.source_is_target
        && left.stack_level == right.stack_level
        && left_context == right_context
        && left.links == right.links
        && left.escaped_atom_replay == right.escaped_atom_replay
        && left.overflow_clip_containing_block == right.overflow_clip_containing_block
}

fn element_contains(element: &Element, target: crate::dom::ElementId) -> bool {
    element.id == target
        || element.children.iter().any(|child| {
            matches!(
                &child.kind,
                NodeKind::Element(descendant) if element_contains(descendant, target)
            )
        })
}

/// Resolve an escaped positioned descendant's hypothetical source in the
/// coordinate space that owns its actual containing block.
///
/// An atom-owned containing block consumes the ordinary static rectangle
/// captured by that atom's block traversal. A page-owned containing block
/// instead receives the atom-local logical source projected into page-owned
/// geometry; escaped-layer replay later carries only automatic axes to the
/// atom's final placement.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
fn resolve_escaped_atom_static_position(
    context: EscapedAtomPositioningContext,
    containing_block: PositionedContainingBlockContext,
    active_space: AtomicInlineCoordinateSpaceId,
) -> AbsoluteStaticPosition {
    match containing_block.coordinate_space {
        PositionedCoordinateSpace::AtomicInline(space) if space == active_space => {
            context.static_position.in_atomic_space()
        }
        PositionedCoordinateSpace::Page | PositionedCoordinateSpace::AtomicInline(_) => context
            .static_position
            .in_page_owned_containing_block(containing_block.geometry),
    }
}

/// Whether resolving an abspos box's physical height requires laying out its
/// contents.
///
/// This is intentionally distinct from whether the authored value is literally
/// `auto`: intrinsic sizing keywords remain indefinite, so their used block
/// size cannot be determined until their content has been laid out. In
/// particular, that measurement must leave percentage-height descendants
/// indefinite rather than resolving them against the abspos containing block.
/// <https://drafts.csswg.org/css-sizing-3/#definite>
/// <https://drafts.csswg.org/css-sizing-3/#sizing-values>
/// <https://drafts.csswg.org/css-position-3/#abspos-auto-size>
pub(in crate::layout) fn positioned_vertical_size_requires_content_measurement(
    style: &ComputedStyle,
) -> bool {
    style.box_values.height.is_auto()
        || needs_intrinsic_height_contribution(style.box_values.height.value().clone())
}

/// Resolve the ordinary-flow static-position alignment before measuring an
/// automatic positioned inline size.
///
/// A Grid ancestor can replace the actual containing block of a nested
/// positioned descendant without owning its static-position rectangle.  CSS
/// Align derives the available automatic size from the static rectangle's
/// selected edge to the opposite edge of that actual containing block.  The
/// rectangle must therefore remain in its intervening formatting context's
/// axes and direction while Grid contributes only the latter box:
/// <https://drafts.csswg.org/css-align-3/#abspos-static-size> and
/// <https://www.w3.org/TR/css-grid-1/#abspos>.
fn ordinary_static_alignment_for_auto_sizing(
    position: Option<AbsoluteStaticPosition>,
    style: &ComputedStyle,
) -> Option<AbsposStaticAlignment> {
    let rectangle = position.and_then(AbsoluteStaticPosition::static_position_rectangle)?;
    let inline = if style.justify_self.keyword == SelfAlignmentKeyword::Auto {
        rectangle.justify_items
    } else {
        style.justify_self
    };
    let block = if style.align_self.keyword == SelfAlignmentKeyword::Auto {
        rectangle.align_items
    } else {
        style.align_self
    };
    Some(AbsposStaticAlignment::new(
        rectangle.area,
        rectangle.writing_mode,
        rectangle.direction,
        style.writing_mode,
        style.used_direction(),
        inline,
        block,
    ))
}

/// The entry point for the in-flow surrogate used to format an already
/// resolved positioned box. It is intentionally separate from static-position
/// geometry: the positioned resolver supplies a physical border-box origin,
/// while this adapter enters the surrogate through the exact inline bridge
/// that makes block-flow replay return to that border-box origin.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
/// <https://drafts.csswg.org/css-writing-modes-4/#inline-flow>
#[derive(Debug, Clone, Copy)]
struct PositionedFlowOrigin {
    content_left: f32,
    content_right: f32,
    cursor_y: f32,
}

impl PositionedFlowOrigin {
    fn from_resolved_positioned_box(
        resolved_border_box: PageTopRect,
        surrogate_inline_bridge: f32,
    ) -> Self {
        // A positioned principal is already resolved in page coordinates.
        // Re-entering its flow surrogate must begin at that physical top edge,
        // regardless of the logical inline-start side. Directional text
        // preparation owns its own advance; moving the whole surrogate to a
        // logical bottom edge displaces the principal border box.
        // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
        let cursor_y = {
            // Positioned layout has already resolved the physical border-box
            // top. Re-entering its flow surrogate must preserve that edge;
            // subtracting the logical inline extent moves a vertical
            // positioned principal down by its entire height.
            // <https://drafts.csswg.org/css-position-3/#absolute-positioning>
            resolved_border_box.top_y()
        };
        Self {
            // Block layout removes the full physical inline non-content
            // extent while resolving a surrogate's outer border box. Passing
            // the resolved border-box start here therefore replayed RTL and
            // vertical-rl boxes one non-content extent away from the box used
            // to select its snap position.
            // This bridge intentionally stays physical: writing-mode
            // projection happens in the normal block layout path.
            // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
            content_left: resolved_border_box.x() + surrogate_inline_bridge,
            content_right: resolved_border_box.x()
                + surrogate_inline_bridge
                + resolved_border_box.width(),
            cursor_y,
        }
    }
}

/// Whether an absolutely positioned principal needs its complete continuous
/// margin-box span, rather than only the pages evidenced by captured output.
///
/// Absolute boxes fragment with their containing block, but CSS Paged Media
/// advises user agents not to generate a long sequence of content-empty pages
/// merely to honor a positioned box's extent. A visible principal decoration,
/// a viewport-fixed descendant, or semantically non-empty content requires
/// the continuous span. Captured paint and positioned layers retain their
/// observed destination pages without turning an otherwise content-empty
/// principal into a continuous tail.
/// <https://drafts.csswg.org/css-break-4/#abspos-breaking>
/// <https://www.w3.org/TR/css-page-3/#renderingpages>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalPageSpanObligation {
    ObservedOutput,
    ContinuousPrincipal,
}

impl PrincipalPageSpanObligation {
    fn for_positioned_principal(
        has_principal_decoration: bool,
        has_viewport_fixed_descendant: bool,
        has_semantically_nonempty_content: bool,
    ) -> Self {
        if has_principal_decoration
            || has_viewport_fixed_descendant
            || has_semantically_nonempty_content
        {
            Self::ContinuousPrincipal
        } else {
            Self::ObservedOutput
        }
    }

    const fn requires_continuous_principal_span(self) -> bool {
        matches!(self, Self::ContinuousPrincipal)
    }
}

/// Whether an in-flow formatting subtree has semantic content even when no
/// principal paint fragment is captured. Out-of-flow descendants plan their
/// own page spans and therefore must not turn their empty positioned parent
/// into a page-obligating subtree.
/// <https://drafts.csswg.org/css-break-4/#abspos-breaking>
fn has_semantically_nonempty_in_flow_content(boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    boxes.iter().any(|box_| {
        if matches!(box_.style().position, Position::Absolute | Position::Fixed) {
            return false;
        }
        match box_ {
            box_tree::FormattingBox::Text(text) => !text.text.trim().is_empty(),
            box_tree::FormattingBox::Replaced(_) => true,
            _ => {
                box_.style().content.is_generated()
                    || has_semantically_nonempty_in_flow_content(box_.children())
            }
        }
    })
}

/// Build the table-specific sizing contract from the absolute containing
/// block and the already-resolved positioned content box.
///
/// The containing block supplies the available logical inline size for CSS
/// Tables' auto-width algorithm. The resolved content size is forwarded only
/// when the authored table has a definite logical block size; an intrinsic
/// auto block size must remain owned by table row sizing. The final inset
/// equation continues to own placement and is deliberately not represented in
/// this record:
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>,
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>,
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning>, and
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping>.
pub(in crate::layout) fn positioned_table_sizing_for_geometry(
    source_style: &ComputedStyle,
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    content_width: PhysicalContentWidth,
    content_height: PhysicalContentHeight,
) -> PositionedTableSizing {
    let writing_mode = style.writing_mode;
    let available_inline_size = if writing_mode.has_vertical_lines() {
        LogicalInlineContentSize::new(content_box_pt(containing_block.height()))
    } else {
        LogicalInlineContentSize::new(content_box_pt(containing_block.width()))
    };
    let authored_block_size_is_definite = if writing_mode.has_vertical_lines() {
        !source_style.box_values.width.is_auto()
    } else {
        !source_style.box_values.height.is_auto()
    };
    let definite_block_content_size = authored_block_size_is_definite.then(|| {
        LogicalBlockContentSize::new(if writing_mode.has_vertical_lines() {
            content_width.content_box_length()
        } else {
            content_height.content_box_length()
        })
    });
    PositionedTableSizing {
        available_inline_size,
        definite_block_content_size,
        writing_mode,
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_positioned_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        if self.is_positioned_auto_size_measurement() {
            // Absolutely and fixed-position descendants are out of flow and
            // do not contribute to a block formatting context root's auto
            // height. Their final positioned pass owns all paint and
            // fragmentation after this measurement completes.
            // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            return;
        }
        // An inline collector may encounter a block-level positioned source
        // while its enclosing block's final fragment is not known yet. That
        // probe has only an inline static edge; the normal block dispatcher
        // follows with the complete static-position rectangle. It must not
        // commit a second, page-zero positioned paint record meanwhile.
        // <https://drafts.csswg.org/css-position-3/#static-position>
        if self.out_of_flow_prebreak_suppression_depth > 0
            && !style.abspos_static_source.is_inline_level()
            && self.absolute_static_position.is_some_and(|position| {
                !position.has_vertical_position() && position.static_position_rectangle().is_none()
            })
        {
            return;
        }
        let positioned_style = self.positioned_used_style(style);
        let source_style = positioned_style.source();
        let style = positioned_style.used();
        let source_static_rect = self
            .absolute_static_position
            .and_then(AbsoluteStaticPosition::horizontal_block_static_rectangle)
            .map(|rect| {
                PositionedChildStaticRect::new(rect.x(), rect.x() + rect.width(), rect.top_y())
            })
            .unwrap_or_else(|| {
                PositionedChildStaticRect::new(self.content_left, self.content_right, self.cursor_y)
            });
        let captures_multicol_positioned_principal =
            self.capture_multicol_positioned_principal(element, source_style, source_static_rect);
        if captures_multicol_positioned_principal && self.multicol_balance_probe_depth > 0 {
            // A balance probe asks only whether in-flow content fits a
            // candidate column height. Its temporary pages do not own the
            // positioned principal, and the deferred descriptor above keeps
            // the final static-position source for the one committed replay.
            // Running the surrogate here would allocate (and then clone on
            // restore) speculative positioned paint for every binary-search
            // candidate.
            // <https://drafts.csswg.org/css-multicol-1/#column-balancing>
            // <https://drafts.csswg.org/css-position/#abspos-breaking>
            return;
        }
        // Positioned table roots bypass the normal table dispatcher, which
        // ordinarily materializes this frozen structure before table layout.
        // The absolute-position equation needs the conflict-resolved outer
        // half-borders before it resolves a border-box width or height, so
        // construct the same immutable table fragment at this boundary.
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        // <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
        let built_positioned_child_boxes;
        let built_positioned_table_fragment;
        let table_fragment = if style.display.is_table() && table_fragment.is_none() {
            let table_children = if let Some(children) = child_boxes {
                children
            } else {
                built_positioned_child_boxes = self
                    .build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
                &built_positioned_child_boxes
            };
            let signature = self
                .ancestors
                .last()
                .cloned()
                .unwrap_or_else(|| element_signature(element));
            built_positioned_table_fragment =
                box_tree::build_frozen_table_fragment(element, &signature, style, table_children);
            Some(&built_positioned_table_fragment)
        } else {
            table_fragment
        };
        let containment = used_property_containment(element, style);
        let paint_page_index = self.pages.len();
        // The positioned principal's resolved in-flow surrogate owns static
        // scroll-snap capture. At this boundary its final block size is not
        // available yet; opening a scope here would construct a zero-height
        // snapport before the surrogate has established the used padding box.
        // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
        let static_scroll_snap_scope = false;
        let positioned_layer_start = self.positioned_layers.len();
        let fixed_layer_start = self.fixed_layers.len();
        let retained_viewport_fixed_descendant = self
            .fixed_layers
            .iter()
            .any(|layer| element_contains(element, layer.source_element));
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_inline_static_position = self.inline_static_position;
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;

        let locally_contained_fixed_block = (style.position == Position::Fixed)
            .then(|| self.fixed_containing_blocks.last().cloned())
            .flatten();
        if locally_contained_fixed_block.is_some() {
            // Intrinsic/table planning can previously have materialized this
            // same fixed element against the initial containing block. Once a
            // committed transformed containing block is available, that
            // viewport-fixed layer is stale and must not survive alongside
            // the locally captured positioned layer.
            self.fixed_layers
                .retain(|layer| layer.source_element != element.id);
        }
        let escaped_atom_positioning_context = (style.position == Position::Absolute
            && self.escaped_atom_positioning_depth > 0)
            .then_some(self.escaped_atom_positioning_context)
            .flatten();
        let uses_escaped_atom_outer_containing_block = style.position == Position::Absolute
            && self.containing_blocks.is_empty()
            && escaped_atom_positioning_context.is_some();
        let source_containing_block_context = if style.position == Position::Fixed {
            locally_contained_fixed_block.unwrap_or_else(|| {
                PositionedContainingBlockContext::page(self.page_containing_block())
            })
        } else {
            self.containing_blocks
                .last()
                .cloned()
                .or_else(|| {
                    uses_escaped_atom_outer_containing_block.then(|| {
                        PositionedContainingBlockContext::page(
                            escaped_atom_positioning_context
                                .expect("escaped atom context was checked above")
                                .actual_containing_block,
                        )
                    })
                })
                .unwrap_or_else(|| {
                    PositionedContainingBlockContext::page(self.page_containing_block())
                })
        };
        let source_containing_block = source_containing_block_context.geometry;
        let uses_initial_page_containing_block =
            matches!(style.position, Position::Absolute | Position::Fixed)
                && self.containing_blocks.is_empty()
                && locally_contained_fixed_block.is_none();
        let grid_descendant_containing_block = (style.position == Position::Absolute)
            .then(|| {
                self.grid_positioning_scopes.iter().rev().find_map(|scope| {
                    scope.descendant_containing_block(style, source_containing_block)
                })
            })
            .flatten();
        // A qualifying Grid descendant uses the Grid area as its actual
        // containing block. Its static-position rectangle remains the one
        // captured from the nested ordinary formatting context; only direct
        // positioned Grid children receive a Grid static rectangle.
        // <https://www.w3.org/TR/css-grid-1/#abspos> and
        // <https://www.w3.org/TR/css-position-3/#staticpos-rect>
        let containing_block_context = grid_descendant_containing_block
            .map(|context| {
                PositionedContainingBlockContext::in_space(
                    context.containing_block,
                    source_containing_block_context.coordinate_space,
                )
            })
            .unwrap_or(source_containing_block_context);
        let containing_block = containing_block_context.geometry;
        let containing_block_fragment_origin_page_index = containing_block
            .origin_page_index
            .unwrap_or(paint_page_index);
        // Preserve whether the authored axis size is automatic before used
        // values normalize replaced-element intrinsic dimensions. Grid's
        // abspos self-alignment must run only after that intrinsic size is
        // known for an automatic axis.
        // <https://www.w3.org/TR/css-grid-1/#abspos-items> and
        // <https://drafts.csswg.org/css-align-3/#abspos-align>.
        let horizontal_size_was_auto = source_style.box_values.width.is_auto();
        let vertical_size_was_auto = source_style.box_values.height.is_auto();
        let mut used_style = style.clone();
        // Grid self-alignment of an automatically sized positioned item must
        // wait for intrinsic sizing below. Replaced elements receive used
        // intrinsic dimensions into `used_style`, so retain the authored auto
        // state before that normalization.
        apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(containing_block.width())),
        );
        if used_style.display.is_inline_level() || style.abspos_static_source.is_inline_level() {
            // CSS Display blockifies the outer display type of absolutely
            // positioned boxes for layout. The source display can already
            // have been blockified while preserving only its static-position
            // provenance, so consult that provenance as well:
            // https://www.w3.org/TR/css-display-3/#transformations
            used_style.display = used_style.display.blockified();
        }
        // Resolve a non-replaced positioned box's auto axis from a definite
        // authored opposite axis before solving the absolute inset equations.
        // Inset-derived fill sizes remain part of those equations and are not
        // treated as an authored preferred size here.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        let collapsed_table_wrapper_insets = is_html_table_element(element)
            .then(|| {
                self.resolved_collapsed_table_wrapper_insets(style, stylesheets, table_fragment)
            })
            .flatten();
        let positioned_border_widths = collapsed_table_wrapper_insets
            .map(|insets| insets.border_widths)
            .unwrap_or_else(|| used_border_widths(style));
        if !is_replaced_element(element) {
            let horizontal_non_content = used_style.padding.left
                + used_style.padding.right
                + positioned_border_widths.left
                + positioned_border_widths.right;
            let vertical_non_content = used_style.padding.top
                + used_style.padding.bottom
                + positioned_border_widths.top
                + positioned_border_widths.bottom;
            let definite_width = used_content_box_width_or_auto(
                &used_style,
                layout_pt(containing_block.width()),
                non_content_pt(horizontal_non_content),
            )
            .map(|width| {
                constrain_content_width(
                    &used_style,
                    width,
                    PercentageBasis::definite(layout_pt(containing_block.width())),
                )
                .points()
            });
            let definite_height = used_content_box_height_or_auto(
                &used_style,
                layout_pt(containing_block.height()),
                non_content_pt(vertical_non_content),
            )
            .map(|height| {
                constrain_content_height(
                    &used_style,
                    height,
                    PercentageBasis::definite(layout_pt(containing_block.height())),
                )
                .points()
            });
            match (horizontal_size_was_auto, vertical_size_was_auto) {
                (true, false) => {
                    if let Some(height) = definite_height
                        && let Some(width) = non_replaced_aspect_ratio_content_width(
                            &used_style,
                            height,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                (false, true) => {
                    if let Some(width) = definite_width
                        && let Some(height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                (true, true) => {
                    let horizontal_fill = used_inset_left(&used_style, containing_block)
                        .zip(used_inset_right(&used_style, containing_block))
                        .map(|(left, right)| {
                            constrain_content_width(
                                &used_style,
                                content_box_pt(
                                    (containing_block.width()
                                        - left
                                        - used_style.margin.left
                                        - horizontal_non_content
                                        - used_style.margin.right
                                        - right)
                                        .max(0.0),
                                ),
                                PercentageBasis::definite(layout_pt(containing_block.width())),
                            )
                            .points()
                        });
                    let vertical_fill = used_inset_top(&used_style, containing_block)
                        .zip(used_inset_bottom(&used_style, containing_block))
                        .map(|(top, bottom)| {
                            constrain_content_height(
                                &used_style,
                                content_box_pt(
                                    (containing_block.height()
                                        - top
                                        - used_style.margin.top
                                        - vertical_non_content
                                        - used_style.margin.bottom
                                        - bottom)
                                        .max(0.0),
                                ),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points()
                        });
                    // When both dimensions are auto, a definite inline fill
                    // size is the ratio's primary axis. This is also the
                    // tie-breaker when both inset pairs are definite.
                    if let Some(mut width) = horizontal_fill {
                        if let Some(mut height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        ) {
                            height = constrain_content_height(
                                &used_style,
                                content_box_pt(height),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points();
                            if let Some(constrained_width) = non_replaced_aspect_ratio_content_width(
                                &used_style,
                                height,
                                horizontal_non_content,
                                vertical_non_content,
                            ) {
                                width = constrain_content_width(
                                    &used_style,
                                    content_box_pt(constrained_width),
                                    PercentageBasis::definite(layout_pt(containing_block.width())),
                                )
                                .points();
                            }
                            set_style_used_width(&mut used_style, width);
                            set_style_used_height(&mut used_style, height);
                            used_style.box_sizing = BoxSizing::ContentBox;
                        }
                    } else if let Some(mut height) = vertical_fill
                        && let Some(mut width) = non_replaced_aspect_ratio_content_width(
                            &used_style,
                            height,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        width = constrain_content_width(
                            &used_style,
                            content_box_pt(width),
                            PercentageBasis::definite(layout_pt(containing_block.width())),
                        )
                        .points();
                        if let Some(constrained_height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        ) {
                            height = constrain_content_height(
                                &used_style,
                                content_box_pt(constrained_height),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points();
                        }
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                _ => {}
            }
        }
        // Direct flex/Grid children install a formatting-context static
        // rectangle before the generic absolute-positioning algorithm. A
        // nested Grid descendant instead retains its ordinary-flow rectangle
        // while Grid replaces only `containing_block` above. In both cases,
        // preserve the selected static edge for automatic inline sizing; the
        // actual containing block still resolves the final inset equation.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        // <https://drafts.csswg.org/css-align-3/#abspos-sizing>
        let active_atomic_space = escaped_atom_positioning_context.map(|_| {
            self.active_atomic_inline_coordinate_spaces
                .last()
                .copied()
                .expect("escaped atomic positioning has an active coordinate space")
        });
        let resolved_escaped_atom_static_position = escaped_atom_positioning_context
            .zip(active_atomic_space)
            .map(|(context, active_space)| {
                resolve_escaped_atom_static_position(
                    context,
                    containing_block_context,
                    active_space,
                )
            });
        let containing_block_is_active_atom = active_atomic_space.is_some_and(|active_space| {
            containing_block_context.coordinate_space
                == PositionedCoordinateSpace::AtomicInline(active_space)
        });
        let mut absolute_static_position =
            if escaped_atom_positioning_context.is_some() && !containing_block_is_active_atom {
                resolved_escaped_atom_static_position
            } else {
                self.absolute_static_position
                    .or(resolved_escaped_atom_static_position)
            };
        let horizontal_insets_are_auto_for_available =
            used_inset_left(&used_style, containing_block).is_none()
                && used_inset_right(&used_style, containing_block).is_none();
        let static_available_outer_width = (horizontal_insets_are_auto_for_available
            && horizontal_size_was_auto)
            .then(|| {
                absolute_static_position
                    .and_then(AbsoluteStaticPosition::static_alignment)
                    .or_else(|| {
                        ordinary_static_alignment_for_auto_sizing(
                            absolute_static_position,
                            &used_style,
                        )
                    })
                    .and_then(|alignment| {
                        alignment.available_horizontal_outer_size(containing_block)
                    })
            })
            .flatten();
        let positioned_available_outer_width = (static_available_outer_width
            .unwrap_or(containing_block.width())
            - used_style.margin.left
            - used_style.margin.right)
            .max(used_style.font_size);
        let replaced_content_size = if is_replaced_element(element) {
            resolve_replaced_element(
                element,
                &used_style,
                ReplacedBoxSizingContext {
                    available_width: content_box_pt(positioned_available_outer_width),
                    inline_percentage_basis: PercentageBasis::definite_from(
                        content_box_pt(positioned_available_outer_width),
                        IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                    ),
                    block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                        PercentageBasis::definite_from(
                            content_box_pt(containing_block.height()),
                            BlockSizeBasisSource::AbsolutePositioned,
                        ),
                    ),
                },
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|replaced| {
                let geometry = replaced.geometry();
                // CSS 2.2 gives absolutely positioned replaced elements their
                // own auto-size rules: intrinsic dimensions and aspect ratio
                // resolve the content size before the absolute inset equation
                // is solved.
                // <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-width>
                // and <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-height>.
                set_style_used_width(&mut used_style, geometry.content_size.width);
                set_style_used_height(&mut used_style, geometry.content_size.height);
                (geometry.content_size.width, geometry.content_size.height)
            })
        } else {
            None
        };
        let style = &used_style;
        // A non-replaced preferred aspect ratio can turn an authored auto
        // height into a definite used height. Content measurement is therefore
        // a decision over the final used style; authored auto flags above
        // remain available for rules that require authored-state semantics.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        let vertical_size_requires_content_measurement =
            positioned_vertical_size_requires_content_measurement(style);
        let left_inset = used_inset_left(style, containing_block);
        let right_inset = used_inset_right(style, containing_block);
        let top_inset = used_inset_top(style, containing_block);
        let bottom_inset = used_inset_bottom(style, containing_block);
        let horizontal_insets_are_auto = left_inset.is_none() && right_inset.is_none();
        let vertical_insets_are_auto = top_inset.is_none() && bottom_inset.is_none();
        // With both block-axis insets auto, the static-position rectangle is
        // generated on the source fragment's page even when the containing
        // block is the initial page area. Explicit insets remain anchored to
        // the containing block's block-start page.
        // <https://drafts.csswg.org/css-position-3/#static-position-rectangle>
        let containing_block_origin_page_index =
            if style.position == Position::Absolute && vertical_insets_are_auto {
                paint_page_index
            } else {
                containing_block_fragment_origin_page_index
            };
        // A hypothetical normal-flow position can lie outside the containing
        // block, for example through a negative margin on an intervening block
        // ancestor. CSS Positioned Layout uses that position directly when
        // resolving two automatic inline insets; it must not be clamped back
        // into the containing block.
        // <https://www.w3.org/TR/css-position-3/#static-position-rectangle>
        // An auto-inset positioned box rooted in the initial containing block
        // still takes its static-position rectangle from the hypothetical
        // in-flow box. `previous_left` and `previous_right` are the active
        // normal-flow content edges, so they already include any propagated
        // root/body canvas inset. Adding the canvas inset again would shift a
        // body descendant by its margin twice.
        // <https://www.w3.org/TR/css-position-3/#static-position-rectangle>
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        let static_source_left = previous_left;
        let static_source_right = previous_right;
        let source_static_position = if escaped_atom_positioning_context.is_some() {
            absolute_static_position.expect("escaped atom has a resolved static position")
        } else {
            AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
                static_source_left,
                static_source_right,
                previous_cursor_y,
                true,
            )
        };
        // The static-position rectangle is an alignment container derived
        // from the formatting context the box would have joined, not the
        // actual absolute-position containing block. For ordinary block flow
        // it has zero block-axis thickness; for inline flow it has zero
        // inline-axis thickness and spans the hypothetical line box.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        // <https://drafts.csswg.org/css-align-3/#align-abspos>
        let static_containing_block = self.static_position_containing_blocks.last().copied();
        let retained_static_rectangle =
            absolute_static_position.and_then(AbsoluteStaticPosition::static_position_rectangle);
        let inline_static_position = self.inline_static_position;
        let (area, writing_mode, direction, default_inline, default_block) =
            if let Some(rectangle) = retained_static_rectangle {
                (
                    rectangle.area,
                    rectangle.writing_mode,
                    rectangle.direction,
                    rectangle.justify_items,
                    rectangle.align_items,
                )
            } else if style.abspos_static_source.is_inline_level()
                && let Some(position) = inline_static_position
            {
                (
                    position.rectangle.area,
                    position.rectangle.writing_mode,
                    position.rectangle.direction,
                    position.rectangle.justify_items,
                    position.rectangle.align_items,
                )
            } else if let Some((context, area)) = static_containing_block
                .filter(|context| !context.axes.writing_mode().has_vertical_lines())
                .zip(
                    absolute_static_position
                        .and_then(AbsoluteStaticPosition::horizontal_block_static_rectangle),
                )
            {
                // The child may be replayed after its block formatting scope
                // has unwound (notably from an atomic inline). The retained
                // hypothetical source position is therefore the authoritative
                // inline span, rather than the current outer builder span.
                (
                    area,
                    context.axes.writing_mode(),
                    context.axes.direction(),
                    context.justify_items,
                    css::SelfAlignment::NORMAL,
                )
            } else if let Some(context) = static_containing_block {
                let area = if context.axes.writing_mode().has_vertical_lines() {
                    let x = match block_start_side(context.axes.writing_mode()) {
                        PhysicalSide::Left => context.content_rect.x(),
                        PhysicalSide::Right => {
                            context.content_rect.x() + context.content_rect.width()
                        }
                        PhysicalSide::Top | PhysicalSide::Bottom => {
                            unreachable!("a vertical writing mode has a horizontal block axis")
                        }
                    };
                    PageTopRect::new(
                        x,
                        context.content_rect.top_y(),
                        0.0,
                        context.content_rect.height(),
                    )
                } else {
                    PageTopRect::new(
                        context.content_rect.x(),
                        previous_cursor_y,
                        context.content_rect.width(),
                        0.0,
                    )
                };
                (
                    area,
                    context.axes.writing_mode(),
                    context.axes.direction(),
                    context.justify_items,
                    css::SelfAlignment::NORMAL,
                )
            } else {
                // Page-margin and detached replay roots have no active block
                // context. Retain a degenerate ordinary-flow rectangle rather
                // than accidentally expanding it to the actual containing
                // block.
                (
                    PageTopRect::new(static_source_left, previous_cursor_y, 0.0, 0.0),
                    self.containing_block_writing_mode,
                    self.containing_block_direction,
                    css::SelfAlignment::NORMAL,
                    css::SelfAlignment::NORMAL,
                )
            };
        // `auto` inherits the parent's default only while determining the
        // static position; it behaves as `normal` for final abspos layout.
        // <https://drafts.csswg.org/css-align-3/#justify-self-property>
        let inline = if style.justify_self.keyword == SelfAlignmentKeyword::Auto {
            default_inline
        } else {
            style.justify_self
        };
        let block = if style.align_self.keyword == SelfAlignmentKeyword::Auto {
            default_block
        } else {
            style.align_self
        };
        // An ordinary-flow rectangle is authoritative even when an earlier
        // speculative pass attached stale alignment geometry. Flex and Grid
        // never install this payload, so their formatting-context-specific
        // static rectangles remain untouched.
        if retained_static_rectangle.is_some()
            || (absolute_static_position
                .and_then(AbsoluteStaticPosition::static_alignment)
                .is_none()
                && (inline.keyword != SelfAlignmentKeyword::Normal
                    || block.keyword != SelfAlignmentKeyword::Normal))
        {
            absolute_static_position = Some(
                absolute_static_position
                    .unwrap_or(source_static_position)
                    .with_static_alignment(AbsposStaticAlignment::new(
                        area,
                        writing_mode,
                        direction,
                        style.writing_mode,
                        style.used_direction(),
                        inline,
                        block,
                    )),
            );
        }
        let inline_auto_static_y =
            style.abspos_static_source.is_inline_level() && vertical_insets_are_auto;
        let inline_auto_static_x =
            style.abspos_static_source.is_inline_level() && horizontal_insets_are_auto;
        let mut static_horizontal_position =
            if horizontal_insets_are_auto && let Some(position) = absolute_static_position {
                position.horizontal_position(containing_block)
            } else {
                inline_auto_static_x
                    .then_some(inline_static_position)
                    .flatten()
                    .map(|position| {
                        PhysicalStaticAxisFallback::new(
                            position.rectangle.area.x() - containing_block.x(),
                            containing_block.x() + containing_block.width()
                                - (position.rectangle.area.x() + position.rectangle.area.width()),
                        )
                    })
                    .unwrap_or_else(|| source_static_position.horizontal_position(containing_block))
            };
        let static_vertical_base = source_static_position.vertical_start(containing_block);
        let mut static_vertical_start = if vertical_insets_are_auto
            && let Some(position) =
                absolute_static_position.filter(|position| position.has_vertical_position())
        {
            position.vertical_start(containing_block)
        } else if inline_auto_static_y && let Some(position) = inline_static_position {
            containing_block.top_y() - position.rectangle.area.top_y()
        } else {
            // The static position is a signed offset from the containing
            // block and may legitimately fall before its block-start edge.
            // Clamping it moves a hypothetically positioned root back onto
            // the page.
            // <https://www.w3.org/TR/css-position-3/#static-position>
            static_vertical_base
        };
        if absolute_static_position.is_none_or(|position| !position.has_vertical_position())
            && !inline_auto_static_y
            && vertical_insets_are_auto
            && let Some(offset) = self.block_static_position_y_offset
        {
            // CSS 2.2 defines the auto vertical static position from the
            // hypothetical normal-flow box. A block-level abspos appearing
            // after buffered inline content uses the line boxes that would
            // precede that hypothetical block:
            // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
            static_vertical_start = static_vertical_base + offset;
        }
        // CSS 2.2 centers a collapsed border on the table grid edge. The
        // outer half of each winning edge is nevertheless part of the table
        // wrapper border box, which the absolute-position equation uses.
        // Table grid sizing consumes the same half-widths exactly once when
        // converting a border-box `width` to its content/grid span.
        // <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
        let horizontal_non_content = style.padding.left
            + style.padding.right
            + positioned_border_widths.left
            + positioned_border_widths.right;
        let vertical_border_width_for_positioning =
            positioned_border_widths.top + positioned_border_widths.bottom;
        let positioned_height_percentage_basis =
            absolute_positioned_content_height_percentage_basis(
                style,
                containing_block,
                vertical_border_width_for_positioning,
            );
        if positioned_height_percentage_basis.is_definite() {
            self.block_percentage_context_stack
                .push_percentage_basis(positioned_height_percentage_basis);
        }
        let auto_or_intrinsic_width = replaced_content_size.map_or_else(
            || {
                if style.writing_mode.has_vertical_lines() && horizontal_size_was_auto {
                    // A physical `width:auto` is the logical block size of a
                    // vertical positioned block.  Its shrink-to-fit width is
                    // therefore the block contribution *after* fitting lines
                    // to its resolved logical inline measure. Generic
                    // intrinsic-width measurement only sees a glyph advance,
                    // which makes an abspos vertical block narrower than the
                    // equivalent in-flow or inline-block formatting context.
                    // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                    // <https://www.w3.org/TR/css-position-3/#abspos-layout>
                    self.used_block_physical_content_width(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        BlockContentWidthInputs {
                            available_outer_width: layout_pt(positioned_available_outer_width),
                            percentage_basis: PercentageBasis::definite(layout_pt(
                                containing_block.width(),
                            )),
                            horizontal_non_content: non_content_pt(horizontal_non_content),
                            definite_content_height: used_content_box_height_or_auto(
                                style,
                                layout_pt(containing_block.height()),
                                non_content_pt(
                                    style.padding.top
                                        + style.padding.bottom
                                        + vertical_border_width_for_positioning,
                                ),
                            )
                            .map(PhysicalContentHeight::new),
                            auto_width_role: BlockAutoWidthRole::PositionedShrinkToFit,
                        },
                    )
                    .points()
                } else {
                    self.used_intrinsic_or_shrink_to_fit_width(
                        element,
                        style,
                        stylesheets,
                        layout_pt(positioned_available_outer_width),
                        non_content_pt(horizontal_non_content),
                        child_boxes,
                        table_fragment,
                    )
                    .points()
                }
            },
            |(width, _)| width,
        );
        if positioned_height_percentage_basis.is_definite() {
            self.block_percentage_context_stack.pop();
        }
        let static_alignment_border_width = used_content_box_width_or_auto(
            style,
            layout_pt(containing_block.width()),
            non_content_pt(horizontal_non_content),
        )
        .map(|width| {
            constrain_content_width(
                style,
                width,
                PercentageBasis::definite(layout_pt(containing_block.width())),
            )
            .points()
        })
        .unwrap_or(auto_or_intrinsic_width)
            + horizontal_non_content;
        if horizontal_insets_are_auto
            && let Some(static_alignment) =
                absolute_static_position.and_then(AbsoluteStaticPosition::static_alignment)
        {
            let formatting_context_static_alignment = absolute_static_position
                .is_some_and(AbsoluteStaticPosition::has_formatting_context_static_alignment);
            static_horizontal_position = if static_alignment.inline.keyword
                == SelfAlignmentKeyword::Normal
                && !formatting_context_static_alignment
                && (style.position == Position::Fixed
                    || style.display.is_table()
                    || !style.abspos_static_source.is_inline_level())
            {
                // Ordinary-flow block rectangles retain both physical
                // inline-side distances. CSS 2 selects `left` or `right`
                // only later, from the static containing block's direction;
                // reducing this rectangle to its left edge moves RTL sources
                // outside their containing block.
                // <https://drafts.csswg.org/css-position-3/#resolving-auto-insets>
                // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
                let start = static_alignment.area.x() - containing_block.x();
                let end = containing_block.x() + containing_block.width()
                    - (static_alignment.area.x() + static_alignment.area.width());
                PhysicalStaticAxisFallback::new_unclamped(start, end)
            } else {
                static_alignment.horizontal_static_position(
                    containing_block,
                    static_alignment_border_width,
                    style.margin.left,
                    style.margin.right,
                )
            };
        }
        let containing_horizontal_direction = retained_static_rectangle
            .map(StaticPositionRectangle::css2_horizontal_direction)
            .unwrap_or_else(|| physical_horizontal_axis_direction(writing_mode, direction));
        // Static-position alignment is defined in the static-position
        // containing block's logical axes. When a physical inset pair is
        // explicit, its available area becomes the actual containing block,
        // but that does not retag `justify-self` and `align-self` with the
        // actual containing block's writing mode.
        // <https://drafts.csswg.org/css-align-3/#align-abspos>
        let final_alignment_container_axes = absolute_static_position
            .and_then(AbsoluteStaticPosition::static_alignment)
            .map(|alignment| (alignment.writing_mode, alignment.direction))
            .unwrap_or((
                self.containing_block_writing_mode,
                self.containing_block_direction,
            ));
        let mut positioned_x = resolve_absolute_horizontal_with_non_content(
            style,
            containing_block,
            auto_or_intrinsic_width,
            // An auto preferred size resolved from the opposite axis through
            // a preferred aspect ratio still has its normal content-based
            // automatic minimum. This must be applied after intrinsic width
            // measurement, rather than treating the ratio transfer as an
            // authored definite width.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            (horizontal_size_was_auto
                && style.box_values.min_width.is_auto()
                && !style.overflow_x.is_scrollable()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some())
            .then_some(auto_or_intrinsic_width),
            static_horizontal_position,
            containing_horizontal_direction,
            horizontal_non_content,
        );
        // When both physical insets are specified, CSS Box Alignment aligns
        // the subject inside the inset-modified containing block. This is a
        // final abspos alignment step, distinct from deriving a static inset
        // pair for a double-auto axis.
        // <https://drafts.csswg.org/css-align-3/#abspos-align>
        if left_inset.is_some()
            && right_inset.is_some()
            && style.justify_self.keyword != SelfAlignmentKeyword::Auto
        {
            let alignment = AbsposStaticAlignment::new(
                containing_block.rect,
                final_alignment_container_axes.0,
                final_alignment_container_axes.1,
                style.writing_mode,
                style.used_direction(),
                style.justify_self,
                style.align_self,
            );
            positioned_x.start = layout_pt(
                alignment
                    .horizontal_static_position(
                        containing_block,
                        static_alignment_border_width,
                        style.margin.left,
                        style.margin.right,
                    )
                    .left,
            );
        }
        let mut positioned_content_width = positioned_x.size.points();
        // Measure under the same out-of-flow named-page suppression as the
        // final positioned subtree, so page-name descendants cannot inflate
        // the measured fragment span:
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        self.push_page_name_scope_suppression();
        let auto_height = if !vertical_size_requires_content_measurement {
            // The vertical absolute-position equation has an authored used
            // size, so no automatic block-size probe can affect placement or
            // descendant percentage bases. Avoiding this speculative replay
            // is especially important for a clipped explicit-height subtree:
            // measuring its descendants as an artificial `height:auto` box
            // can recursively re-enter deferred positioned layout despite
            // the measurement not contributing to the final equation.
            // <https://www.w3.org/TR/css-position-3/#abspos-layout>
            content_box_pt(0.0)
        } else if let Some((_, height)) = replaced_content_size {
            content_box_pt(height)
        } else {
            let mut estimate_style = style.clone();
            estimate_style.position = Position::Static;
            estimate_style.margin = css::Edges::ZERO;
            estimate_style.box_values.margin =
                css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
            set_style_used_width(&mut estimate_style, positioned_content_width);
            // `positioned_content_width` is already a used content-box
            // size.  The measurement surrogate must not interpret it through
            // the authored `box-sizing` and subtract padding/borders again.
            // <https://www.w3.org/TR/css-sizing-3/#box-sizing>
            estimate_style.box_sizing = BoxSizing::ContentBox;
            set_style_auto_height(&mut estimate_style);
            clear_position_insets(&mut estimate_style);
            // Absolute-positioned automatic sizes use fit-content sizing on
            // their automatic axes. A vertical box's physical height is its
            // logical inline axis, so treating this as a normal-flow `auto`
            // height would stretch it through its vertical containing block
            // during the measurement pass.
            // <https://www.w3.org/TR/css-position-3/#abspos-layout>
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            if estimate_style.writing_mode.has_vertical_lines() {
                estimate_style
                    .box_values
                    .height
                    .replace_with_used(css::ComputedLengthPercentageOrAuto::FitContent(None));
            }
            let measured_height = self.measure_auto_positioned_block_height(
                element,
                &estimate_style,
                stylesheets,
                PositionedAutoBlockMeasurementSpace {
                    content_width: PhysicalContentWidth::new(content_box_pt(
                        positioned_content_width,
                    )),
                    available_physical_height: PhysicalContentHeight::new(content_box_pt(
                        containing_block.height(),
                    )),
                },
                child_boxes,
                table_fragment,
            );
            // Size-contained positioned boxes use the measured size of their
            // empty principal formatting context. A font-sized floor would
            // incorrectly reintroduce descendant/font intrinsic size.
            // In vertical writing, physical height is the logical inline
            // axis. Its automatic size is the measured inline contribution;
            // a logical block-axis line-height floor belongs to physical
            // width and would make a one-glyph abspos box too tall.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            if containment.size {
                content_box_pt(0.0)
            } else {
                // An empty block's automatic block size is zero. Line height
                // contributes only when inline content establishes a line
                // box; adding it here makes an empty abspos `stretch` subject
                // paint when an automatic inset correctly prevents stretching.
                // <https://drafts.csswg.org/css-position-3/#abspos-sizing>
                measured_height
            }
        };
        self.pop_page_name_scope_suppression();
        let static_alignment_border_height = used_content_box_height_or_auto(
            style,
            layout_pt(containing_block.height()),
            non_content_pt(
                style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
            ),
        )
        .map(|height| {
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(containing_block.height())),
            )
            .points()
        })
        .unwrap_or_else(|| auto_height.points())
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width_for_positioning;
        if vertical_insets_are_auto
            && let Some(static_alignment) =
                absolute_static_position.and_then(AbsoluteStaticPosition::static_alignment)
        {
            let formatting_context_static_alignment = absolute_static_position
                .is_some_and(AbsoluteStaticPosition::has_formatting_context_static_alignment);
            static_vertical_start = if static_alignment.block.keyword
                == SelfAlignmentKeyword::Normal
                && !formatting_context_static_alignment
                && WritingModeAxes::new(static_alignment.writing_mode, static_alignment.direction)
                    .logical_axis_for_physical(PhysicalAxis::Vertical)
                    == LogicalAxis::Block
                && (style.position == Position::Fixed
                    || style.display.is_table()
                    || (!style.abspos_static_source.is_inline_level()
                        && style.used_direction() == Direction::Ltr))
            {
                // As on the horizontal axis, an ordinary-flow normal static
                // rectangle is already pinned to its hypothetical start
                // edge. Let the absolute-position equation apply the margin
                // once rather than adding it while deriving that edge.
                // <https://drafts.csswg.org/css-position-3/#resolving-auto-insets>
                containing_block.top_y() - static_alignment.area.top_y()
            } else {
                static_alignment.vertical_static_start(
                    containing_block,
                    static_alignment_border_height,
                    style.margin.top,
                    style.margin.bottom,
                )
            };
        }
        let positioned_y = resolve_absolute_vertical(
            style,
            containing_block,
            auto_height,
            // See the corresponding inline-axis minimum above. The block
            // automatic minimum is the measured in-flow content height.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            (vertical_size_was_auto
                && style.box_values.min_height.is_auto()
                && !style.overflow_y.is_scrollable()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some())
            .then_some(auto_height.points()),
            static_vertical_start,
            vertical_border_width_for_positioning,
        );
        let mut positioned_y = positioned_y;
        if top_inset.is_some()
            && bottom_inset.is_some()
            && style.align_self.keyword != SelfAlignmentKeyword::Auto
        {
            let alignment = AbsposStaticAlignment::new(
                containing_block.rect,
                final_alignment_container_axes.0,
                final_alignment_container_axes.1,
                style.writing_mode,
                style.used_direction(),
                style.justify_self,
                style.align_self,
            );
            positioned_y.start = layout_pt(alignment.vertical_static_start(
                containing_block,
                static_alignment_border_height,
                style.margin.top,
                style.margin.bottom,
            ));
        }
        let positioned_content_height = positioned_y.size.points();
        // A min/max constraint on an automatic block size transfers through
        // the preferred aspect ratio to constrain the automatic inline size.
        // The absolute-position equations are solved independently, so feed
        // the final constrained block result back into the inline equation
        // once both authored preferred sizes were automatic.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio-size-transfers>
        if horizontal_size_was_auto
            && vertical_size_was_auto
            && let Some(unconstrained_height) = non_replaced_aspect_ratio_content_height(
                style,
                positioned_content_width,
                horizontal_non_content,
                style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
            )
            && (positioned_content_height - unconstrained_height).abs() > 0.01
            && let Some(transferred_width) = non_replaced_aspect_ratio_content_width(
                style,
                positioned_content_height,
                horizontal_non_content,
                style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
            )
        {
            let constrained_width = if positioned_content_height < unconstrained_height {
                positioned_content_width.min(transferred_width)
            } else {
                positioned_content_width.max(transferred_width)
            };
            if (constrained_width - positioned_content_width).abs() > 0.01 {
                let mut ratio_constrained_style = style.clone();
                set_style_used_width(&mut ratio_constrained_style, constrained_width);
                ratio_constrained_style.box_sizing = BoxSizing::ContentBox;
                positioned_x = resolve_absolute_horizontal_with_non_content(
                    &ratio_constrained_style,
                    containing_block,
                    auto_or_intrinsic_width,
                    None,
                    static_horizontal_position,
                    containing_horizontal_direction,
                    horizontal_non_content,
                );
                positioned_content_width = positioned_x.size.points();
            }
        }
        let fragmentainer_axes = FlowAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        );
        let positioned_margin_box = FragmentainerBlockMarginBox::new(PageTopRect::new(
            containing_block.x() + positioned_x.start.points(),
            containing_block.top_y() - positioned_y.start.points(),
            positioned_x.margin_start.points()
                + positioned_content_width
                + horizontal_non_content
                + positioned_x.margin_end.points(),
            positioned_y.margin_start.points()
                + positioned_y.size.points()
                + style.padding.top
                + style.padding.bottom
                + vertical_border_width_for_positioning
                + positioned_y.margin_end.points(),
        ));
        let (positioned_page_offset, positioned_fragmentainer_remainder) = if style.position
            == Position::Absolute
            && !uses_escaped_atom_outer_containing_block
        {
            // A block-level static-position rectangle at a fragmentainer's
            // block-end edge starts in the following fragmentainer, just as
            // its in-flow hypothetical box would. Keeping it on the prior
            // page overlaps the final in-flow fragment and leaves a trailing
            // empty absolute-positioned continuation.
            // <https://drafts.csswg.org/css-position-3/#static-position>
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>
            self.absolute_positioned_page_start_offset(positioned_margin_box, fragmentainer_axes)
        } else {
            (0, 0.0)
        };
        let positioned_origin_page_index =
            containing_block_origin_page_index + positioned_page_offset;
        let positioned_border_box_width = positioned_content_width + horizontal_non_content;
        self.content_left =
            containing_block.x() + positioned_x.start.points() + positioned_x.margin_start.points();
        self.content_right = self.content_left + positioned_border_box_width;
        match fragmentainer_axes.block_start_side() {
            // The existing block-layout scratch coordinate grows from the
            // physical page top. Preserve that continuous-Y transport for a
            // horizontal principal flow; its destination-page translation is
            // applied below.
            PhysicalSide::Top | PhysicalSide::Bottom => {}
            PhysicalSide::Left => {
                self.content_left = self.page_left()
                    + positioned_fragmentainer_remainder
                    + positioned_x.margin_start.points();
                self.content_right = self.content_left + positioned_border_box_width;
            }
            PhysicalSide::Right => {
                self.content_right = self.current_page_context.right()
                    - positioned_fragmentainer_remainder
                    - positioned_x.margin_end.points();
                self.content_left = self.content_right - positioned_border_box_width;
            }
        }
        // A direct atomic-inline placeholder is already anchored at its
        // hypothetical margin-box block start. A formatting-context or
        // ordinary static-alignment payload, however, supplies the same
        // margin-box inset to the absolute-position equation and therefore
        // still needs the one normal margin-to-border conversion here.
        let static_alignment_uses_margin_box_inset = absolute_static_position
            .and_then(AbsoluteStaticPosition::static_alignment)
            .is_some();
        let positioned_margin_top = if inline_auto_static_y
            && style.abspos_static_source.is_atomic_inline()
            && !static_alignment_uses_margin_box_inset
        {
            0.0
        } else {
            positioned_y.margin_start.points()
        };
        self.cursor_y =
            containing_block.top_y() - positioned_y.start.points() - positioned_margin_top;
        log::trace!(
            target: "spindrift::layout::static_position",
            "checkpoint=resolved-box element={:?} css2_horizontal_direction={:?} text_indent={:.2} positioned=(x_start:{:.2},x_margin:{:.2},y_start:{:.2},y_margin:{:.2},content_width:{:.2},content_height:{:.2}) page_border_box=(x:{:.2},top:{:.2},width:{:.2},height:{:.2})",
            element.id,
            containing_horizontal_direction,
            style.text_indent.amount.length_points(),
            positioned_x.start.points(),
            positioned_x.margin_start.points(),
            positioned_y.start.points(),
            positioned_margin_top,
            positioned_content_width,
            positioned_content_height,
            self.content_left,
            self.cursor_y,
            positioned_border_box_width,
            positioned_content_height + style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
        );
        // Enter the destination fragmentainer's local coordinate space before
        // laying out the positioned subtree. Keeping a continuous coordinate
        // below the source page and translating captured paint afterward loses
        // exact-boundary placement and gives descendants the wrong containing
        // block geometry.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        if positioned_page_offset > 0
            && matches!(
                fragmentainer_axes.block_start_side(),
                PhysicalSide::Top | PhysicalSide::Bottom
            )
        {
            self.cursor_y += positioned_page_offset as f32 * self.page_area_height().max(1.0);
        }
        let mut positioned_flow_margin_box_top = self.cursor_y;
        let positioned_border_box_height = positioned_content_height
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width_for_positioning;
        let positioned_table_sizing =
            (style.display.is_table() && table_fragment.is_some()).then(|| {
                positioned_table_sizing_for_geometry(
                    source_style,
                    style,
                    containing_block,
                    PhysicalContentWidth::new(content_box_pt(positioned_content_width)),
                    PhysicalContentHeight::new(content_box_pt(positioned_content_height)),
                )
            });
        let mut positioned_border_box = PageTopRect::new(
            self.content_left,
            self.cursor_y,
            positioned_border_box_width,
            positioned_border_box_height,
        )
        .paint_clip();
        // CSS Positioned Layout and CSS 2.2 Appendix E order positioned
        // boxes in tree order in their containing stacking context. Reserve
        // this box's order before laying out descendants so child positioned
        // contexts, including fixed descendants, sort after their parent.
        let positioned_source_order = self.next_paint_source_order();

        let mut flow_style = style.clone();
        flow_style.position = Position::Static;
        // Absolute and fixed boxes establish an independent formatting
        // context. The flow surrogate removes their positioning only to
        // replay their already-resolved geometry; it must retain that
        // formatting-context boundary so in-flow child margins cannot
        // collapse through the principal box.
        // <https://drafts.csswg.org/css-position-3/#position-property>
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        if flow_style.display.inner == css::DisplayInner::Flow {
            flow_style.display = flow_style.display.with_inner(css::DisplayInner::FlowRoot);
        }
        // The blockified flow surrogate has the final used padding box, so it
        // owns static scroll capture for this positioned scroll container.
        // The surrogate is laid out at its static-flow origin solely to
        // capture the positioned principal's contents.  It is not a second
        // CSS box, so it must not contribute a duplicate snap area at that
        // temporary origin.
        flow_style.scroll_snap_align = css::ScrollSnapAlign::default();
        flow_style.margin = css::Edges::ZERO;
        flow_style.box_values.margin =
            css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
        set_style_used_width(&mut flow_style, positioned_content_width);
        set_style_used_height(&mut flow_style, positioned_content_height);
        // Positioned-axis resolution supplies content-box sizes.  The static
        // flow surrogate replays those used values, so it must retain their
        // content-box space instead of reapplying an authored border-box
        // conversion.
        // <https://www.w3.org/TR/css-sizing-3/#box-sizing>
        flow_style.box_sizing = BoxSizing::ContentBox;
        clear_position_insets(&mut flow_style);
        // The surrogate is already a used style.  It re-enters the ordinary
        // formatting-context dispatcher only to lay out its descendants, so
        // leave no effective zoom for that dispatcher to apply to this box
        // again. Descendant cascade continues to use the frozen child tree,
        // never this transport style.
        flow_style.effective_zoom = css::EffectiveZoom::NORMAL;
        let positioned_containing_block_top = self.cursor_y - positioned_border_widths.top;
        let positioned_containing_block = self.positioned_containing_block_context(
            ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left + positioned_border_widths.left,
                positioned_containing_block_top,
                positioned_content_width + flow_style.padding.left + flow_style.padding.right,
                positioned_content_height + flow_style.padding.top + flow_style.padding.bottom,
            ))
            .on_page(positioned_origin_page_index),
        );
        self.containing_blocks.push(positioned_containing_block);
        // A statically positioned absolute box can fragment its contents
        // through later page contexts. An explicitly block-start-pinned
        // absolute box, and every fixed box, instead belongs to one resolved
        // out-of-flow placement and must not let descendant `page` values
        // manufacture normal-flow page transitions.
        // <https://drafts.csswg.org/css-position-3/#fragmentation>
        let suppress_descendant_page_name_transitions = style.position == Position::Fixed
            || (style.position == Position::Absolute && !vertical_insets_are_auto);
        if suppress_descendant_page_name_transitions {
            self.push_page_name_scope_suppression();
        }
        let previous_overflow_clips = self.overflow_clips.clone();
        self.overflow_clips =
            positioned_applicable_overflow_clips(&previous_overflow_clips, containing_block);
        let fragmentainer_block_start_side = fragmentainer_axes.block_start_side();
        let fragmentainer_block_start = match fragmentainer_block_start_side {
            PhysicalSide::Top => self.page_top(),
            PhysicalSide::Bottom => self.page_bottom(),
            PhysicalSide::Left => self.page_left(),
            PhysicalSide::Right => self.current_page_context.right(),
        };
        let fragmentainer_block_size = layout_pt(
            self.current_page_context
                .logical_block_size(self.principal_flow.writing_mode),
        );
        let positioned_paint_reach =
            PositionedPaintReach::from_overflow_clips(&self.overflow_clips, fragmentainer_axes);
        // A non-scrollable clip is the one case that needs the principal span
        // before scratch layout: it supplies the scratch-page ceiling. A
        // potentially visible principal starts with no speculative tail and
        // gains a continuous span only after layout establishes an actual
        // decoration, fixed descendant, or semantic-content obligation.
        // <https://drafts.csswg.org/css-position/#abspos-breaking>
        let span_start_page_index = containing_block_origin_page_index.saturating_sub(
            if containing_block_origin_page_index > 0 {
                positioned_page_offset
            } else {
                0
            },
        );
        let span_start_progress = positioned_margin_box
            .start_distance_from(fragmentainer_block_start, fragmentainer_block_start_side)
            .max(0.0);
        let mut principal_page_span_target = (style.position == Position::Absolute
            && matches!(positioned_paint_reach, PositionedPaintReach::Clipped { .. }))
        .then(|| {
            self.absolute_positioned_page_span_target(
                style,
                positioned_margin_box,
                fragmentainer_axes,
                span_start_page_index,
                span_start_progress,
            )
        })
        .flatten();
        let mut positioned_fragmentation_plan = if style.position == Position::Absolute {
            PositionedFragmentationPlan::for_absolute_box(
                positioned_origin_page_index,
                principal_page_span_target,
                fragmentainer_block_start,
                fragmentainer_block_start_side,
                fragmentainer_block_size,
                positioned_paint_reach,
            )
        } else {
            PositionedFragmentationPlan::for_absolute_box(
                paint_page_index,
                None,
                fragmentainer_block_start,
                fragmentainer_block_start_side,
                fragmentainer_block_size,
                PositionedPaintReach::PotentiallyVisible,
            )
        };
        let positioned_paint_transaction = PositionedPaintTransaction::begin(
            self,
            positioned_fragmentation_plan.scratch_page_limit(),
        );
        if style.position == Position::Absolute
            && positioned_page_offset > 0
            && self.positioned_paint_transaction_depth == 1
        {
            // The positioned box's static rectangle starts on a later
            // destination page. Its physical inset remains in the continuous
            // containing block, while its first scratch fragment must use the
            // selected destination page's page area (which may have different
            // margins or dimensions).
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            // <https://www.w3.org/TR/css-page-3/#page-model>
            let destination_context =
                self.resolved_page_context(positioned_origin_page_index + 1, false);
            if destination_context != self.current_page_context {
                self.positioned_scratch_page_origin =
                    Some(DocumentPageIndex::new(positioned_origin_page_index));
                self.rebase_positioned_scratch_page_context(destination_context);

                if matches!(fragmentainer_axes.block_start_side(), PhysicalSide::Top) {
                    // Physical top coordinates are page-local. The static
                    // rectangle reached the exact end of its source
                    // fragmentainer, so restart at the destination page's
                    // physical block start rather than carrying the prior page's
                    // shorter area into its cursor.
                    self.cursor_y =
                        self.current_page_context.top() - positioned_fragmentainer_remainder;
                    positioned_flow_margin_box_top = self.cursor_y;
                    positioned_border_box = PageTopRect::new(
                        self.content_left,
                        self.cursor_y,
                        positioned_border_box_width,
                        positioned_border_box_height,
                    )
                    .paint_clip();
                    let positioned_containing_block_top =
                        self.cursor_y - positioned_border_widths.top;
                    self.containing_blocks
                        .last_mut()
                        .expect("positioned flow owns its containing block")
                        .geometry = ContainingBlock::from_page_top_rect(PageTopRect::new(
                        self.content_left + positioned_border_widths.left,
                        positioned_containing_block_top,
                        positioned_content_width
                            + flow_style.padding.left
                            + flow_style.padding.right,
                        positioned_content_height
                            + flow_style.padding.top
                            + flow_style.padding.bottom,
                    ))
                    .on_page(positioned_origin_page_index);
                }
            }
        }
        // A positioned descendant is not emitted through normal-flow block
        // layout, so record its final border-box geometry only after its
        // scratch fragmentainer has selected the destination page context.
        // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
        self.record_static_scroll_snap_area(element, style, positioned_border_box.paint_rect());
        self.record_static_scroll_target_area(
            element.is_target,
            positioned_border_box.paint_rect(),
            style,
        );
        let previous_fragmentainer_override = self.fragmentainer_override;
        // The absolute containing block above stays continuous and remains
        // the percentage basis for the positioned box. Its *contents*,
        // however, fragment into the destination page sequence. Do not pin
        // scratch fragmentainers to the source page context: each generated
        // destination page can have a different page area and must rebase
        // local available sizes before laying out its continuation.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-abspos>
        // <https://drafts.csswg.org/css-break-4/#varying-size-fragmentainers>
        let previous_defer_block_decoration_promotion = self.defer_next_block_decoration_promotion;
        // The positioned stacking context owns its principal decoration. Keep
        // that decoration in the background/border band so overflow and paint
        // containment can clip only the captured contents below.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
        self.defer_next_block_decoration_promotion = true;
        // Absolutely positioned block containers establish a new block
        // formatting context. In particular, float exclusions from the source
        // formatting context cannot constrain either this box's line widths or
        // its auto block-size; floats created inside remain local to it.
        // <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>
        self.push_float_context();
        if let Some(sizing) = positioned_table_sizing {
            self.push_positioned_table_sizing(sizing);
        }
        let positioned_flow_origin = PositionedFlowOrigin::from_resolved_positioned_box(
            PageTopRect::new(
                self.content_left,
                positioned_flow_margin_box_top,
                positioned_content_width,
                positioned_content_height,
            ),
            // An explicit physical inset has already resolved the surrogate's
            // border-box origin.  Only a static inline position is produced
            // from its hypothetical content insertion edge and needs the
            // block-flow non-content bridge on replay.
            // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
            if horizontal_insets_are_auto && style.abspos_static_source.is_inline_level() {
                positioned_border_widths.left
                    + positioned_border_widths.right
                    + flow_style.padding.left
                    + flow_style.padding.right
            } else {
                0.0
            },
        );
        self.content_left = positioned_flow_origin.content_left;
        self.content_right = positioned_flow_origin.content_right;
        self.cursor_y = positioned_flow_origin.cursor_y;
        log::trace!(
            target: "spindrift::layout::static_position",
            "checkpoint=positioned-flow-origin element={:?} content=(x:{:.2},top:{:.2},bottom:{:.2},width:{:.2},height:{:.2}) axes=({:?},{:?}) inline_start={:?} cursor_y={:.2}",
            element.id,
            self.content_left,
            positioned_flow_margin_box_top,
            positioned_flow_margin_box_top - positioned_content_height,
            positioned_content_width,
            positioned_content_height,
            style.writing_mode,
            style.used_direction(),
            WritingModeAxes::new(style.writing_mode, style.used_direction())
                .physical_side(LogicalSide::InlineStart),
            self.cursor_y,
        );
        // The positioned stacking context assembled below owns the principal
        // transform, opacity, and clipping effects for this box.  Re-entering
        // the generic element dispatcher with its normal effect capture would
        // wrap the same principal fragment a second time, applying (for
        // example) a CSS transform twice.  Descendants still capture their
        // own effects normally.
        //
        // CSS Transforms § 3 makes the transformed element one stacking
        // context; it does not create a nested second context for its own
        // principal box.
        self.layout_element_inner_with_principal_effect_context(
            element,
            &flow_style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
            false,
            PrincipalBoxPaintMode::RootPaints,
            Some(if positioned_height_percentage_basis.is_definite() {
                DescendantBlockPercentageContext::from_percentage_basis(
                    positioned_height_percentage_basis,
                )
            } else if vertical_size_requires_content_measurement {
                DescendantBlockPercentageContext::ContentSized
            } else {
                DescendantBlockPercentageContext::Indefinite
            }),
        );
        self.fragmentainer_override = previous_fragmentainer_override;
        log::trace!(
            target: "spindrift::layout::static_position",
            "checkpoint=positioned-flow-outcome element={:?} block_outcome={:?}",
            element.id,
            self.last_block_layout_outcome,
        );
        if positioned_table_sizing.is_some() {
            self.positioned_table_sizing.pop();
        }
        self.pop_float_context();
        self.defer_next_block_decoration_promotion = previous_defer_block_decoration_promotion;
        self.overflow_clips = previous_overflow_clips;
        if suppress_descendant_page_name_transitions {
            self.pop_page_name_scope_suppression();
        }
        self.containing_blocks.pop();
        let mut child_positioned_layers = if positioned_layer_start < self.positioned_layers.len() {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let has_principal_decoration = style.visibility == Visibility::Visible
            && (style.background.background_color.is_potentially_visible()
                || style.background.background_image.is_image()
                || style.border_image.source.is_image()
                || used_border_width(style) > layout_pt(0.0)
                || (style.outline_width > 0.0 && !style.outline_style.suppresses_used_width()));
        let viewport_fixed_descendant =
            retained_viewport_fixed_descendant || self.fixed_layers.len() > fixed_layer_start;
        // An in-flow DOM or generated-content subtree remains part of the
        // fragmented box even when the captured principal paint is empty (for
        // example through `visibility: hidden`). In contrast, a structurally
        // empty, transparent absolute box is eligible for Paged Media's
        // content-empty-page avoidance. An absolutely positioned descendant
        // does not count here: it owns a separate fragmentation plan.
        // <https://www.w3.org/TR/css-page-3/#renderingpages>
        // <https://drafts.csswg.org/css-break-4/#abspos-breaking>
        let direct_semantic_text = element.children.iter().any(|child| {
            matches!(&child.kind, crate::dom::NodeKind::Text(text) if !text.trim().is_empty())
        });
        let semantically_nonempty_content = style.content.is_generated()
            || direct_semantic_text
            || (style.visibility != Visibility::Visible
                && child_boxes.is_some_and(has_semantically_nonempty_in_flow_content));
        let principal_page_span_obligation = PrincipalPageSpanObligation::for_positioned_principal(
            has_principal_decoration,
            viewport_fixed_descendant,
            semantically_nonempty_content,
        );
        let scroll_padding_box = paint_space_rect(
            positioned_border_box.x() + positioned_border_widths.left,
            positioned_border_box.y() + positioned_border_widths.bottom,
            (positioned_border_box.width()
                - positioned_border_widths.left
                - positioned_border_widths.right)
                .max(0.0),
            (positioned_border_box.height()
                - positioned_border_widths.top
                - positioned_border_widths.bottom)
                .max(0.0),
        );
        // A positioned box without a z-index is still the clipping owner for
        // its positioned descendants when both *used* overflow axes clip.
        // Root/body overflow propagated to the viewport becomes `visible` on
        // the source element, so this reconstruction must not read the
        // computed longhands directly.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        // <https://www.w3.org/TR/css-overflow-3/#overflow-propagation>
        let used_positioned_overflow = self.used_overflow_axes_for_element(element, style);
        // A replaced principal box is captured as one atomic paint fragment,
        // containing both its CSS decoration and its concrete object. The
        // concrete-object painter applies the used content-edge clip to the
        // image/SVG itself; applying this generic fragment clip would instead
        // classify the whole atom as contents and cut off its border-image
        // outset. Non-replaced positioned boxes still use the normal
        // descendant-only padding-box scope below.
        // <https://drafts.csswg.org/css-overflow-3/#overflow-clipping>
        // <https://drafts.csswg.org/css-backgrounds-3/#border-image-outset>
        let positioned_contents_clip = (!is_replaced_element(element)
            && used_positioned_overflow.clips_x()
            && used_positioned_overflow.clips_y())
        .then_some(scroll_padding_box);
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_padding_box,
        );
        // A positioned principal's scratch paint has one escape route: the
        // transaction drains it, restores the parent page sequence, and
        // returns non-cloneable scratch fragments. This must happen before
        // any final positioned layer is assembled.
        let captured_paint = positioned_paint_transaction.capture_and_restore(self);
        let initial_page_context = captured_paint.initial_page_context;
        if style.position == Position::Absolute
            && principal_page_span_obligation.requires_continuous_principal_span()
            && principal_page_span_target.is_none()
        {
            principal_page_span_target = self.absolute_positioned_page_span_target(
                style,
                positioned_margin_box,
                fragmentainer_axes,
                span_start_page_index,
                span_start_progress,
            );
            positioned_fragmentation_plan = PositionedFragmentationPlan::for_absolute_box(
                positioned_origin_page_index,
                principal_page_span_target,
                fragmentainer_block_start,
                fragmentainer_block_start_side,
                fragmentainer_block_size,
                PositionedPaintReach::PotentiallyVisible,
            );
        }
        let mut positioned_replay = if style.position == Position::Absolute {
            // Preserve every captured slice when moving scratch pagination to
            // the absolute box's destination sequence. A slice can contain
            // only background, border, or another non-text paint primitive;
            // using the first text baseline to choose a remapping origin
            // drops those observable fragments.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            captured_paint.into_final_absolute(AbsoluteFragmentPlacement::new(
                0,
                // Scratch pagination starts at the containing formatting
                // context's source fragment. An explicitly inset nested box
                // enters a destination-local fragmentainer before its own
                // scratch sequence begins, so retain that additional source
                // to destination displacement on replay. A static-positioned
                // box's resolved rectangle already owns the source fragment
                // and must not add it again.
                // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
                positioned_origin_page_index
                    + if containing_block_origin_page_index > 0 && !vertical_insets_are_auto {
                        positioned_page_offset
                    } else {
                        0
                    },
            ))
        } else {
            captured_paint.into_final_same_pages()
        };
        let mut positioned_fragments = std::mem::take(&mut positioned_replay.fragments);
        // An explicit-size positioned principal with no in-flow descendants
        // can contribute only its own decoration. Its continuous margin-box
        // span is therefore an exact paint bound, unlike a box whose
        // overflowing descendants may legitimately reach later pages.
        if style.position == Position::Absolute
            && !style.box_values.height.is_auto()
            // A missing formatting-box slice is not proof that the element
            // has no in-flow descendants: ordinary text and anonymous boxes
            // are represented directly from the DOM at this boundary.
            && element.children.is_empty()
            && let Some(principal_end) =
                positioned_fragmentation_plan.materialized_destination_end()
        {
            positioned_fragments
                .retain(|fragment| fragment.destination_page().get() <= principal_end);
            positioned_replay.retain_effects_through(DocumentPageIndex::new(principal_end));
        }
        let observed_positioned_target_page_index = positioned_fragments
            .iter()
            .filter(|fragment| !fragment.fragment().is_empty())
            .map(|fragment| fragment.destination_page().get())
            .chain(positioned_replay.effect_pages().map(DocumentPageIndex::get))
            .max();
        // A principal's own page span is not an output bound for independently
        // positioned descendants. In particular, a transparent absolute box
        // can own a nested absolute descendant whose resolved destination is
        // later than the parent principal. Only a non-scrollable overflow
        // clip proves that such a tail cannot paint.
        // <https://drafts.csswg.org/css-position/#abspos-breaking>
        if matches!(positioned_paint_reach, PositionedPaintReach::Clipped { .. })
            && let Some(materialized_end) =
                positioned_fragmentation_plan.materialized_destination_end()
        {
            positioned_fragments
                .retain(|fragment| fragment.destination_page().get() <= materialized_end);
            child_positioned_layers.retain(|layer| layer.page_index <= materialized_end);
            positioned_replay.retain_effects_through(DocumentPageIndex::new(materialized_end));
        }
        if has_principal_decoration
            && let Some(target_page_index) = if style.box_values.height.is_auto() {
                observed_positioned_target_page_index
            } else {
                positioned_fragmentation_plan.materialized_destination_end()
            }
        {
            let captured_last_page_index = positioned_fragments
                .iter()
                .map(|fragment| fragment.destination_page().get())
                .max()
                .unwrap_or(paint_page_index);
            self.extend_positioned_principal_decoration_fragments(
                &mut positioned_fragments,
                style,
                positioned_border_box,
                paint_page_index,
                captured_last_page_index,
                target_page_index,
                initial_page_context,
            );
        }
        if static_scroll_offset.x != 0.0 || static_scroll_offset.y != 0.0 {
            let translation =
                crate::layout::scroll_snap::static_scroll_translation(static_scroll_offset, style);
            for fragment in &mut positioned_fragments {
                *fragment.fragment_mut() = fragment.fragment().clone().translated(translation);
            }
            for layer in &mut child_positioned_layers {
                *layer = layer.clone().translated(translation);
            }
        }
        let mut child_layers_by_page =
            PendingPageLocalLayers::from_positioned_layers(child_positioned_layers);
        let child_layer_pages = child_layers_by_page.page_indices().collect::<Vec<_>>();
        for page_index in child_layer_pages {
            if !positioned_fragments
                .iter()
                .any(|fragment| fragment.destination_page() == page_index)
            {
                positioned_fragments.push(FinalPositionedFragment::empty(page_index));
            }
        }
        let target_page_index = if style.position == Position::Absolute {
            positioned_fragments
                .iter()
                .filter(|fragment| !fragment.fragment().is_empty())
                .map(|fragment| fragment.destination_page().get())
                .chain(
                    child_layers_by_page
                        .page_indices()
                        .map(DocumentPageIndex::get),
                )
                .chain(positioned_replay.effect_pages().map(DocumentPageIndex::get))
                .chain(
                    principal_page_span_obligation
                        .requires_continuous_principal_span()
                        .then_some(positioned_fragmentation_plan.materialized_destination_end())
                        .flatten(),
                )
                .max()
        } else {
            None
        };
        let target_page_index = if style.box_values.height.is_auto() {
            observed_positioned_target_page_index
        } else {
            target_page_index
        };
        // A retry lays this subtree out in scratch fragmentainers after its
        // viewport-fixed descendants have already been committed. The retry
        // must not promote its scratch continuation beyond the principal
        // margin-box span into fresh document pages.
        let target_page_index =
            if retained_viewport_fixed_descendant && !style.box_values.height.is_auto() {
                match (target_page_index, principal_page_span_target) {
                    (Some(target), Some(principal)) => Some(target.min(principal)),
                    (target, _) => target,
                }
            } else {
                target_page_index
            };
        positioned_fragmentation_plan = if style.box_values.height.is_auto() {
            // Auto-height principals have no independently specified
            // fragment span; materialize only the fragments actually
            // produced by their continuous formatting context.
            positioned_fragmentation_plan.with_observed_destination_end(target_page_index)
        } else {
            positioned_fragmentation_plan.with_materialized_destination_end(target_page_index)
        };
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.inline_static_position = previous_inline_static_position;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.retain_absolute_positioned_page_span(principal_page_span_target);
        self.ensure_positioned_page_span(positioned_fragmentation_plan);

        // A positioned box owns its principal decoration even when its
        // contained formatting context contributes no fragment. This occurs
        // for example when size containment suppresses the only in-flow
        // descendant. Do not let that empty content result suppress a visible
        // background, border, or outline.
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if positioned_fragments
            .iter()
            .all(|fragment| fragment.fragment().is_empty())
            && child_layers_by_page.is_empty()
            && style.visibility == Visibility::Visible
            && has_principal_decoration
        {
            let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                self.box_background_primitives(
                    paint_space_rect(
                        positioned_border_box.x(),
                        positioned_border_box.y(),
                        positioned_border_box.width(),
                        positioned_border_box.height(),
                    ),
                    style,
                ),
            );
            fragment.append_primitives_in_band(
                PaintBand::Outline,
                self.box_outline_primitives(
                    paint_space_rect(
                        positioned_border_box.x(),
                        positioned_border_box.y(),
                        positioned_border_box.width(),
                        positioned_border_box.height(),
                    ),
                    style,
                ),
            );
            if !fragment.is_empty() {
                // An empty scratch page is only a capture placeholder.  It
                // must not prevent the positioned principal's own border or
                // background from reaching its parent stacking context.
                positioned_fragments.clear();
                positioned_fragments.push(FinalPositionedFragment::new(
                    DocumentPageIndex::new(positioned_origin_page_index),
                    fragment,
                ));
            }
        }

        if positioned_fragments
            .iter()
            .all(|fragment| fragment.fragment().is_empty())
            && child_layers_by_page.is_empty()
            && !positioned_replay.has_effects()
        {
            return;
        }
        // Semantic effects have crossed the same typed scratch-to-document
        // projection as the paint above. Queue them before consuming the
        // page-local layers so an effect-only positioned destination is kept
        // materializable by the normal deferred-page machinery.
        self.apply_deferred_layout_side_effects(
            positioned_replay.into_deferred_layout_side_effects(),
        );
        for mut positioned_fragment in positioned_fragments {
            let destination_page = positioned_fragment.destination_page();
            let page_index = destination_page.get();
            if style.position == Position::Absolute
                && !style.box_values.height.is_auto()
                && retained_viewport_fixed_descendant
                && principal_page_span_target.is_some_and(|target| page_index > target)
            {
                continue;
            }
            let mut child_layers = child_layers_by_page.drain_for_page(destination_page);
            let mut links = positioned_fragment.links().to_vec();
            let bounds = positioned_border_box;
            if containment.size {
                positioned_fragment =
                    positioned_fragment.with_monolithic_fragmentation_scope(bounds);
            }
            let mut policy = StackingContextPolicy::for_positioned(element, style, bounds);
            if is_replaced_element(element) {
                // A replaced element's concrete-object painter owns the clip
                // for `object-fit` and used overflow. Its principal
                // decoration is nevertheless CSS box paint, so an outer
                // positioned-box overflow clip must not cut off its
                // background, border, or border-image outset ink.
                // <https://drafts.csswg.org/css-overflow-3/#overflow>
                // <https://drafts.csswg.org/css-backgrounds-3/#border-image-outset>
                policy.effects.clear_overflow_clip_effects();
            }
            if self
                .document_canvas_overflow
                .is_viewport_overflow_source(element)
            {
                // Propagated root/body overflow is applied to the viewport;
                // its source box has used `overflow: visible` and must not
                // retain a local clipping effect when positioned.
                // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
                policy.effects.clear_overflow_clip_effects();
            }
            if uses_initial_page_containing_block
                && !uses_escaped_atom_outer_containing_block
                && style.position != Position::Fixed
            {
                // A collapsed table's resolved wrapper border box can extend
                // into page margins through its outer half-borders and
                // negative margins. Its positioned paint therefore clips to
                // the physical page box, not the initial containing block's
                // smaller page area. Other positioned boxes retain the
                // established containing-block clip: several named-page
                // continuation paths replay their source geometry against
                // that page-area coordinate space.
                //
                // Fixed descendants instead replay over the final page
                // sequence, whose media boxes provide the page-local clip.
                // Retaining this initial-page clip would hide a fixed layer
                // in the extra area of a later differently sized named page.
                // Add the absolute-positioned clip only if this box reaches
                // outside the page. Applying an identical clip to a fully
                // contained opaque rectangle introduces a second antialiased
                // edge in PDF rasterizers, despite no overflow needing
                // containment.
                //
                // An escaped atomic-inline descendant is replayed from its
                // scratch formatting context into the source atom's final
                // page. Its page clip must be owned by that destination
                // replay, not translated from the scratch page together with
                // the layer; the final page itself provides the page-area
                // boundary in the meantime.
                // <https://www.w3.org/TR/css-page-3/#page-model>
                let page_clip = if collapsed_table_wrapper_insets.is_some() {
                    let page = self.pages.get(page_index).unwrap_or(&self.current_page);
                    PageTopRect::new(0.0, page.height(), page.width(), page.height()).paint_clip()
                } else {
                    PageTopRect::new(
                        containing_block.x(),
                        containing_block.top_y(),
                        containing_block.width(),
                        containing_block.height(),
                    )
                    .paint_clip()
                };
                let box_overflows_page = bounds.x() < page_clip.x()
                    || bounds.y() < page_clip.y()
                    || bounds.x() + bounds.width() > page_clip.x() + page_clip.width()
                    || bounds.y() + bounds.height() > page_clip.y() + page_clip.height();
                if box_overflows_page {
                    policy
                        .effects
                        .set_rectangular_overflow_clip(Some(page_clip));
                }
            }
            if positioned_fragment.contains_overflow_clip()
                && policy
                    .effects
                    .overflow_clip_bounds()
                    .is_some_and(|clip| clip.width() <= 0.0 || clip.height() <= 0.0)
            {
                // The measured formatting context already owns this empty
                // used padding-box clip. Applying the reconstructed empty clip
                // around that nested context would also erase the principal
                // decoration, which lies outside the contents clip.
                policy.effects.clear_overflow_clip_effects();
            }
            let escaped_atom_replay = if self.escaped_atom_positioning_depth > 0 {
                let containing_block_is_atom_local = self
                    .active_atomic_inline_coordinate_spaces
                    .last()
                    .is_some_and(|space| {
                        containing_block_context.coordinate_space
                            == PositionedCoordinateSpace::AtomicInline(*space)
                    });
                EscapedAtomReplay::for_positioned_box(
                    containing_block,
                    containing_block_is_atom_local,
                    horizontal_insets_are_auto && absolute_static_position.is_some(),
                    vertical_insets_are_auto
                        && absolute_static_position
                            .is_some_and(AbsoluteStaticPosition::has_vertical_position),
                )
            } else {
                EscapedAtomReplay::none()
            };
            // Overflow clipping is not itself a stacking context, but it is
            // nevertheless an isolation boundary for the clipped contents.
            // Keep positioned descendants in the locally captured fragment so
            // the padding-box clip applies to them before their parent-level
            // stacking phase is selected.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            let captures_positioned_descendants = (policy.is_real_stacking_context
                && policy.captures_positioned_descendants)
                || policy.effects.overflow_clip_effect.is_some()
                || positioned_contents_clip.is_some();
            let mut child_contexts = Vec::new();
            if captures_positioned_descendants {
                for layer in child_layers.drain(..) {
                    let layer = layer.into_layer();
                    links.extend(layer.links);
                    child_contexts.push(layer.context);
                }
            }
            let positioned_overflow_clip = match policy.effects.overflow_clip_effect.take() {
                Some(crate::document::paint::contours::OverflowClipEffect::Rect(clip)) => {
                    Some(clip)
                }
                Some(effect) => {
                    policy.effects.overflow_clip_effect = Some(effect);
                    positioned_contents_clip.map(PaintClip::from_paint_rect)
                }
                None => positioned_contents_clip.map(PaintClip::from_paint_rect),
            };
            if let Some(overflow_clip) = positioned_overflow_clip {
                // Overflow and paint containment clip the positioned box's
                // contents and captured descendants, not its own background,
                // border, or outline. Keep that clip inside the positioned
                // stacking context instead of applying it to the context as a
                // whole.
                // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
                // <https://www.w3.org/TR/css-contain-1/#containment-paint>
                positioned_fragment = positioned_fragment.with_contents_clipped_to_rect(
                    overflow_clip,
                    std::mem::take(&mut child_contexts),
                );
            }
            let has_positioned_paint =
                !positioned_fragment.is_empty() || !child_contexts.is_empty();
            if has_positioned_paint {
                let positioned_source_box = self
                    .positioned_generated_source
                    .and_then(|source| source.box_source_for_element(element.id))
                    .unwrap_or(InlineStaticPositionBoxSource::Principal);
                if captures_multicol_positioned_principal {
                    // Source layout still runs to materialize anonymous
                    // columns and resolve intrinsic geometry, but the paint
                    // belongs to the deferred principal replay. Letting it
                    // enter this temporary page would also embed it in a
                    // normal-flow ancestor's stacking context.
                    continue;
                }
                if style.position == Position::Fixed && locally_contained_fixed_block.is_none() {
                    let layer = positioned_fragment.into_viewport_fixed_layer(
                        element.id,
                        source_style.clone(),
                        PositionedStackingMetadata {
                            transaction_depth: self.positioned_paint_transaction_depth,
                            source_is_target: element.is_target,
                            stack_level: policy.stack_level,
                            source_order: positioned_source_order,
                            effects: policy.effects,
                            bounds,
                            child_contexts,
                            links,
                            escaped_atom_replay,
                        },
                    );
                    // Intrinsic and auto-size measurement can visit a
                    // viewport-fixed element before its final in-flow static
                    // position is known. A fixed layer is one replayable
                    // paint record per source element, so the later committed
                    // layout replaces any provisional record even when their
                    // geometry differs.
                    // <https://drafts.csswg.org/css-position-3/#fixed-pos>
                    self.fixed_layers.retain(|existing| {
                        existing.source_element != layer.source_element
                            || existing.source_style != layer.source_style
                    });
                    self.fixed_layers.push(layer);
                    continue;
                }
                let mut layer = positioned_fragment
                    .into_page_local_layer(
                        PositionedPaintIdentity {
                            source_element: Some(element.id),
                            source_style: source_style.clone(),
                            source_style_identity: source_style as *const ComputedStyle as usize,
                            source_box: positioned_source_box,
                        },
                        PositionedStackingMetadata {
                            transaction_depth: self.positioned_paint_transaction_depth,
                            source_is_target: element.is_target,
                            stack_level: policy.stack_level,
                            source_order: positioned_source_order,
                            effects: policy.effects,
                            bounds,
                            child_contexts,
                            links,
                            escaped_atom_replay,
                        },
                    )
                    .into_layer();
                layer.overflow_clip_containing_block = Some(if style.position == Position::Fixed {
                    PositionedContainingBlockScopeDepth::Fixed(self.fixed_containing_blocks.len())
                } else {
                    PositionedContainingBlockScopeDepth::Absolute(self.containing_blocks.len())
                });
                // A block formatting retry can revisit this box after its
                // final static position is known. Replace the provisional
                // capture for that element/page before committing its final
                // layer.
                self.positioned_layers.retain(|existing| {
                    existing.source_element != Some(element.id)
                        || existing.source_box != positioned_source_box
                        || existing.page_index != page_index
                });
                // Captured positioned paint can be reached again when an
                // enclosing layout pass replays its final fragmentainer.
                // Keep one record for an identical page-local stacking
                // context; otherwise backgrounds, annotations, and PDF
                // operators are emitted twice.
                // <https://www.w3.org/TR/css-position-3/#painting-order>
                if !self
                    .positioned_layers
                    .iter()
                    .any(|existing| equivalent_positioned_layer(existing, &layer))
                {
                    if style.position == Position::Absolute
                        && !style.box_values.height.is_auto()
                        && retained_viewport_fixed_descendant
                        && principal_page_span_target.is_some_and(|target| page_index > target)
                    {
                        continue;
                    }
                    self.positioned_layers.push(layer);
                }
            }
            if !captures_positioned_descendants {
                // These child layers have escaped this principal. Their
                // scratch transaction is complete, but an enclosing
                // positioned transaction (if any) still owns them.
                self.positioned_layers
                    .extend(child_layers.into_iter().map(|layer| {
                        layer.release_to_transaction_depth(self.positioned_paint_transaction_depth)
                    }));
            }
        }
        if style.position == Position::Absolute
            && !style.box_values.height.is_auto()
            && retained_viewport_fixed_descendant
            && let Some(target) = principal_page_span_target
        {
            self.positioned_layers.retain(|layer| {
                layer.source_element != Some(element.id)
                    || layer.source_box
                        != self
                            .positioned_generated_source
                            .and_then(|source| source.box_source_for_element(element.id))
                            .unwrap_or(InlineStaticPositionBoxSource::Principal)
                    || layer.page_index <= target
            });
        }
    }

    pub(in crate::layout) fn layout_positioned_block_with_inline_static_position(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        static_position: StaticPositionCapture,
    ) {
        let previous = self.inline_static_position;
        let previous_absolute_static_position = self.absolute_static_position;
        let rectangle = static_position.rectangle;
        let absolute_static_position = self.absolute_static_position.unwrap_or_else(|| {
            AbsoluteStaticPosition::from_page_rect(
                rectangle.area.x(),
                rectangle.area.x() + rectangle.area.width(),
                rectangle.area.top_y(),
            )
        });
        // Horizontal inline layout keeps the surrounding block hypothetical
        // position as its physical block-axis fallback. Vertical inline
        // layout instead needs the captured inline edge for automatic
        // physical `top`/`bottom` resolution.
        self.absolute_static_position = Some(if style.writing_mode.has_vertical_lines() {
            absolute_static_position.with_inline_static_position_rectangle(rectangle)
        } else {
            absolute_static_position.with_static_position_rectangle(rectangle)
        });
        self.inline_static_position = Some(static_position);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.inline_static_position = previous;
        self.absolute_static_position = previous_absolute_static_position;
    }
}

#[cfg(test)]
mod positioned_flow_origin_tests {
    use super::*;

    #[test]
    fn enters_positioned_surrogates_at_the_resolved_page_top_cursor() {
        let content_box = PageTopRect::new(20.0, 100.0, 12.0, 60.0);

        let origin = PositionedFlowOrigin::from_resolved_positioned_box(content_box, 8.0);
        assert_eq!(origin.cursor_y, 100.0);
        assert_eq!(origin.content_left, 28.0);
        assert_eq!(origin.content_right, 40.0);
    }

    #[test]
    fn only_decoration_or_fixed_descendants_require_a_continuous_principal_span() {
        assert_eq!(
            PrincipalPageSpanObligation::for_positioned_principal(false, false, false),
            PrincipalPageSpanObligation::ObservedOutput
        );
        assert!(
            PrincipalPageSpanObligation::for_positioned_principal(true, false, false)
                .requires_continuous_principal_span()
        );
        assert!(
            PrincipalPageSpanObligation::for_positioned_principal(false, true, false)
                .requires_continuous_principal_span()
        );
        assert!(
            PrincipalPageSpanObligation::for_positioned_principal(false, false, true)
                .requires_continuous_principal_span()
        );
    }
}
