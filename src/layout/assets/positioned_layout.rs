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
        && left.escaped_atom_translation == right.escaped_atom_translation
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

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_positioned_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let positioned_style = self.positioned_used_style(style);
        let source_style = positioned_style.source();
        let style = positioned_style.used();
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
                box_tree::build_frozen_table_fragment(element, &signature, table_children);
            Some(&built_positioned_table_fragment)
        } else {
            table_fragment
        };
        let containment = used_property_containment(element, style);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let static_scroll_snap_scope = self.begin_static_scroll_snap_scope(style, false);
        let positioned_layer_start = self.positioned_layers.len();
        let fixed_layer_start = self.fixed_layers.len();
        let retained_viewport_fixed_descendant = self
            .fixed_layers
            .iter()
            .any(|layer| element_contains(element, layer.source_element));
        let pagination_state = self.positioned_pagination_state();
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
        let source_containing_block = if style.position == Position::Fixed {
            locally_contained_fixed_block.unwrap_or_else(|| self.page_containing_block())
        } else {
            self.containing_blocks
                .last()
                .cloned()
                .or_else(|| {
                    uses_escaped_atom_outer_containing_block.then(|| {
                        escaped_atom_positioning_context
                            .expect("escaped atom context was checked above")
                            .actual_containing_block
                    })
                })
                .unwrap_or_else(|| self.page_containing_block())
        };
        let uses_initial_page_containing_block =
            matches!(style.position, Position::Absolute | Position::Fixed)
                && self.containing_blocks.is_empty()
                && locally_contained_fixed_block.is_none();
        let grid_positioning_context = (style.position == Position::Absolute)
            .then(|| {
                self.grid_positioning_scopes.iter().rev().find_map(|scope| {
                    scope.descendant_positioning_context(style, source_containing_block)
                })
            })
            .flatten();
        // A qualifying positioned descendant receives Grid's actual area
        // containing block and a distinct grid-derived static rectangle.
        // <https://www.w3.org/TR/css-grid-1/#abspos>
        let containing_block = grid_positioning_context
            .and_then(|context| context.grid_area_containing_block)
            .unwrap_or(source_containing_block);
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
        // Direct flex children install a static-position rectangle before the
        // generic absolute-positioning algorithm. Preserve its centered-axis
        // available size for automatic sizing; the ordinary containing block
        // still resolves the final inset equation.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        // <https://drafts.csswg.org/css-align-3/#abspos-sizing>
        let mut absolute_static_position = self
            .absolute_static_position
            .or_else(|| grid_positioning_context.map(|context| context.static_position));
        let horizontal_insets_are_auto_for_available =
            used_inset_left(&used_style, containing_block).is_none()
                && used_inset_right(&used_style, containing_block).is_none();
        let flex_available_outer_width = (horizontal_insets_are_auto_for_available
            && horizontal_size_was_auto)
            .then(|| {
                absolute_static_position
                    .and_then(AbsoluteStaticPosition::static_alignment)
                    .and_then(|alignment| {
                        alignment.available_horizontal_outer_size(containing_block)
                    })
            })
            .flatten();
        let positioned_available_outer_width = (flex_available_outer_width
            .unwrap_or(containing_block.width())
            - used_style.margin.left
            - used_style.margin.right)
            .max(used_style.font_size);
        let replaced_content_size = if is_replaced_element(element) {
            used_image(
                element,
                &used_style,
                positioned_available_outer_width,
                PercentageBasis::definite_from(
                    content_box_pt(containing_block.height()),
                    BlockSizeBasisSource::AbsolutePositioned,
                ),
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                // CSS 2.2 gives absolutely positioned replaced elements their
                // own auto-size rules: intrinsic dimensions and aspect ratio
                // resolve the content size before the absolute inset equation
                // is solved.
                // <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-width>
                // and <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-height>.
                set_style_used_width(&mut used_style, image.content_size.width);
                set_style_used_height(&mut used_style, image.content_size.height);
                (image.content_size.width, image.content_size.height)
            })
        } else {
            None
        };
        let style = &used_style;
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
        let source_static_position = escaped_atom_positioning_context
            .map(|context| context.static_position)
            .unwrap_or_else(|| {
                AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
                    static_source_left,
                    static_source_right,
                    previous_cursor_y,
                    true,
                )
            });
        // Ordinary flow has no layout-specific abspos alignment record, but
        // explicit self-alignment still uses its static-position rectangle
        // when both insets are automatic. The positioned containing block is
        // the conservative complete rectangle available at this boundary;
        // Flexbox and Grid install their more specific records above.
        // <https://drafts.csswg.org/css-align-3/#align-abspos>
        if absolute_static_position.is_none()
            && (style.justify_self.keyword != SelfAlignmentKeyword::Auto
                || style.align_self.keyword != SelfAlignmentKeyword::Auto)
        {
            let inline = if style.justify_self.keyword == SelfAlignmentKeyword::Auto {
                css::SelfAlignment::NORMAL
            } else {
                style.justify_self
            };
            let block = if style.align_self.keyword == SelfAlignmentKeyword::Auto {
                css::SelfAlignment::NORMAL
            } else {
                style.align_self
            };
            absolute_static_position = Some(source_static_position.with_static_alignment(
                AbsposStaticAlignment::new(
                    containing_block.rect,
                    self.containing_block_writing_mode,
                    self.containing_block_direction,
                    inline,
                    block,
                ),
            ));
        }
        let inline_auto_static_y =
            style.abspos_static_source.is_inline_level() && vertical_insets_are_auto;
        let inline_static_position = self.inline_static_position;
        let inline_static_uses_margin_box_top = inline_auto_static_y
            && inline_static_position.is_some_and(|position| position.use_margin_box_top);
        // A margin-box static rectangle already fixes the positioned
        // fragment's block-start. Translating its first line again from a
        // baseline would shift atomic inline sources twice. Baseline-mode
        // placeholders instead use the prepared line baseline below.
        let inline_static_baseline_position = (inline_auto_static_y
            && !inline_static_uses_margin_box_top)
            .then_some(inline_static_position)
            .flatten();
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
                        StaticHorizontalPosition::new(
                            position.start_x - containing_block.x(),
                            containing_block.x() + containing_block.width() - position.end_x,
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
        } else if inline_static_uses_margin_box_top && let Some(position) = inline_static_position {
            containing_block.top_y() - position.top_y
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
            self.definite_block_size_stack
                .push(positioned_height_percentage_basis);
        }
        let auto_or_intrinsic_width = replaced_content_size.map_or_else(
            || {
                if style.writing_mode.has_vertical_lines() && horizontal_size_was_auto {
                    // A physical `width:auto` is the logical block size of a
                    // vertical positioned block.  Its shrink-to-fit width is
                    // therefore the block contribution *after* fitting lines
                    // to the direct orthogonal available height.  Generic
                    // intrinsic-width measurement only sees a glyph advance,
                    // which makes an abspos vertical block narrower than the
                    // equivalent in-flow or inline-block formatting context.
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
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
                            definite_content_height: None,
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
            self.definite_block_size_stack.pop();
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
            static_horizontal_position = static_alignment.horizontal_static_position(
                containing_block,
                static_alignment_border_width,
                style.margin.left,
                style.margin.right,
            );
        }
        let containing_horizontal_direction = physical_horizontal_axis_direction(
            self.containing_block_writing_mode,
            self.containing_block_direction,
        );
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
        // An absolutely positioned table is laid out against the containing
        // block's available size, even when the generic absolute-position
        // equation has two opposing insets that would span a larger range.
        // Keep the table layout algorithm's auto width instead of replacing it
        // with that range; the insets still determine the table's position.
        // <https://drafts.csswg.org/css-tables-3/#abspos>
        if table_fragment.is_some() && horizontal_size_was_auto {
            positioned_x.size = auto_or_intrinsic_width;
        }
        let mut positioned_content_width = positioned_x.size;

        // Measure under the same out-of-flow named-page suppression as the
        // final positioned subtree, so page-name descendants cannot inflate
        // the measured fragment span:
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        self.push_page_name_scope_suppression();
        let auto_height = if vertical_size_was_auto && let Some((_, height)) = replaced_content_size
        {
            height
        } else {
            let mut estimate_style = style.clone();
            estimate_style.position = Position::Static;
            estimate_style.margin = css::Edges::ZERO;
            estimate_style.box_values.margin =
                css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
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
                0.0
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
        .unwrap_or(auto_height)
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width_for_positioning;
        if vertical_insets_are_auto
            && let Some(static_alignment) =
                absolute_static_position.and_then(AbsoluteStaticPosition::static_alignment)
        {
            static_vertical_start = static_alignment.vertical_static_start(
                containing_block,
                static_alignment_border_height,
                style.margin.top,
                style.margin.bottom,
            );
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
            .then_some(auto_height),
            static_vertical_start,
            vertical_border_width_for_positioning,
        );
        let positioned_content_height = positioned_y.size;
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
                positioned_content_width = positioned_x.size;
            }
        }
        let positioned_page_offset =
            if style.position == Position::Absolute && !uses_escaped_atom_outer_containing_block {
                let (page_offset, remainder) =
                    self.absolute_positioned_page_start_offset(containing_block, positioned_y);
                // A static-position rectangle exactly on a fragmentainer end
                // belongs to that current fragmentainer. Its contents can then
                // fragment forward normally; eagerly shifting the principal box
                // first manufactures an empty intervening page.
                // <https://www.w3.org/TR/css-position-3/#static-position>
                if vertical_insets_are_auto && page_offset > 0 && remainder <= 0.01 {
                    page_offset - 1
                } else {
                    page_offset
                }
            } else {
                0
            };
        let positioned_origin_page_index =
            containing_block_origin_page_index + positioned_page_offset;
        let positioned_border_box_width = positioned_content_width + horizontal_non_content;
        self.content_left = containing_block.x() + positioned_x.start + positioned_x.margin_start;
        self.content_right = self.content_left + positioned_border_box_width;
        let positioned_margin_top = if inline_auto_static_y {
            if inline_static_uses_margin_box_top {
                // The hypothetical atomic inline supplies its margin-box
                // block-start as the static rectangle. The absolute-position
                // equation has already selected that edge, so subtracting
                // the positioned box's start margin here would apply it a
                // second time.
                // <https://www.w3.org/TR/css-position-3/#staticpos-rect>
                0.0
            } else {
                ((style.line_height - style.font_size) / 2.0).max(0.0)
            }
        } else {
            positioned_y.margin_start
        };
        self.cursor_y = containing_block.top_y() - positioned_y.start - positioned_margin_top;
        // Enter the destination fragmentainer's local coordinate space before
        // laying out the positioned subtree. Keeping a continuous coordinate
        // below the source page and translating captured paint afterward loses
        // exact-boundary placement and gives descendants the wrong containing
        // block geometry.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        if positioned_page_offset > 0 {
            self.cursor_y += positioned_page_offset as f32 * self.page_area_height().max(1.0);
        }
        let positioned_border_box_height = positioned_content_height
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width_for_positioning;
        let positioned_border_box = PageTopRect::new(
            self.content_left,
            self.cursor_y,
            positioned_border_box_width,
            positioned_border_box_height,
        )
        .paint_clip();
        // A positioned descendant is not emitted through normal-flow block
        // layout, so record its final border-box geometry here while the
        // ancestor scroll-container capture is active.  The later stacking
        // context bounds are paint bounds (and may include a different
        // coordinate remapping), whereas CSS Scroll Snap defines an area from
        // the box's border box before its scroll-margin outsets.
        // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
        self.record_static_scroll_snap_area(element, style, positioned_border_box.paint_rect());
        self.record_static_scroll_target_area(
            element.is_target,
            positioned_border_box.paint_rect(),
            style,
        );
        // CSS Positioned Layout and CSS 2.2 Appendix E order positioned
        // boxes in tree order in their containing stacking context. Reserve
        // this box's order before laying out descendants so child positioned
        // contexts, including fixed descendants, sort after their parent.
        let positioned_source_order = self.next_paint_source_order();

        let mut flow_style = style.clone();
        flow_style.position = Position::Static;
        // This positioned principal box owns the scroll-capture boundary;
        // its blockified flow surrogate must not open a duplicate scope.
        flow_style.scroll_snap_type = css::ScrollSnapType::None;
        // The surrogate is laid out at its static-flow origin solely to
        // capture the positioned principal's contents.  It is not a second
        // CSS box, so it must not contribute a duplicate snap area at that
        // temporary origin.
        flow_style.scroll_snap_align = css::ScrollSnapAlign::default();
        flow_style.margin = css::Edges::ZERO;
        flow_style.box_values.margin =
            css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
        set_style_used_width(&mut flow_style, positioned_content_width);
        set_style_used_height(&mut flow_style, positioned_content_height);
        // Positioned-axis resolution supplies content-box sizes.  The static
        // flow surrogate replays those used values, so it must retain their
        // content-box space instead of reapplying an authored border-box
        // conversion.
        // <https://www.w3.org/TR/css-sizing-3/#box-sizing>
        flow_style.box_sizing = BoxSizing::ContentBox;
        clear_position_insets(&mut flow_style);
        let positioned_containing_block_top = self.cursor_y - positioned_border_widths.top;
        self.containing_blocks.push(
            ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left + positioned_border_widths.left,
                positioned_containing_block_top,
                positioned_content_width + flow_style.padding.left + flow_style.padding.right,
                positioned_content_height + flow_style.padding.top + flow_style.padding.bottom,
            ))
            .on_page(positioned_origin_page_index),
        );
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
        );
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
            && (style.background_color.is_potentially_visible()
                || style.background_image.is_image()
                || style.border_image.source.is_image()
                || used_border_width(style) > layout_pt(0.0)
                || (style.outline_width > 0.0 && !style.outline_style.suppresses_used_width()));
        let viewport_fixed_descendant =
            retained_viewport_fixed_descendant || self.fixed_layers.len() > fixed_layer_start;
        // A transparent absolute principal still occupies a sequence of page
        // fragmentainers.  That structural span is independent from whether
        // the principal itself has background/border paint: viewport-fixed
        // descendants replay on every occupied page.
        // https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements
        let principal_page_span_target = self.absolute_positioned_page_span_target(
            style,
            containing_block,
            positioned_y,
            vertical_border_width_for_positioning,
            containing_block_origin_page_index.saturating_sub(
                if containing_block_origin_page_index > 0 {
                    positioned_page_offset
                } else {
                    0
                },
            ),
        );
        let mut positioned_fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        if has_principal_decoration && let Some(target_page_index) = principal_page_span_target {
            let captured_last_page_index = positioned_fragments
                .iter()
                .map(|(page_index, _)| *page_index)
                .max()
                .unwrap_or(paint_page_index);
            self.extend_positioned_principal_decoration_fragments(
                &mut positioned_fragments,
                style,
                positioned_border_box,
                paint_page_index,
                captured_last_page_index,
                target_page_index,
                pagination_state.current_page_context,
            );
        }
        if style.position == Position::Absolute {
            // Preserve every captured slice when moving scratch pagination to
            // the absolute box's destination sequence. A slice can contain
            // only background, border, or another non-text paint primitive;
            // using the first text baseline to choose a remapping origin
            // drops those observable fragments.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            self.remap_absolute_positioned_fragments(
                &mut positioned_fragments,
                paint_page_index,
                // Scratch pagination starts at the containing formatting
                // context's source fragment, while an explicitly offset
                // nested absolute box has already entered the containing
                // box's destination-local coordinate space. Preserve that
                // destination-local offset when assigning the captured
                // source slices their final page ownership.
                // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
                positioned_origin_page_index
                    + if containing_block_origin_page_index > 0 {
                        positioned_page_offset
                    } else {
                        0
                    },
            );
            // A descendant positioned box resolves its containing block from
            // this box's destination-page origin. Its layer page index is
            // therefore already final, even though the parent principal
            // fragments above were captured in scratch fragmentainers.
            // Remapping it again would shift a nested absolute box by the
            // parent page offset a second time.
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        }
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
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_padding_box,
        );
        if static_scroll_offset.x != 0.0 || static_scroll_offset.y != 0.0 {
            let translation =
                crate::layout::scroll_snap::static_scroll_translation(static_scroll_offset, style);
            for (_, fragment) in &mut positioned_fragments {
                *fragment = fragment.clone().translated(translation);
            }
            for layer in &mut child_positioned_layers {
                *layer = layer.clone().translated(translation);
            }
        }
        for layer in &child_positioned_layers {
            if !positioned_fragments
                .iter()
                .any(|(page_index, _)| *page_index == layer.page_index)
            {
                positioned_fragments.push((
                    layer.page_index,
                    PaintFragment::from_primitives(Vec::new(), Vec::new()),
                ));
            }
        }
        let target_page_index = if style.position == Position::Absolute {
            positioned_fragments
                .iter()
                .filter(|(_, fragment)| !fragment.is_empty())
                .map(|(page_index, _)| *page_index)
                .chain(child_positioned_layers.iter().map(|layer| layer.page_index))
                .chain(
                    (has_principal_decoration || viewport_fixed_descendant)
                        .then_some(principal_page_span_target)
                        .flatten(),
                )
                .max()
        } else {
            None
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
        let nested_positioned_page_span_target = self.pending_positioned_page_span_target;
        let nested_absolute_positioned_page_span_target = self.absolute_positioned_page_span_target;
        self.restore_positioned_pagination_state(pagination_state);
        self.pending_positioned_page_span_target = merged_positioned_page_span_target(
            self.pending_positioned_page_span_target,
            nested_positioned_page_span_target,
        );
        self.absolute_positioned_page_span_target = merged_positioned_page_span_target(
            self.absolute_positioned_page_span_target,
            nested_absolute_positioned_page_span_target,
        );
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.inline_static_position = previous_inline_static_position;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.retain_absolute_positioned_page_span(principal_page_span_target);
        self.ensure_positioned_page_span(target_page_index);

        // A positioned box owns its principal decoration even when its
        // contained formatting context contributes no fragment. This occurs
        // for example when size containment suppresses the only in-flow
        // descendant. Do not let that empty content result suppress a visible
        // background, border, or outline.
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if positioned_fragments.is_empty()
            && child_positioned_layers.is_empty()
            && style.visibility == Visibility::Visible
            && positioned_border_box.width() > 0.0
            && positioned_border_box.height() > 0.0
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
                positioned_fragments.push((positioned_origin_page_index, fragment));
            }
        }

        for (_, positioned_fragment) in &mut positioned_fragments {
            if let (Some(static_position), Some(fragment_baseline_y)) = (
                inline_static_baseline_position,
                positioned_fragment.first_line_y(),
            ) {
                *positioned_fragment =
                    positioned_fragment
                        .clone()
                        .translated(PaintTranslation::new(
                            0.0,
                            static_position.baseline_y - fragment_baseline_y,
                        ));
            }
        }
        if positioned_fragments
            .iter()
            .all(|(_, fragment)| fragment.is_empty())
            && child_positioned_layers.is_empty()
        {
            return;
        }
        let child_layers_by_page = child_positioned_layers;
        for (page_index, mut positioned_fragment) in positioned_fragments {
            if style.position == Position::Absolute
                && !style.box_values.height.is_auto()
                && retained_viewport_fixed_descendant
                && principal_page_span_target.is_some_and(|target| page_index > target)
            {
                continue;
            }
            let child_layers = child_layers_by_page
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .collect::<Vec<_>>();
            let mut links = positioned_fragment.links.clone();
            let bounds = positioned_border_box;
            if containment.size {
                positioned_fragment =
                    positioned_fragment.with_monolithic_fragmentation_scope(bounds);
            }
            let mut policy = StackingContextPolicy::for_positioned(element, style, bounds);
            if self
                .document_canvas_overflow
                .is_viewport_overflow_source(element)
            {
                // Propagated root/body overflow is applied to the viewport;
                // its source box has used `overflow: visible` and must not
                // retain a local clipping effect when positioned.
                // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
                policy.effects.overflow_clip = None;
                policy.effects.rounded_overflow_clip = None;
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
                    policy.effects.overflow_clip = Some(page_clip);
                }
            }
            if positioned_fragment.contains_overflow_clip()
                && policy
                    .effects
                    .overflow_clip
                    .is_some_and(|clip| clip.width() <= 0.0 || clip.height() <= 0.0)
            {
                // The measured formatting context already owns this empty
                // used padding-box clip. Applying the reconstructed empty clip
                // around that nested context would also erase the principal
                // decoration, which lies outside the contents clip.
                policy.effects.overflow_clip = None;
            }
            let escaped_atom_translation = if self.escaped_atom_positioning_depth > 0 {
                let containing_block_is_atom_local =
                    self.escaped_atom_containing_block == Some(containing_block);
                EscapedAtomTranslation::from_positioned_static_axes(
                    containing_block,
                    containing_block_is_atom_local
                        || (horizontal_insets_are_auto && absolute_static_position.is_some()),
                    containing_block_is_atom_local
                        || (vertical_insets_are_auto
                            && absolute_static_position
                                .is_some_and(AbsoluteStaticPosition::has_vertical_position)),
                    !containing_block_is_atom_local,
                )
            } else {
                EscapedAtomTranslation::none()
            };
            let captures_positioned_descendants =
                policy.is_real_stacking_context && policy.captures_positioned_descendants;
            let mut child_contexts = if captures_positioned_descendants {
                for layer in &child_layers {
                    links.extend(layer.links.clone());
                }
                child_layers
                    .iter()
                    .cloned()
                    .map(|layer| layer.context)
                    .collect()
            } else {
                Vec::new()
            };
            if containment.paint
                && let Some(overflow_clip) = policy.effects.overflow_clip.take()
            {
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
                let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                    policy.stack_level,
                    positioned_fragment,
                    child_contexts,
                )
                .with_source_order(positioned_source_order)
                .with_effects(policy.effects)
                .with_bounds(bounds);
                if style.position == Position::Fixed && locally_contained_fixed_block.is_none() {
                    let layer = FixedPaintLayer {
                        source_element: element.id,
                        source_style: source_style.clone(),
                        stack_level: policy.stack_level,
                        context,
                        links,
                    };
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
                let layer = PositionedPaintLayer {
                    page_index,
                    source_element: Some(element.id),
                    source_style: style.clone(),
                    source_is_target: element.is_target,
                    stack_level: policy.stack_level,
                    context,
                    links,
                    escaped_atom_translation,
                };
                // A block formatting retry can revisit this box after its
                // final static position is known. Replace the provisional
                // capture for that element/page before committing its final
                // layer.
                self.positioned_layers.retain(|existing| {
                    existing.source_element != Some(element.id) || existing.page_index != page_index
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
                self.positioned_layers.extend(child_layers);
            }
        }
        if style.position == Position::Absolute
            && !style.box_values.height.is_auto()
            && retained_viewport_fixed_descendant
            && let Some(target) = principal_page_span_target
        {
            self.positioned_layers.retain(|layer| {
                layer.source_element != Some(element.id) || layer.page_index <= target
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
        static_position: InlineStaticPosition,
    ) {
        let previous = self.inline_static_position;
        self.inline_static_position = Some(static_position);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.inline_static_position = previous;
    }
}
