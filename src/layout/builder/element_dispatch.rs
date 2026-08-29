use super::*;
use crate::css::ContainerType;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_element_inner(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            PrincipalBoxPaintMode::RootPaints,
            None,
        );
    }

    /// Lay out an element with explicit ownership for its principal-box paint.
    ///
    /// The paint mode applies only to this element. It is not propagated into
    /// `layout_element_inner_kind`, so descendants still create their own CSS
    /// stacking contexts and compositing groups. CSS Grid and Flexbox place
    /// the item as a stacking unit after its independent formatting context
    /// has been laid out:
    /// <https://www.w3.org/TR/css-grid-1/#z-order> and
    /// <https://www.w3.org/TR/css-flexbox-1/#painting>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_inner_with_principal_effect_context(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        capture_principal_effect_context: bool,
        principal_box_paint_mode: PrincipalBoxPaintMode,
        principal_descendant_percentage_context: Option<DescendantBlockPercentageContext>,
    ) {
        // `content-visibility` has its own content-skipping behavior, but its
        // principal box establishes the containment effects needed to size and
        // isolate that skipped content. Keep this as a used-style adjustment
        // rather than conflating it with the authored `contain` value.
        // <https://drafts.csswg.org/css-contain-2/#content-visibility>
        let needs_content_visibility_containment = has_html_rendering_semantics(element)
            && !matches!(style.content_visibility, ContentVisibility::Visible);
        let needs_container_containment = !matches!(style.container_type, ContainerType::Normal);
        let mut used_style;
        let style = if needs_content_visibility_containment || needs_container_containment {
            used_style = style.clone();
            if needs_container_containment {
                used_style.contain.layout = true;
                used_style.contain.style = true;
                match used_style.container_type {
                    ContainerType::Normal => {}
                    ContainerType::InlineSize => used_style.contain.inline_size = true,
                    ContainerType::Size => used_style.contain.size = true,
                }
            }
            if needs_content_visibility_containment {
                used_style.contain.layout = true;
                used_style.contain.paint = true;
                used_style.contain.style = true;
                // `auto` is conservatively visible in paged output, so its
                // descendants still determine the principal box's size. A
                // skipped `hidden` subtree instead uses size containment and
                // its `contain-intrinsic-size` fallback.
                // <https://www.w3.org/TR/css-contain-2/#content-visibility>
                if matches!(style.content_visibility, ContentVisibility::Hidden) {
                    used_style.contain.size = true;
                    used_style.content = css::Content::Normal;
                }
            }
            &used_style
        } else {
            style
        };
        let hidden_content = matches!(style.content_visibility, ContentVisibility::Hidden);
        let empty_children = [];
        let child_boxes = hidden_content
            .then_some(&empty_children[..])
            .or(child_boxes);
        let run_in_children = if hidden_content {
            &empty_children[..]
        } else {
            run_in_children
        };
        let table_fragment = (!hidden_content).then_some(table_fragment).flatten();
        let layout_kind = element_layout_kind(element, style);
        if capture_principal_effect_context
            && self.should_capture_non_positioned_effect_context(layout_kind, element, style)
        {
            self.layout_non_positioned_effect_context(
                layout_kind,
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
                principal_box_paint_mode,
                principal_descendant_percentage_context,
            );
            return;
        }
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            principal_box_paint_mode,
            principal_descendant_percentage_context,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_inner_kind(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
        principal_descendant_percentage_context: Option<DescendantBlockPercentageContext>,
    ) {
        let replayed_flex_item_percentage_height_basis =
            self.take_replayed_flex_item_percentage_height_basis();
        let replayed_item_logical_inline_size = replayed_flex_item_percentage_height_basis
            .is_some()
            .then(|| {
                LogicalInlineContentSize::new(content_box_pt(
                    self.current_content_logical_inline_size(),
                ))
            });
        match layout_kind {
            ElementLayoutKind::None => (),
            ElementLayoutKind::Positioned
                if self.positioned_inline_layout_suppression_depth == 0 =>
            {
                self.layout_positioned_block_with_static_source(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    table_fragment,
                );
            }
            // Positioned descendants are out of flow and do not contribute to
            // intrinsic inline measurements. Their committed formatting pass
            // owns static-position resolution and paint-layer creation.
            ElementLayoutKind::Positioned => {}
            // Replaced block-level boxes bypass `layout_block_*`, which owns
            // the usual forced-break-after hook. Preserve the same class-A
            // boundary semantics here so `page` and `break-after` coalesce
            // instead of one of them being silently skipped.
            // <https://drafts.csswg.org/css-page-3/#using-named-pages>
            // <https://drafts.csswg.org/css-break-3/#forced-breaks>
            ElementLayoutKind::Canvas => {
                debug_assert!(principal_box_paint_mode.root_paints());
                self.layout_canvas(element, style);
                self.apply_forced_break_after_box_in(self.active_fragmentainer_kind(), style);
            }
            ElementLayoutKind::Image => {
                debug_assert!(principal_box_paint_mode.root_paints());
                self.layout_image(element, style);
                self.apply_forced_break_after_box_in(self.active_fragmentainer_kind(), style);
            }
            ElementLayoutKind::GeneratedImage => {
                debug_assert!(principal_box_paint_mode.root_paints());
                self.layout_generated_image(element, style);
                self.apply_forced_break_after_box_in(self.active_fragmentainer_kind(), style);
            }
            ElementLayoutKind::Svg => {
                debug_assert!(principal_box_paint_mode.root_paints());
                self.layout_svg(element, style);
                self.apply_forced_break_after_box_in(self.active_fragmentainer_kind(), style);
            }
            ElementLayoutKind::Flex => self.layout_flex_with_descendant_percentage_height_basis(
                element,
                style,
                stylesheets,
                child_boxes,
                principal_descendant_percentage_context
                    .map(DescendantBlockPercentageContext::percentage_basis)
                    .or(replayed_flex_item_percentage_height_basis),
                principal_box_paint_mode,
            ),
            ElementLayoutKind::Grid => self.layout_grid_with_descendant_percentage_height_basis(
                element,
                style,
                stylesheets,
                child_boxes,
                principal_descendant_percentage_context
                    .map(DescendantBlockPercentageContext::percentage_basis)
                    .or(replayed_flex_item_percentage_height_basis),
                principal_box_paint_mode,
            ),
            ElementLayoutKind::Table => {
                debug_assert!(principal_box_paint_mode.root_paints());
                let built_child_boxes;
                let table_children = if let Some(children) = child_boxes {
                    children
                } else {
                    built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let built_fragment;
                let fragment = if let Some(fragment) = table_fragment {
                    fragment
                } else {
                    let signature = self
                        .ancestors
                        .last()
                        .cloned()
                        .unwrap_or_else(|| element_signature(element));
                    built_fragment = box_tree::build_frozen_table_fragment(
                        element,
                        &signature,
                        style,
                        table_children,
                    );
                    &built_fragment
                };
                self.layout_table(element, style, stylesheets, fragment)
            }
            ElementLayoutKind::InlineFlow => {
                debug_assert!(principal_box_paint_mode.root_paints());
                let text = inline_text_for_style(element, style);
                if !text.is_empty() {
                    if style.display.is_list_item() {
                        let marker = self.marker_for_list_item(
                            element,
                            style,
                            self.containing_block_direction,
                        );
                        self.layout_list_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                            marker.as_ref(),
                        );
                    } else {
                        self.layout_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                        );
                    }
                }
            }
            ElementLayoutKind::BlockFlow => {
                self.layout_block_with_descendant_percentage_height_basis(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    child_boxes,
                    replayed_flex_item_percentage_height_basis,
                    principal_descendant_percentage_context,
                    replayed_item_logical_inline_size,
                    principal_box_paint_mode,
                );
            }
        }
        if matches!(layout_kind, ElementLayoutKind::BlockFlow)
            && !element.tag.eq_ignore_ascii_case("html")
        {
            self.last_principal_transform_box = self
                .last_block_layout_outcome
                .static_border_box
                .map(assets::TransformReferenceBox::css_layout);
        }
    }

    pub(in crate::layout) fn should_capture_non_positioned_effect_context(
        &self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        !matches!(
            layout_kind,
            ElementLayoutKind::None | ElementLayoutKind::Positioned
        ) && (self.preserve_3d_context_depth > 0
            || StackingContextPolicy::style_needs_non_positioned_scope(element, style))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_non_positioned_effect_context(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
        principal_descendant_percentage_context: Option<DescendantBlockPercentageContext>,
    ) {
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.last_principal_transform_box = None;
        let mut initial_policy = StackingContextPolicy::for_non_positioned_effect(
            element,
            style,
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
        );
        let deferred_flattening_boundary = self.preserve_3d_context_depth > 0
            && matches!(initial_policy.context_kind, StackingContextKind::None);
        if deferred_flattening_boundary {
            // A plain descendant might contain an independently flattened 3D
            // subtree. Keep its positioned descendants available until their
            // used effects are known below. If none needs flattening, they
            // are allowed to escape again so Appendix E paint bands remain
            // interleaved with the ancestor plane.
            initial_policy.child_layer_policy = ChildLayerPolicy::CaptureAll;
        }
        let previous_defer_block_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = true;
        let enters_3d_context =
            assets::used_transform_style(style) == css::TransformStyle::Preserve3d;
        if enters_3d_context {
            self.preserve_3d_context_depth += 1;
        }
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            principal_box_paint_mode,
            principal_descendant_percentage_context,
        );
        if enters_3d_context {
            self.preserve_3d_context_depth -= 1;
        }
        self.defer_next_block_decoration_promotion = previous_defer_block_decoration_promotion;
        let child_layers = if positioned_layer_start < self.positioned_layers.len()
            && !matches!(
                initial_policy.child_layer_policy,
                ChildLayerPolicy::EscapeAll
            ) {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let (mut child_layers, escaped_layers): (Vec<_>, Vec<_>) =
            match initial_policy.child_layer_policy {
                ChildLayerPolicy::CaptureAll => (child_layers, Vec::new()),
                ChildLayerPolicy::CaptureAutoLevel => child_layers
                    .into_iter()
                    .partition(|layer| matches!(layer.stack_level, StackLevel::Auto)),
                ChildLayerPolicy::EscapeAll => (Vec::new(), child_layers),
            };
        self.positioned_layers.extend(escaped_layers);
        let mut fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        if matches!(
            initial_policy.child_layer_policy,
            ChildLayerPolicy::CaptureAutoLevel
        ) {
            // A relatively positioned box with `z-index: auto` is an atomic
            // paint unit in the parent auto/zero phase, but it is not a real
            // stacking context. Non-auto descendant contexts must therefore
            // remain in the parent's negative/positive phases. Flex and Grid
            // items can create exactly such contexts while still being
            // statically positioned, so they are present in the captured
            // fragment rather than in `positioned_layers` above.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            // <https://drafts.csswg.org/css-flexbox/#painting>
            for (page_index, fragment) in &mut fragments {
                let (captured_contexts, escaped_contexts): (Vec<_>, Vec<_>) = fragment
                    .take_positioned_stacking_contexts()
                    .into_iter()
                    .partition(|context| matches!(context.stack_level, StackLevel::Auto));
                fragment.restore_positioned_stacking_contexts(captured_contexts);
                self.positioned_layers
                    .extend(
                        escaped_contexts
                            .into_iter()
                            .map(|context| PositionedPaintLayer {
                                page_index: *page_index,
                                transaction_depth: self.positioned_paint_transaction_depth,
                                source_element: None,
                                source_style: style.clone(),
                                source_style_identity: style as *const ComputedStyle as usize,
                                multicol_fragment_index: None,
                                source_is_target: false,
                                stack_level: context.stack_level,
                                context,
                                links: Vec::new(),
                                escaped_atom_translation: EscapedAtomTranslation::none(),
                            }),
                    );
            }
        }
        let contains_affine_3d_subtree = child_layers
            .iter()
            .any(|layer| layer.context.effects.affine_3d_transform.is_some())
            || fragments
                .iter()
                .any(|(_, fragment)| fragment.contains_affine_3d_transform());
        if deferred_flattening_boundary && !contains_affine_3d_subtree {
            // CSS Transforms only gives the context root and 3D-transformed
            // participants their own planes. An ordinary flat wrapper with
            // no 3D subtree stays in its ancestor's Appendix-E plane, so its
            // positioned descendants must escape this provisional capture.
            // <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
            self.positioned_layers
                .extend(std::mem::take(&mut child_layers));
        }
        for layer in &child_layers {
            if !fragments
                .iter()
                .any(|(page_index, _)| *page_index == layer.page_index)
            {
                fragments.push((
                    layer.page_index,
                    PaintFragment::from_primitives(Vec::new(), Vec::new()),
                ));
            }
        }
        for (page_index, mut fragment) in fragments {
            let mut child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .map(|layer| layer.context.with_links(layer.links))
                .collect::<Vec<_>>();
            if paint_containment_applies_to_element(element, style)
                && !child_contexts.is_empty()
                && let Some(overflow_clip) = fragment.top_level_contents_overflow_clip()
            {
                // Paint containment establishes the positioned containing
                // block, so its exact padding-box clip applies to captured
                // negative/positive stacking levels as well as normal flow.
                // The inner formatting context supplied the geometry; the
                // dispatcher owns the positioned descendant contexts.
                // <https://www.w3.org/TR/css-contain-1/#containment-paint>
                fragment = fragment.with_contents_clipped_to_rect(
                    overflow_clip,
                    std::mem::take(&mut child_contexts),
                );
            }
            if fragment.is_empty() && child_contexts.is_empty() {
                continue;
            }
            let source_order = self.next_paint_source_order();
            let (page_width, page_height) = if page_index < self.pages.len() {
                (
                    self.pages[page_index].width(),
                    self.pages[page_index].height(),
                )
            } else {
                (self.current_page.width(), self.current_page.height())
            };
            let bounds = fragment
                .bounds()
                .unwrap_or(PaintClip::from_paint_rect(paint_space_rect(
                    0.0,
                    0.0,
                    page_width,
                    page_height,
                )));
            // The captured fragment's bounds are paint ink, not the used box
            // that CSS Transforms uses for transform-origin and percentage
            // translations. Block layout reports that exact untransformed
            // border box after sizing, while paint bounds remain responsible
            // only for context culling and stacking.
            let geometry = self
                .last_principal_transform_box
                .map(|transform_box| {
                    assets::PrincipalPaintGeometry::with_transform_box(bounds, transform_box)
                })
                .unwrap_or_else(|| assets::PrincipalPaintGeometry::css_layout(bounds));
            let mut policy = StackingContextPolicy::for_non_positioned_effect_with_geometry(
                element, style, geometry,
            );
            if matches!(
                layout_kind,
                ElementLayoutKind::Canvas
                    | ElementLayoutKind::Image
                    | ElementLayoutKind::GeneratedImage
                    | ElementLayoutKind::Svg
            ) {
                // Replaced content carries its used content-edge contour on
                // the image/SVG primitive.  This dispatcher scope contains
                // the element's own background and border as well, so the
                // generic padding-box overflow effect must not wrap it.
                policy.effects.clear_overflow_clip_effects();
            }
            if self
                .document_canvas_overflow
                .is_viewport_overflow_source(element)
            {
                // Root/body overflow propagated to the viewport has used
                // `visible` overflow on its source element. The generic
                // stacking policy only sees computed style, so remove the
                // stale local clip at this used-value boundary.
                // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
                policy.effects.clear_overflow_clip_effects();
            }
            if matches!(layout_kind, ElementLayoutKind::Table)
                && policy.effects.overflow_clip_effect.is_some()
            {
                // Table layout owns the table-box overflow effect because it
                // has to split the table-root decoration from the grid. The
                // generic element capture includes that decoration and table
                // wrapper captions, which CSS table overflow must not clip.
                // <https://www.w3.org/Style/css2-updates/REC-CSS2-20110607-errata.html#s.11.1.1b>
                // <https://drafts.csswg.org/css-tables-3/#table-layout>
                policy.effects.clear_overflow_clip_effects();
            } else if fragment.top_level_contents_overflow_clip().is_some() {
                // The formatting context already resolved this box's
                // padding-box edge from used geometry and retained it around
                // its descendants. Reconstructing another overflow effect
                // from captured ink can duplicate the clip and substitute a
                // transformed child's source bounds for the owner's
                // scrollport.
                // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
                // <https://www.w3.org/TR/css-transforms-1/#transform-rendering>
                policy.effects.clear_overflow_clip_effects();
            }
            if let Some(crate::document::paint::contours::OverflowClipEffect::Rect(overflow_clip)) =
                policy.effects.overflow_clip_effect.take()
            {
                if matches!(policy.context_kind, StackingContextKind::None)
                    && child_contexts.is_empty()
                {
                    let scope_page = self.pages.get(page_index).unwrap_or(&self.current_page);
                    fragment = fragment
                        .with_contents_effect_scoped_to_rect_if_needed(scope_page, overflow_clip);
                    self.append_or_defer_scoped_paint_fragment(page_index, fragment);
                    continue;
                } else {
                    let scope_page = self.pages.get(page_index).unwrap_or(&self.current_page);
                    fragment = fragment
                        .with_contents_effect_scoped_to_rect_and_child_contexts_if_needed(
                            scope_page,
                            overflow_clip,
                            std::mem::take(&mut child_contexts),
                        );
                }
            }
            // Some formatting contexts own an exact geometry-dependent effect
            // internally, while this dispatcher merely provides the capture
            // boundary. Once that effect has been consumed, a policy with no
            // stacking semantics must merge its bands back into the parent;
            // wrapping it in an otherwise empty context would incorrectly make
            // earlier inline foreground paint below later sibling backgrounds.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            if matches!(policy.context_kind, StackingContextKind::None)
                && child_contexts.is_empty()
                && policy.effects == PaintEffects::default()
                && !contains_affine_3d_subtree
            {
                self.append_or_defer_scoped_paint_fragment(page_index, fragment);
                continue;
            }
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                child_contexts,
            )
            .with_source_order(source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
            let context_fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            self.append_or_defer_scoped_paint_fragment(page_index, context_fragment);
        }
    }

    /// Append scoped paint to its assigned fragmentainer, or defer it until
    /// that fragmentainer exists.
    ///
    /// Relative positioning and other non-positioned effects can capture
    /// positioned descendants whose fragments extend beyond surrounding normal
    /// flow. Binding every future context to the current page loses its page
    /// assignment and stacks continuations at one coordinate. The deferred
    /// queue preserves that assignment until page/column materialization:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(in crate::layout) fn block_static_position_rectangle_at(
        &self,
        source_block_start: PageTopBlockPosition,
    ) -> StaticPositionRectangle {
        let context = self.static_position_containing_blocks.last().copied();
        let writing_mode = context.map_or(self.containing_block_writing_mode, |context| {
            context.axes.writing_mode()
        });
        let direction = context.map_or(self.containing_block_direction, |context| {
            context.axes.direction()
        });
        let static_block_top_y = if writing_mode.has_vertical_lines() {
            source_block_start.points()
        } else {
            source_block_start.points() - self.block_static_position_y_offset.unwrap_or(0.0)
        };
        let area = if writing_mode.has_vertical_lines() {
            let x = match block_start_side(writing_mode) {
                PhysicalSide::Left => context.map_or(self.content_left, |context| {
                    context.content_rect.x() + context.content_rect.width()
                }),
                PhysicalSide::Right => {
                    context.map_or(self.content_right, |context| context.content_rect.x())
                }
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical writing mode has a horizontal block axis")
                }
            };
            PageTopRect::new(
                x,
                context.map_or(source_block_start.points(), |context| {
                    context.content_rect.top_y()
                }),
                0.0,
                context.map_or(0.0, |context| context.content_rect.height()),
            )
        } else {
            PageTopRect::new(
                self.content_left,
                static_block_top_y,
                (self.content_right - self.content_left).max(0.0),
                0.0,
            )
        };
        StaticPositionRectangle {
            area,
            writing_mode,
            direction,
            justify_items: context
                .map_or(css::SelfAlignment::NORMAL, |context| context.justify_items),
            align_items: css::SelfAlignment::NORMAL,
        }
    }

    pub(in crate::layout) fn layout_positioned_block_with_static_source(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let has_formatting_context_static_alignment = self
            .absolute_static_position
            .is_some_and(AbsoluteStaticPosition::has_formatting_context_static_alignment);
        if style.abspos_static_source.is_inline_level() && !has_formatting_context_static_alignment
        {
            // A direct inline-level positioned child of a block formatting
            // context still has an inline hypothetical box.  Measuring it
            // against the previous committed page line loses the current
            // float exclusions (and therefore `direction`/`text-align`)
            // whenever no ordinary inline sibling has painted a line yet.
            // An ambient block-flow static rectangle can likewise describe
            // an earlier in-flow sibling, never this inline-origin source.
            // Inline provenance therefore takes precedence over any saved
            // block-source rectangle.
            // Reuse the same non-painting placeholder path as collected
            // inline descendants so the hypothetical static position is
            // selected by the current line formatting context.
            //
            // The source style inherits the block's inline formatting
            // properties, which are the only block-style inputs used by this
            // empty source stream. The placeholder itself supplies the
            // blockified subject's hypothetical footprint.
            // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
            // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
            let static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                style,
                None,
                &[],
            );
            self.layout_positioned_block_with_inline_static_position(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_position,
            );
            return;
        }
        let previous_absolute_static_position = self.absolute_static_position;
        // Flex and Grid install a complete static-position alignment container
        // before entering the generic positioned-box dispatcher. An ordinary
        // block-flow rectangle is a different static-position source: replacing
        // the formatting-context source would discard its alignment defaults
        // and axes, including Grid's RTL and orthogonal-flow semantics.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        // <https://drafts.csswg.org/css-align-3/#align-abspos>
        if !style.abspos_static_source.is_inline_level()
            && !has_formatting_context_static_alignment
            && (self
                .absolute_static_position
                .and_then(AbsoluteStaticPosition::static_position_rectangle)
                .is_none()
                || self.escaped_atom_positioning_depth > 0)
        {
            let context = self.static_position_containing_blocks.last().copied();
            let writing_mode = context.map_or(self.containing_block_writing_mode, |context| {
                context.axes.writing_mode()
            });
            let direction = context.map_or(self.containing_block_direction, |context| {
                context.axes.direction()
            });
            // A block-level positioned source following a buffered inline
            // run is hypothetically placed after that run's line boxes. The
            // deferred line advance must be part of the retained static
            // rectangle itself: final abspos self-alignment resolves from
            // this rectangle and would otherwise discard a later scalar
            // static-position correction.
            // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
            let static_block_top_y = if writing_mode.has_vertical_lines() {
                self.cursor_y
            } else {
                self.cursor_y - self.block_static_position_y_offset.unwrap_or(0.0)
            };
            let area = if writing_mode.has_vertical_lines() {
                let x = match block_start_side(writing_mode) {
                    // The hypothetical block-level static rectangle is
                    // anchored to the source formatting context's used
                    // content box. `self.content_left/right` can already
                    // describe the orthogonal child being dispatched, which
                    // would select the subject's own block edge instead of
                    // the parent source edge.
                    // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
                    // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
                    PhysicalSide::Left => context.map_or(self.content_left, |context| {
                        context.content_rect.x() + context.content_rect.width()
                    }),
                    PhysicalSide::Right => {
                        context.map_or(self.content_right, |context| context.content_rect.x())
                    }
                    PhysicalSide::Top | PhysicalSide::Bottom => {
                        unreachable!("a vertical writing mode has a horizontal block axis")
                    }
                };
                PageTopRect::new(
                    x,
                    context.map_or(self.cursor_y, |context| context.content_rect.top_y()),
                    0.0,
                    context.map_or(0.0, |context| context.content_rect.height()),
                )
            } else {
                PageTopRect::new(
                    self.content_left,
                    static_block_top_y,
                    (self.content_right - self.content_left).max(0.0),
                    0.0,
                )
            };
            let rectangle = StaticPositionRectangle {
                area,
                writing_mode,
                direction,
                justify_items: context
                    .map_or(css::SelfAlignment::NORMAL, |context| context.justify_items),
                align_items: css::SelfAlignment::NORMAL,
            };
            self.absolute_static_position = Some(
                self.absolute_static_position
                    .unwrap_or_else(|| {
                        AbsoluteStaticPosition::from_page_rect(
                            self.content_left,
                            self.content_right,
                            static_block_top_y,
                        )
                    })
                    .with_static_position_rectangle(rectangle),
            );
        }
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.absolute_static_position = previous_absolute_static_position;
    }
}
