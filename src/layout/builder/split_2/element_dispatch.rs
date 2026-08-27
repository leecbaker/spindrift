use super::*;
use crate::css::ContainerType;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use crate::layout::inline_layout::InlineLayoutOutcome;

/// Continuation-local containing-block state, separate from the page selected
/// for the destination fragmentainer.
///
/// The destination page may have different `@page` geometry, but a fragmented
/// nested formatting context keeps its own local insets, percentage bases,
/// writing-mode axes, and float exclusions.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Clone)]
pub(in crate::layout) struct FragmentContinuationContext {
    pub(in crate::layout) local_offsets: FragmentOffsets,
    canvas_insets: Vec<FragmentOffsets>,
    logical_inline_sizes: Vec<f32>,
    child_available_space: Vec<ChildAvailableSpace>,
    definite_block_sizes: Vec<BlockSizePercentageBasis>,
    direction: Direction,
    writing_mode: WritingMode,
    float_contexts: Vec<FloatContext>,
    fragmentainer_kind: FragmentainerKind,
}

impl<'a> LayoutBuilder<'a> {
    /// Exits an inline page-name scope, breaking before following inline content.
    ///
    /// When inline content has already been painted on the named page, returning
    /// to the surrounding page group must create a new page box before
    /// restoring that group. Otherwise following inline content would use the
    /// previous page box's margins and page selectors:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn exit_inline_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        let Some(PageNameScope::Inline { previous_page_name }) = scope else {
            return;
        };
        if self.current_page_has_content() {
            self.push_page_if_nonempty();
        }
        self.enter_page_name_scope_for_value(previous_page_name.as_deref());
    }

    /// Suppresses CSS named-page group creation for out-of-flow and atomic layout.
    ///
    /// CSS Paged Media defines named page groups through normal-flow class A
    /// page-break boundaries. Absolutely positioned and fixed-position boxes
    /// are out of flow, while inline-block contents are laid out in an
    /// independent atomic inline formatting context; in both cases descendant
    /// `page` values do not directly select document page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>, and
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning>.
    pub(in crate::layout) fn push_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression += 1;
    }

    /// Re-enables CSS named-page group creation after suppressed layout.
    ///
    /// This closes the temporary suppression scope opened for out-of-flow or
    /// atomic inline formatting-context layout:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression = self.page_name_scope_suppression.saturating_sub(1);
    }

    /// Enters the lexical used-value scope for the CSS `page` property.
    ///
    /// CSS Paged Media resolves `page:auto` from the nearest non-`auto`
    /// ancestor. That lexical value survives a descendant's temporary page
    /// group, so it cannot be recovered from the current page cursor:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn push_page_value_scope(&mut self, style: &ComputedStyle) {
        let inherited = self.page_value_scope_stack.last().cloned().flatten();
        // An explicitly specified `page: auto` differs structurally from an
        // omitted declaration, but its *used* value is the nearest non-auto
        // ancestor page name. Keep the resolved lexical value in this stack;
        // `PageBoundaryValue::Auto` preserves the authored distinction at the
        // class-A boundary.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        let used = style.page.effective_name(inherited);
        self.page_value_scope_stack.push(used);
    }

    /// Leaves one lexical CSS `page` used-value scope.
    pub(in crate::layout) fn pop_page_value_scope(&mut self) {
        self.page_value_scope_stack.pop();
    }

    /// Resolves a class-A child boundary in the active lexical page scope.
    pub(in crate::layout) fn page_boundary_name_in_active_scope(
        &self,
        source: PageBoundaryValue,
        parent_style: &ComputedStyle,
    ) -> Option<String> {
        match source {
            PageBoundaryValue::Named(name) => Some(name),
            PageBoundaryValue::Inapplicable => None,
            PageBoundaryValue::Auto | PageBoundaryValue::Inherited => self
                .page_value_scope_stack
                .last()
                .cloned()
                .flatten()
                .or_else(|| {
                    parent_style
                        .page
                        .specified_name()
                        .map(|name| name.as_str().to_string())
                }),
        }
    }

    /// Returns the used page type inherited by a formatting-tree child.
    ///
    /// Boundary propagation resolves this once, before a class-A transition
    /// materializes a destination page. Keeping it separate from
    /// `current_page_name` prevents a preceding sibling's output page from
    /// becoming an ancestor for a later descendant's `page:auto`.
    pub(in crate::layout) fn active_page_value_scope(
        &self,
        parent_style: &ComputedStyle,
    ) -> Option<String> {
        self.page_value_scope_stack
            .last()
            .cloned()
            .flatten()
            .or_else(|| {
                parent_style
                    .page
                    .specified_name()
                    .map(|name| name.as_str().to_string())
            })
    }

    /// Suppresses element-entry named-page scopes while preserving sibling switches.
    ///
    /// Flex items do not expose their own `page` value, or descendant-derived
    /// first/last page values, to the flex container boundary. Class A break
    /// opportunities between ordinary block descendants inside the flex item
    /// still select named page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(in crate::layout) fn push_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression += 1;
    }

    /// Re-enables element-entry named-page scopes after isolated item layout.
    ///
    /// This closes the flex-item page-scope isolation described by CSS Paged
    /// Media named pages and CSS Flexbox pagination:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression =
            self.page_name_element_scope_suppression.saturating_sub(1);
    }

    /// Selects the destination named-page group at an already-established
    /// class-A boundary.
    ///
    /// The page that is being completed keeps `current_page_name`; the
    /// destination page context must instead be resolved from `page_name`
    /// before it is materialized.  Updating the cursor first made the source
    /// page acquire the destination type, while updating it after a generic
    /// page push made the destination inherit the source context.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    pub(in crate::layout) fn enter_page_name_scope_for_value(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<Option<String>> {
        if self.current_page_name.as_deref() == page_name {
            return None;
        }
        let previous = self.current_page_name.clone();
        // CSS Paged Media assigns a named page type to boxes using the `page`
        // property. The initial `auto` value is still a real page type when
        // explicitly specified, because it can end an ancestor's named page
        // group. In this cursor-based layout engine, pages occupied by the
        // scoped element inherit that page value until the element finishes.
        // https://www.w3.org/TR/css-page-3/#using-named-pages
        // A named-page boundary is a class-A break between normal-flow boxes.
        // Prior out-of-flow paint (for example a float) remains on the current
        // page but does not by itself establish a preceding page group that
        // the next in-flow box must break away from. Conversely, once a named
        // page has been selected, its out-of-flow paint materializes that page
        // context and it must be committed before a later class-A transition.
        // This keeps page-type selection separate from normal-flow geometry:
        // `page` applies to Class-A boxes even when their only paint is an
        // absolutely positioned descendant.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        // The page context records whether the source page was selected as a
        // named page independently of normal-flow occupancy. In particular,
        // positioned descendants are laid out after their source's page-value
        // boundaries have been determined, so their paint cannot be used to
        // decide whether that named page must be committed.
        let materialized_destination = self.current_page_has_named_page_flow_content
            || self.current_page_selected_name.is_some();
        let replacing_committed_empty_page =
            !materialized_destination && !self.current_page_has_content() && !self.pages.is_empty();
        let empty_page_selected_by_named_boundary = replacing_committed_empty_page
            && self.page_names.last().map(Option::as_deref)
                != Some(self.current_page_name.as_deref());
        if materialized_destination {
            self.push_page_for_page_name(page_name);
        } else if empty_page_selected_by_named_boundary {
            // Preserve the preceding empty named group's structural end
            // value while replacing its unpainted page with the successor's
            // continuation context.
            self.push_page_for_page_name(page_name);
        }
        self.current_page_name = page_name.map(str::to_string);
        // A first-page replacement occurs before its root/body fragment is
        // materialized, so it must retain that fragment's document-canvas
        // insets. A committed class-A transition already has a fresh
        // destination context from `push_page_for_page_name`; rebuilding it
        // would remeasure those source-page insets against the destination
        // page area and shift its first line.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if !materialized_destination {
            if replacing_committed_empty_page {
                if !empty_page_selected_by_named_boundary {
                    self.select_named_page_for_committed_empty_page();
                }
            } else {
                self.rebuild_empty_current_page_context();
            }
        }
        Some(previous)
    }

    pub(in crate::layout) fn exit_page_name_scope(&mut self, _scope: Option<PageNameScope>) {
        // Element scope exit only restores lexical CSS-value state (handled by
        // `pop_page_value_scope`). A page is selected exclusively by the
        // parent formatting context's class-A preceding-end/succeeding-start
        // comparison; changing `current_page_name` here manufactures an extra
        // boundary for nested page groups and `page:auto`.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    }

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
                replayed_flex_item_percentage_height_basis,
                principal_box_paint_mode,
            ),
            ElementLayoutKind::Grid => self.layout_grid_with_descendant_percentage_height_basis(
                element,
                style,
                stylesheets,
                child_boxes,
                replayed_flex_item_percentage_height_basis,
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
    fn append_or_defer_scoped_paint_fragment(
        &mut self,
        page_index: usize,
        fragment: PaintFragment,
    ) {
        if page_index < self.pages.len() {
            self.pages[page_index]
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            self.pages[page_index].sort_paint_tree_stacking_contexts();
        } else if page_index == self.pages.len() {
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            self.current_page.sort_paint_tree_stacking_contexts();
        } else {
            self.pending_paint_fragments.push(PendingPaintFragment {
                page_index,
                fragment,
                kind: PendingPaintFragmentKind::PositionedOrScoped,
            });
        }
    }

    /// Build the ordinary-flow static-position rectangle at a captured block
    /// cursor, rather than at a later mutable layout cursor.
    ///
    /// A block-flow traversal owns this source position. Out-of-flow siblings
    /// must not move it: CSS Positioned Layout derives each automatic inset
    /// from the box's hypothetical in-flow position, and absolutely positioned
    /// boxes do not participate in that flow.
    /// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
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

    pub(in crate::layout) fn layout_anonymous_block(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        marker: Option<&ListMarker>,
    ) -> bool {
        self.layout_anonymous_block_with_first_line_policy(
            style,
            children,
            stylesheets,
            marker,
            true,
            true,
        )
        .has_flow_effects
    }

    pub(in crate::layout) fn layout_anonymous_block_with_first_line_policy(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        marker: Option<&ListMarker>,
        allow_typographic_first_line: bool,
        initial_first_formatted_line: bool,
    ) -> InlineLayoutOutcome {
        let suppressed_style = (!allow_typographic_first_line)
            .then(|| style_without_typographic_first_line_pseudos(style))
            .flatten();
        let style = suppressed_style.as_ref().unwrap_or(style);
        let available_width = self.current_content_logical_inline_size().max(1.0);
        if marker.is_none()
            && anonymous_block_is_plain_text_with_style(children, style)
            // `layout_text_block` starts a fresh inline formatting context,
            // which is correct only for the originating block's first
            // anonymous run. A later run after an in-flow block must carry
            // the already-consumed first-formatted-line state through the
            // shared item formatter so it cannot restart `text-indent`.
            // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
            && initial_first_formatted_line
            && !self
                .active_float_exclusions_at(PageBlockSpan::new(self.cursor_y, style.line_height))
        {
            let text = inline_text_from_formatting_boxes(children);
            // A whitespace-only anonymous run at a line edge is discarded by
            // CSS Text whitespace processing.  It must not manufacture a
            // line box before a following float: that would move a source-
            // early float down and erase the CSS 2.2 distinction between
            // floats that occur before and after prior inline content.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            if !style.white_space.collapses_spaces()
                || !trim_css_collapsible_whitespace(&text).is_empty()
            {
                let outcome = self.layout_text_block(&text, style, 0.0, 0.0, None);
                return outcome;
            }
            return InlineLayoutOutcome::default();
        }
        let mut items = Vec::new();
        if let Some(marker) = marker
            && marker.paints_outside()
            && !self.outside_marker_anchor_is_pending(marker)
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            let anchor = self.outside_marker_fallback_anchor(
                style,
                PageInlineSpan::from_edges(self.content_left, self.content_right),
            );
            self.paint_outside_marker(marker, style, anchor);
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if let Some(marker) = marker
            && marker.participates_in_first_line()
            && !marker.follows_content_in_first_line()
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        let multicol_column_width = {
            let multicol_style = self.multicol_used_style(style);
            let style = &multicol_style;
            let gap = used_multicol_column_gap(
                style.column_gap.clone(),
                PercentageBasis::definite(content_box_pt(available_width)),
                style.font_size,
            )
            .points();
            used_multicol_column_count(style, available_width, gap)
                .filter(|count| *count > 1)
                .map(|column_count| {
                    let total_gap = gap * column_count.saturating_sub(1) as f32;
                    ((available_width - total_gap) / column_count as f32).max(1.0)
                })
        };
        let saved_content_right = self.content_right;
        if let Some(column_width) = multicol_column_width {
            // Inline atoms resolve percentage sizes during collection. A
            // multicol anonymous block therefore supplies its column box as
            // the containing-block basis before line construction.
            // <https://www.w3.org/TR/css-multicol-1/#column-box>.
            self.content_right = self.content_left + column_width;
            self.content_logical_inline_size_stack.push(column_width);
        }
        self.collect_inline_box_items(
            children,
            stylesheets,
            None,
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration_origins.effective_layers_vec(),
            &mut items,
        );
        if let Some(marker) = marker
            && marker.follows_content_in_first_line()
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        if multicol_column_width.is_some() {
            self.content_logical_inline_size_stack.pop();
            self.content_right = saved_content_right;
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if !items.is_empty() {
            let multicol_content_height =
                style.box_values.height.length_if_no_percent().or_else(|| {
                    self.definite_block_size_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(PercentageBasis::indefinite)
                        .points()
                });
            match self.try_layout_multicol_inline_items(
                items,
                style,
                available_width,
                (0.0, 0.0),
                multicol_content_height,
            ) {
                Ok(()) => {
                    return InlineLayoutOutcome {
                        next_line_index: 0,
                        clamp_line_slots: 0,
                        clamp_block_advance: Default::default(),
                        has_non_phantom_line: true,
                        has_flow_effects: true,
                        has_local_continuation_cutoff: false,
                    };
                }
                Err(returned_items) => items = returned_items,
            }
            return self.layout_inline_items_with_first_formatted_line_policy(
                items,
                style,
                available_width,
                0.0,
                0.0,
                stylesheets,
                initial_first_formatted_line,
            );
        }
        InlineLayoutOutcome::default()
    }

    pub(in crate::layout) fn layout_inline_split_block_context_with_parent_decoration(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        stylesheets: &Stylesheets<'_>,
        parent_style: Option<&ComputedStyle>,
    ) {
        let used_context_style = parent_style
            .map(|style| {
                TextDecorationPropagationContext::from_style(style)
                    .used_child_style(&context.core.style)
            })
            .unwrap_or_else(|| (*context.core.style).clone());
        let scope = self.begin_inline_split_block_paint_scope();
        self.with_inline_split_block_relative_layout_scope(Some(context), |layout| {
            for child in &context.core.children {
                let prior_line_baseline = layout.last_in_flow_line_baseline_y;
                layout.layout_formatting_box_with_parent_decoration(
                    child,
                    stylesheets,
                    Some(&used_context_style),
                );
                if child.element_parts().is_some_and(|(element, _, style, _)| {
                    layout_containment_applies_to_element(element, style)
                        && !matches!(style.position, Position::Absolute | Position::Fixed)
                        && style.float == Float::None
                }) {
                    // A layout-contained block cannot replace the preceding line
                    // baseline through the anonymous block generated by
                    // block-in-inline splitting.
                    // <https://www.w3.org/TR/css-contain-1/#containment-layout>
                    layout.last_in_flow_line_baseline_y = prior_line_baseline;
                }
            }
        });
        self.finish_inline_split_block_paint_scope(context, scope);
    }

    /// Query floats for an in-flow block fragment of a split inline in the
    /// inline's relative-positioned coordinate space.
    ///
    /// CSS 2.2 splits an inline around an in-flow block child, but the
    /// relative translation of the original inline still affects that block.
    /// In particular, float exclusions must be queried against the translated
    /// line-box span. The block itself remains in its parent flow coordinate
    /// space, so the normal-flow cursor, sibling geometry, and eventual paint
    /// translation each remain applied exactly once.
    ///
    /// The scope is deliberately entered only for the split block children;
    /// an intervening float keeps its own static placement in the parent flow.
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    /// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
    pub(in crate::layout) fn with_inline_split_block_relative_layout_scope<R>(
        &mut self,
        context: Option<&box_tree::InlineSplitBlockContextBox<'_>>,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(context) = context else {
            return layout(self);
        };
        let offset = self.normal_flow_relative_position_offset(&context.core.style);
        if offset.is_zero() {
            return layout(self);
        }

        let previous_offset = self.inline_split_float_exclusion_query_offset;
        self.inline_split_float_exclusion_query_offset = RelativeOffset {
            vector: ContainerVector::new(
                previous_offset.x() + offset.x(),
                previous_offset.y() + offset.y(),
            ),
        };

        let result = layout(self);

        self.inline_split_float_exclusion_query_offset = previous_offset;
        result
    }

    pub(in crate::layout) fn begin_inline_split_block_paint_scope(
        &mut self,
    ) -> InlineSplitBlockPaintScope {
        InlineSplitBlockPaintScope {
            page_index: self.pages.len(),
            checkpoint: self.current_page.paint_checkpoint(),
            positioned_layer_start: self.positioned_layers.len(),
            source_order: self.next_paint_source_order(),
        }
    }

    /// Lays out a float generated by a block-in-inline split while preserving
    /// the split inline ancestor as the absolute containing block.
    ///
    /// CSS 2.2 defines the containing block for an absolutely positioned box
    /// whose nearest positioned ancestor is inline as the bounding box around
    /// that inline's padding boxes. Block-in-inline normalization unwraps the
    /// block child for normal flow, so floated descendants need this temporary
    /// scope to keep absolute descendants from resolving against the outer
    /// block or page instead:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_floating_child_in_inline_split_block_context(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
        split_inline_block_offset: Option<f32>,
        pseudo_source: Option<box_tree::CounterEventSource>,
    ) -> bool {
        let pushed_containing_block = self.push_inline_split_positioning_containing_block(context);
        let saved_cursor_y = self.cursor_y;
        if let Some(offset) = split_inline_block_offset {
            self.cursor_y -= offset;
        }
        let laid_out = if let Some(pseudo_source) = pseudo_source {
            self.layout_generated_floating_child(
                child_element,
                child_signature,
                child_style,
                child_children,
                table_fragment,
                stylesheets,
                placement_axes,
                run,
                pseudo_source,
            )
        } else {
            self.layout_floating_child(
                child_element,
                child_signature,
                child_style,
                child_children,
                table_fragment,
                stylesheets,
                placement_axes,
                run,
            )
        };
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.cursor_y = saved_cursor_y;
        laid_out
    }

    /// Push the CSS absolute containing block established by a positioned
    /// inline split fragment.
    ///
    /// CSS 2.2 makes an inline positioned ancestor establish the absolute
    /// containing block from its padding boxes. For a split segment containing
    /// only a block-level child, Quire has no inline line fragment to measure,
    /// so the single-line fragment is represented by the inline padding box at
    /// the current block-flow cursor:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    pub(in crate::layout) fn push_inline_split_positioning_containing_block(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
    ) -> bool {
        let style = &context.core.style;
        if !inline_split_style_establishes_positioning_containing_block(style) {
            return false;
        }
        let border_widths = used_border_widths(style);
        let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            self.content_left + style.margin.left + border_widths.left,
            self.cursor_y - border_widths.top,
            style.padding.left + style.padding.right,
            style.line_height + style.padding.top + style.padding.bottom,
        ));
        self.containing_blocks.push(containing_block);
        true
    }

    /// Captures a block-in-inline split segment under its inline ancestor's
    /// stacking policy.
    ///
    /// CSS 2.2 splits an inline around in-flow block-level descendants, but
    /// relative positioning applies to all generated boxes for that inline and
    /// Appendix E paints a positioned inline's generated content at the inline's
    /// stack level. The layout scope makes float exclusion queries use the
    /// final visual coordinates; this method applies the corresponding paint
    /// translation once:
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>, and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(in crate::layout) fn finish_inline_split_block_paint_scope(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        scope: InlineSplitBlockPaintScope,
    ) {
        let initial_policy = StackingContextPolicy::for_non_positioned_style_effect(
            &context.core.style,
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
        );
        let child_layers = if scope.positioned_layer_start < self.positioned_layers.len()
            && !matches!(
                initial_policy.child_layer_policy,
                ChildLayerPolicy::EscapeAll
            ) {
            self.positioned_layers
                .split_off(scope.positioned_layer_start)
        } else {
            Vec::new()
        };
        let (child_layers, escaped_layers): (Vec<_>, Vec<_>) =
            match initial_policy.child_layer_policy {
                ChildLayerPolicy::CaptureAll => (child_layers, Vec::new()),
                ChildLayerPolicy::CaptureAutoLevel => child_layers
                    .into_iter()
                    .partition(|layer| matches!(layer.stack_level, StackLevel::Auto)),
                ChildLayerPolicy::EscapeAll => (Vec::new(), child_layers),
            };
        self.positioned_layers.extend(escaped_layers);

        let mut fragments =
            self.take_positioned_fragments_since(scope.page_index, scope.checkpoint);
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

        let relative_offset = self.normal_flow_relative_position_offset(&context.core.style);
        let paint_offset = PaintTranslation::new(relative_offset.x(), relative_offset.y());
        for (page_index, fragment) in fragments {
            let child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .map(|layer| {
                    layer
                        .context
                        .translated(paint_offset)
                        .with_links(layer.links)
                })
                .collect::<Vec<_>>();
            let fragment = fragment.translated(paint_offset);
            if fragment.is_empty() && child_contexts.is_empty() {
                continue;
            }
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
            let policy =
                StackingContextPolicy::for_non_positioned_style_effect(&context.core.style, bounds);
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                child_contexts,
            )
            .with_source_order(scope.source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
            let fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            self.append_or_defer_scoped_paint_fragment(page_index, fragment);
        }
    }

    #[track_caller]
    pub(in crate::layout) fn push_page(&mut self) {
        self.push_page_for_page_name(self.current_page_name.clone().as_deref());
        self.record_current_fragmentainer_destination();
    }

    /// Materializes a page transition while retaining the source page type for
    /// the page being committed and resolving the destination page context
    /// from `destination_page_name`.
    ///
    /// Named-page selection is a forced break with a page-context change.  A
    /// generic page break cannot perform it by mutating `current_page_name`
    /// on either side of the push without assigning one of the two page boxes
    /// the wrong type.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    #[track_caller]
    pub(in crate::layout) fn push_page_for_page_name(
        &mut self,
        destination_page_name: Option<&str>,
    ) {
        if self.footnote_measurement_depth == 0 {
            self.flush_current_page_footnotes();
        }
        let next_fragmentainer_index = self.pages.len() + 1;
        let next_override_context = self
            .fragmentainer_override
            .map(|override_| override_.context_for_fragmentainer(next_fragmentainer_index));
        let named_page_transition = self.current_page_name.as_deref() != destination_page_name;
        let fragment_replay_offsets = (!named_page_transition)
            .then(|| {
                self.float_fragment_parent_inline_spans
                    .last()
                    .copied()
                    .map(|parent_span| FragmentOffsets {
                        left: parent_span.left_x() - self.current_page_context.left(),
                        right: self.current_page_context.right() - parent_span.right_x(),
                        top: 0.0,
                    })
            })
            .flatten();
        if !self.current_page_has_content()
            && !self.current_page_has_named_page_flow_content
            && self.current_page_selected_name.is_none()
        {
            // CSS Fragmentation allows a box fragment to be split across
            // fragmentainers, but a carried fragment offset must not make a
            // fresh empty page permanently unfillable. If a break is requested
            // before anything painted on the current page, keep the same page
            // number and retry the fragment at the top of this page area:
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>.
            let offsets = fragment_replay_offsets.unwrap_or_else(|| FragmentOffsets {
                top: 0.0,
                ..self.current_fragment_offsets()
            });
            let context = next_override_context.unwrap_or_else(|| {
                self.resolved_page_context_for_name(
                    self.destination_document_page_number(self.pages.len() + 1),
                    false,
                    destination_page_name,
                )
            });
            let advances_to_larger_fragmentainer = self.fragmentainer_override.is_some()
                && context.area_height() > self.current_page_context.area_height() + 0.01;
            if advances_to_larger_fragmentainer {
                let next_page = page_for_context(context);
                let page = std::mem::replace(&mut self.current_page, next_page);
                self.current_page_has_flow_content = false;
                self.current_page_has_named_page_flow_content = false;
                self.pages.push(page);
                self.page_names.push(self.current_page_name.clone());
                self.page_blanks.push(false);
                self.page_named_strings
                    .push(std::mem::take(&mut self.current_page_named_strings));
                self.page_running_elements
                    .push(std::mem::take(&mut self.current_page_running_elements));
                self.apply_page_context(context, offsets);
                self.current_page_selected_name = None;
                self.truncate_page_start_margins = true;
                self.apply_pending_fragments_for_current_page();
                return;
            }
            self.current_page = page_for_context(context);
            self.current_page_has_flow_content = false;
            self.current_page_has_named_page_flow_content = false;
            self.apply_page_context(context, offsets);
            self.current_page_selected_name = None;
            self.truncate_page_start_margins = true;
            self.apply_pending_fragments_for_current_page();
            return;
        }
        // A fragmented float owns the complete paint subtree of each of its
        // fragments, including positioned descendants that cross this page
        // boundary.  Let the float harvest those layers after its child
        // layout completes rather than committing them to the source page.
        // <https://www.w3.org/TR/css-break-3/#breaks-between>
        if self.float_paint_capture_depth == 0 {
            self.flush_positioned_layers();
        }
        let offsets = if named_page_transition {
            // A class-A named-page boundary is a forced page break in the
            // current block-fragmentation context. Re-enter that context with
            // the same normalized continuation origin as an explicit
            // `break-before: page`; a raw page-area offset would retain the
            // source root/body canvas rather than the destination fragment's
            // canvas translation.
            //
            // This is deliberately distinct from the empty-page replacement
            // above. Before a page has been committed there is no preceding
            // root/body fragment to continue.
            self.block_page_break_continuation_context().local_offsets
        } else {
            fragment_replay_offsets
                .unwrap_or_else(|| self.current_fragment_offsets_for_page_break())
        };
        let next_context = next_override_context.unwrap_or_else(|| {
            self.resolved_page_context_for_name(
                self.destination_document_page_number(self.pages.len() + 2),
                false,
                destination_page_name,
            )
        });
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.current_page_selected_name = None;
        self.truncate_page_start_margins = true;
        self.apply_pending_fragments_for_current_page();
    }

    pub(in crate::layout) fn push_blank_page(&mut self) {
        // CSS Fragmentation forced left/right/recto/verso breaks can generate
        // blank pages. Those pages are real page boxes and match `@page :blank`.
        // https://www.w3.org/TR/css-break-3/#break-between
        let page_number = self.destination_document_page_number(self.pages.len() + 1);
        let context = self.resolved_page_context(page_number, true);
        self.pages.push(page_for_context(context));
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(true);
        self.page_named_strings.push(HashMap::new());
        self.page_running_elements.push(HashMap::new());
    }

    #[track_caller]
    pub(in crate::layout) fn push_page_if_nonempty(&mut self) {
        if self.current_page_has_content() {
            self.push_page();
        }
    }

    /// Capture the nested containing block before selecting a destination
    /// page or column. Page selection itself remains the responsibility of
    /// the ordinary fragmentation transition.
    pub(in crate::layout) fn fragment_continuation_context(&self) -> FragmentContinuationContext {
        FragmentContinuationContext {
            // This context is replayed after the destination page has been
            // selected. Preserve the actual page-local containing-block
            // edges, including root/body canvas insets, rather than the
            // generic page-break offsets which intentionally subtract those
            // insets for ordinary root-flow continuation.
            local_offsets: FragmentOffsets {
                left: self.content_left - self.current_page_context.left(),
                right: self.current_page_context.right() - self.content_right,
                top: 0.0,
            },
            canvas_insets: self.document_canvas_fragment_insets.clone(),
            logical_inline_sizes: self.content_logical_inline_size_stack.clone(),
            child_available_space: self.child_available_space_stack.clone(),
            definite_block_sizes: self.definite_block_size_stack.clone(),
            direction: self.containing_block_direction,
            writing_mode: self.containing_block_writing_mode,
            float_contexts: self.float_contexts.clone(),
            fragmentainer_kind: self.active_fragmentainer_kind(),
        }
    }

    /// Capture an in-flow block retry's page continuation.
    ///
    /// Unlike table-row slices, a nested block retry re-enters each fragment's
    /// root/body canvas. Its local offsets must therefore retain the complete
    /// ordinary page-break continuation origin, including the canvas's inline
    /// insets, so replay starts at the same position as normal in-flow page
    /// continuation.
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout) fn block_page_break_continuation_context(
        &self,
    ) -> FragmentContinuationContext {
        let mut continuation = self.fragment_continuation_context();
        // `current_fragment_offsets_for_page_break` already restores the
        // document-canvas inline insets needed by a real page continuation.
        // Subtracting them here a second time shifts retried avoided blocks
        // outside the propagated root/body canvas on their destination page.
        continuation.local_offsets = self.current_fragment_offsets_for_page_break();
        continuation
    }

    /// Capture a float's destination context.
    ///
    /// A root-flow float crosses the same canvas boundary as ordinary in-flow
    /// page content. A nested float instead keeps its narrower local
    /// containing block (for example an overflow-clipped formatting context).
    pub(in crate::layout) fn float_page_break_continuation_context(
        &self,
    ) -> FragmentContinuationContext {
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        let root_flow_width =
            (self.content_left - self.current_page_context.left() - canvas.left).abs() <= 0.01
                && (self.current_page_context.right() - self.content_right - canvas.right).abs()
                    <= 0.01;
        if root_flow_width {
            // The deferred float is replayed in isolation after its parent
            // flow has remained on the source page. Unlike an ordinary
            // in-flow page break, no root/body layout pass will re-enter the
            // document canvas before the float is placed. Preserve the
            // actual root-flow insets here so the destination margin box has
            // the same containing block as an equivalent forced-break block.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            let mut continuation = self.fragment_continuation_context();
            // Float exclusion rectangles are page-local. A root-flow float
            // moved to a fresh page must not be placed beside a float from
            // the preceding page.
            // The context vector also encodes active float-formatting scopes;
            // retain those frames so subsequent placement always has a root
            // context. Only the exclusion shapes themselves belong to the
            // preceding page.
            for context in &mut continuation.float_contexts {
                context.shapes.clear();
            }
            continuation
        } else {
            self.fragment_continuation_context()
        }
    }

    /// Apply a captured continuation to an already-selected destination page.
    ///
    /// `push_page` carries mutable layout stacks until replay. Reinstall the
    /// captured continuation state before applying the destination page so
    /// page-area rebasing starts from the source formatting context rather
    /// than from a partially advanced sibling or scratch fragment. The page
    /// context then recalculates only entries that genuinely represent the
    /// destination page area.
    pub(in crate::layout) fn replay_fragment_continuation_on_page(
        &mut self,
        continuation: &FragmentContinuationContext,
        destination: PageContext,
    ) {
        debug_assert_eq!(continuation.fragmentainer_kind, FragmentainerKind::Page);
        debug_assert_eq!(self.active_fragmentainer_kind(), FragmentainerKind::Page);
        self.document_canvas_fragment_insets = continuation.canvas_insets.clone();
        self.content_logical_inline_size_stack = continuation.logical_inline_sizes.clone();
        self.child_available_space_stack = continuation.child_available_space.clone();
        self.definite_block_size_stack = continuation.definite_block_sizes.clone();
        self.containing_block_direction = continuation.direction;
        self.containing_block_writing_mode = continuation.writing_mode;
        self.float_contexts = continuation.float_contexts.clone();

        self.current_page = page_for_context(destination);
        self.apply_page_context(destination, continuation.local_offsets);
        self.current_page_selected_name = None;
    }

    /// Captures the active formatting-context insets from the current page area.
    ///
    /// A page break fragments boxes without leaving their containing block, while
    /// a named-page transition can select a different page area. Keeping these
    /// offsets preserves ancestor margins and padding on the new page fragment:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn current_fragment_offsets(&self) -> FragmentOffsets {
        let raw = FragmentOffsets {
            left: self.content_left - self.current_page_context.left(),
            right: self.current_page_context.right() - self.content_right,
            top: self
                .fragment_top_offsets
                .last()
                .map(|offset| offset.first_fragment_start())
                .unwrap_or_else(|| self.current_page_context.top() - self.cursor_y),
        };
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        FragmentOffsets {
            left: raw.left - canvas.left,
            right: raw.right - canvas.right,
            top: raw.top - canvas.top,
        }
    }

    /// Captures fragment insets for an actual page break.
    ///
    /// The next fragment keeps inline containing-block insets, including the
    /// root/body canvas's inline margins, but starts at the block-start edge
    /// of the new fragmentainer. CSS Fragmentation's initial
    /// `box-decoration-break: slice` behavior does not clone ancestor
    /// block-start margin, border, or padding into continuation fragments:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>.
    pub(in crate::layout) fn current_fragment_offsets_for_page_break(&self) -> FragmentOffsets {
        // A multicolumn implementation uses temporary page contexts as
        // fragmentainers. Their contexts already encode the real page's
        // canvas margins, so subtracting document-canvas insets again shifts
        // each continuation column horizontally. Preserve only the local
        // containing-block inset when advancing a synthetic column page.
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        if self
            .fragmentainer_override
            .is_some_and(|override_| override_.kind == FragmentainerKind::Column)
        {
            return FragmentOffsets {
                left: self.content_left - self.current_page_context.left(),
                right: self.current_page_context.right() - self.content_right,
                // Synthetic column fragmentainers retain their local inline
                // containing block, but `clone` still restarts every active
                // block below its cloned block-start border and padding.
                // Returning a raw zero here made definite blocks consume
                // that start edge as source content, leaving the following
                // sibling fifteen CSS pixels too high in clone-004.
                top: self
                    .fragment_top_offsets
                    .iter()
                    .map(|offset| offset.continuation_start())
                    .sum(),
            };
        }
        let mut offsets = self.current_fragment_offsets();
        // `current_fragment_offsets` removes active document-canvas insets
        // so an isolated fragment replay can reconstruct its own canvas. A
        // real root-flow page continuation instead re-enters that canvas on
        // the destination page, so retain its logical-inline insets here.
        // Otherwise an ordinary body margin disappears after the first page
        // even though the page area itself changes correctly.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        offsets.left += canvas.left;
        offsets.right += canvas.right;
        // Reset the exhausted source *block-start* coordinate only. In a
        // vertical principal flow, clearing both physical horizontal insets
        // incorrectly widens the continuation and loses the root/body
        // block-end inset; `vertical-rl` restarts from the right and
        // `vertical-lr` from the left.
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        // <https://www.w3.org/TR/css-break-4/#box-splitting>
        offsets.clear_fragmentainer_block_start(FlowAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        ));
        // A cloned ancestor creates a fresh border/padding inset in every
        // continuation.  The regular fragment-offset reset above implements
        // `slice`; reapply only the explicitly recorded clone start edges.
        // These are physical top insets because this page-flow path is the
        // horizontal principal-flow continuation boundary. Vertical roots
        // use their dedicated logical page-fragmentation projection instead.
        if self.principal_flow.writing_mode == WritingMode::HorizontalTb {
            offsets.top += self
                .fragment_top_offsets
                .iter()
                .map(|offset| offset.continuation_start())
                .sum::<f32>();
        }
        offsets
    }

    /// Applies a new page context while preserving active fragment insets.
    ///
    /// CSS Paged Media changes the page area's size and margins per page, but
    /// CSS Fragmentation keeps content in the same containing block across page
    /// fragments:
    /// <https://www.w3.org/TR/css-page-3/#page-model> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn apply_page_context(
        &mut self,
        context: PageContext,
        offsets: FragmentOffsets,
    ) {
        let previous_context = self.current_page_context;
        self.current_page_context = context;
        self.current_page.rotation = context.rotation;
        self.cursor_y = context.top() - offsets.top;
        self.content_left = context.left() + offsets.left;
        self.content_right = (context.right() - offsets.right).max(self.content_left);
        self.rebase_page_area_context_caches(previous_context, context);
    }

    /// Selects the first destination fragmentainer for an out-of-flow scratch
    /// layout without changing the already-resolved physical box geometry.
    ///
    /// Absolutely positioned boxes resolve their insets in their continuous
    /// containing block, then fragment their contents through destination
    /// page areas.  When the static-position rectangle begins on a later
    /// page, the scratch layout must use that page's percentage bases and
    /// continuation dimensions from its first fragment.  Its physical cursor
    /// and containing-block coordinates have already been resolved, however,
    /// and must not be reset to the page-area origin.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    pub(in crate::layout) fn rebase_positioned_scratch_page_context(
        &mut self,
        context: PageContext,
    ) {
        let previous_context = self.current_page_context;
        self.current_page_context = context;
        self.current_page = page_for_context(context);
        self.rebase_page_area_context_caches(previous_context, context);
    }

    /// Updates active parent caches that directly represent the page area.
    ///
    /// A page transition can select a different page size. Root-level
    /// auto-sized formatting contexts use cached page-area dimensions while
    /// their descendants are being laid out, so those exact page-area entries
    /// must change with the context before the descendant's used percentages
    /// are resolved.
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    fn rebase_page_area_context_caches(
        &mut self,
        previous_context: PageContext,
        next_context: PageContext,
    ) {
        const EPSILON: f32 = 0.01;
        if previous_context == next_context {
            return;
        }
        let active_page_writing_mode = self
            .child_available_space_stack
            .last()
            .map(|space| space.writing_mode)
            .unwrap_or(WritingMode::HorizontalTb);
        if self
            .content_logical_inline_size_stack
            .last()
            .is_some_and(|size| {
                (*size - previous_context.logical_inline_size(active_page_writing_mode)).abs()
                    <= EPSILON
            })
            && let Some(size) = self.content_logical_inline_size_stack.last_mut()
        {
            *size = next_context.logical_inline_size(active_page_writing_mode);
        }
        let page_available_space = ChildAvailableSpace::new(
            active_page_writing_mode,
            PhysicalContentWidth::new(content_box_pt(next_context.area_width())),
            true,
            Some(PhysicalContentHeight::new(content_box_pt(
                next_context.area_height(),
            ))),
            self.initial_containing_block_physical_height(),
        );
        if self
            .child_available_space_stack
            .last()
            .is_some_and(|space| {
                space.writing_mode == active_page_writing_mode
                    && (space.physical_content_width.points() - previous_context.area_width()).abs()
                        <= EPSILON
                    && (space.available_physical_height().points() - previous_context.area_height())
                        .abs()
                        <= EPSILON
            })
            && let Some(space) = self.child_available_space_stack.last_mut()
        {
            *space = page_available_space;
        }
    }

    /// Restores an enclosing page-area formatting context after a child caused
    /// a page transition with a different used page size.
    ///
    /// CSS Paged Media resolves each page's containing block from that page's
    /// used page area. An `html`/`body`-like auto-sized block that filled the
    /// preceding page area must therefore fill the new page area after a child
    /// fragments; restoring its old physical rectangle would retain the prior
    /// page width for all following siblings.
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    pub(in crate::layout) fn restore_page_area_parent_context_after_page_transition(
        &mut self,
        previous_left: f32,
        previous_right: f32,
        page_context_at_entry: PageContext,
        page_index_at_entry: usize,
    ) {
        const EPSILON: f32 = 0.01;
        let filled_previous_page_area = (previous_left - page_context_at_entry.left()).abs()
            <= EPSILON
            && (previous_right - page_context_at_entry.right()).abs() <= EPSILON;
        if self.pages.len() != page_index_at_entry {
            // The child committed a page boundary. `apply_page_context` has
            // already installed the destination's continuation origin; in
            // particular, it has removed the source root/body canvas inset.
            // Restoring `previous_left` here would reintroduce that source
            // offset and make later siblings start at a different position
            // from the equivalent explicit forced break.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            return;
        }
        if self.current_page_context != page_context_at_entry && filled_previous_page_area {
            // The outer root/body canvas re-enters each page fragment, but a
            // named-page transition must not rebuild the destination from a
            // source fragment's generic offsets. Reapply only that stable
            // canvas inset here, after the new page area's geometry is known.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let canvas = self.document_canvas_fragment_insets.iter().fold(
                FragmentOffsets::ZERO,
                |total, inset| FragmentOffsets {
                    left: total.left + inset.left,
                    right: total.right + inset.right,
                    top: total.top,
                },
            );
            self.content_left = self.current_page_context.left() + canvas.left;
            self.content_right =
                (self.current_page_context.right() - canvas.right).max(self.content_left);
        } else {
            self.content_left = previous_left;
            self.content_right = previous_right;
        }
    }

    pub(in crate::layout) fn apply_forced_break(&mut self, forced_break: PageBreak) {
        if !FragmentainerKind::Page.is_forced_break(forced_break) {
            return;
        }
        let current_empty_named_destination = !self.current_page_has_content()
            && self.page_names.last().map(Option::as_deref)
                != Some(self.current_page_name.as_deref());
        if self.current_page_has_content() {
            self.push_page();
        }
        while !forced_break_satisfied(
            forced_break,
            self.destination_document_page_number(self.pages.len() + 1),
            self.page_progression_direction,
        ) {
            self.push_blank_page();
        }
        if !self.current_page_has_content() && !current_empty_named_destination {
            let offsets = self.current_fragment_offsets_for_page_break();
            let page_number = self.destination_document_page_number(self.pages.len() + 1);
            let context = self.resolved_page_context(page_number, false);
            self.current_page = page_for_context(context);
            self.apply_page_context(context, offsets);
            self.current_page_selected_name = None;
        }
        // At a forced break, adjoining margins before the break are
        // truncated, but margins after the break are preserved. The box that
        // follows this boundary is therefore unlike a continuation placed at
        // an unforced fragmentainer break, whose block-start margin is
        // truncated.
        // <https://www.w3.org/TR/css-break-3/#break-margins>
        self.truncate_page_start_margins = false;
    }

    pub(in crate::layout) fn apply_forced_break_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        forced_break: PageBreak,
    ) {
        // Callers pass the resolved outgoing break value so that a flex, grid,
        // or table item can leave `auto` for its following sibling. Only a
        // value forced in the active fragmentainer kind may materialize a
        // continuation; treating `auto` as a column transition manufactures
        // an empty anonymous column after every such box.
        // <https://www.w3.org/TR/css-break-3/#forced-breaks>
        if !fragmentainer_kind.is_forced_break(forced_break) {
            return;
        }
        if self.fragmentation_suppression_depth > 0 {
            return;
        }
        if fragmentainer_kind == FragmentainerKind::Column
            && self
                .fragmentainer_override
                .is_some_and(|override_| override_.kind == FragmentainerKind::Column)
        {
            self.materialize_column_continuation();
            return;
        }
        if !fragmentainer_kind.materializes_page_cursor() {
            return;
        }
        self.apply_forced_break(forced_break);
    }

    /// Apply this generated box's `break-before` in the active fragmentainer.
    ///
    /// CSS Fragmentation defines `break-before` generically across
    /// fragmentainer types. Quire currently materializes only page transitions
    /// at this builder layer, but the break value is still resolved through the
    /// shared target-aware break context so column-specific values remain
    /// ignored here rather than accidentally treated as page breaks:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn apply_forced_break_before_box_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        style: &ComputedStyle,
    ) {
        if let Some(forced_break) = FragmentBreakContext::for_standalone_box(style)
            .forced_break_before_in(fragmentainer_kind)
        {
            self.apply_forced_break_in(fragmentainer_kind, forced_break);
        }
    }

    /// Apply this generated box's `break-after` in the active fragmentainer.
    ///
    /// This is the exit-boundary counterpart to
    /// `apply_forced_break_before_box_in`; layout modes that carry descendant
    /// forced breaks should resolve the fallback through
    /// `FragmentBreakContext::forced_break_after_or_in` before calling the
    /// page transition primitive:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn apply_forced_break_after_box_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        style: &ComputedStyle,
    ) {
        if let Some(forced_break) = FragmentBreakContext::for_standalone_box(style)
            .forced_break_after_in(fragmentainer_kind)
        {
            // A forced break establishes a page boundary after this box even
            // when the box has no paint (for example, an empty `min-height`
            // block). Retain that completed fragmentainer during document
            // finalization; generic trailing geometry without a forced
            // boundary remains eligible for omission.
            // <https://www.w3.org/TR/css-break-3/#forced-breaks>
            if self.current_page_has_flow_content {
                self.current_page.mark_fragmentation_content();
            }
            // An out-of-flow-only source box can have no normal-flow cursor
            // effect, while its positioned paint is still owned by this page.
            // Commit that paint before deciding whether a forced side break
            // needs a new page; otherwise the break is treated as if it
            // occurred at document start and the positioned content is
            // replayed into a later sibling's page.
            // <https://www.w3.org/TR/css-break-3/#breaks-between>
            if fragmentainer_kind.materializes_page_cursor() && !self.current_page_has_content() {
                self.flush_positioned_layers();
            }
            self.apply_forced_break_in(fragmentainer_kind, forced_break);
        }
    }

    pub(in crate::layout) fn current_page_has_content(&self) -> bool {
        self.current_page.has_paint_content() || self.current_page_has_flow_content
    }

    pub(in crate::layout) fn active_fragmentainer_kind(&self) -> FragmentainerKind {
        self.fragmentainer_override
            .map(|override_| override_.kind)
            .unwrap_or(FragmentainerKind::Page)
    }

    /// Return whether a transition for `kind` has a concrete cursor-backed
    /// fragmentainer in the active layout scope.
    ///
    /// Page layout is always cursor-backed. The multicol engine also installs
    /// page-shaped anonymous column canvases, so column transitions inside
    /// that scope must materialize just like page transitions.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn fragmentainer_materializes_cursor(
        &self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.materializes_page_cursor()
            || self
                .fragmentainer_override
                .is_some_and(|override_| override_.kind == kind)
    }

    /// Marks the current page as carrying source-owned normal-flow content.
    ///
    /// CSS Fragmentation fragments boxes into page fragmentainers even when a
    /// particular fragment has no visible paint. A used border box with
    /// positive area, or a zero-size box placed after clearance in a new
    /// fragmentainer, therefore participates in forced breaks independently
    /// from PDF paint primitives. At document
    /// finalization, a trailing run with no paint or page-owning side effect
    /// can be omitted from static PDF output:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn mark_current_page_flow_content(&mut self) {
        self.current_page_has_flow_content = true;
        self.current_page_has_named_page_flow_content = true;
        // An explicit named-page assignment is observable even when its
        // normal-flow box contributes geometry but no paint: it selects the
        // page box, including its size and page rules. Preserve every
        // fragmentainer actually occupied by that named flow rather than
        // discarding it as an unpainted trailing geometry page.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if self.current_page_name.is_some() {
            self.current_page.mark_fragmentation_content();
        }
    }

    pub(in crate::layout) fn page_left(&self) -> f32 {
        self.current_page_context.left()
    }

    pub(in crate::layout) fn page_top(&self) -> f32 {
        self.current_page_context.top()
    }

    /// Whether the active traversal resolves an automatic positioned block
    /// size before the positioned box has been fragmented.
    pub(in crate::layout) fn is_positioned_auto_size_measurement(&self) -> bool {
        self.layout_pass_kind == LayoutPassKind::PositionedAutoSizeMeasurement
    }

    pub(in crate::layout) fn page_bottom(&self) -> f32 {
        if self.fragmentation_suppression_depth > 0 || self.footnote_measurement_depth > 0 {
            self.current_page_context.bottom() - 1_000_000.0
        } else {
            self.current_page_context.bottom()
                + self
                    .footnote_reservations
                    .get(&self.pages.len())
                    .copied()
                    .unwrap_or(0.0)
                // Every active cloned block owns its block-end padding and
                // border in this fragmentainer. Keep that reservation in the
                // layout capacity so descendants cannot consume the space
                // that its principal-box decoration must occupy.
                // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
                + self
                    .fragment_top_offsets
                    .iter()
                    .map(|offset| offset.continuation_end())
                    .sum::<f32>()
        }
    }

    pub(in crate::layout) fn page_area_width(&self) -> f32 {
        self.current_page_context.area_width()
    }

    pub(in crate::layout) fn page_area_height(&self) -> f32 {
        self.page_top() - self.page_bottom()
    }

    /// The physical block-axis size of the document initial containing block.
    ///
    /// This is the immutable initial printable page area, not the remaining
    /// extent of the current fragmentainer. Orthogonal-flow line fitting falls
    /// back to this size after direct and scroll-container candidates have
    /// been exhausted.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn initial_containing_block_physical_height(
        &self,
    ) -> PhysicalContentHeight {
        PhysicalContentHeight::new(content_box_pt(self.initial_viewport_context.area_height()))
    }

    pub(in crate::layout) fn current_content_logical_inline_size(&self) -> f32 {
        self.content_logical_inline_size_stack
            .last()
            .cloned()
            .unwrap_or_else(|| (self.content_right - self.content_left).max(0.0))
    }

    /// Return the active containing block's logical inline content-box size.
    ///
    /// The stack is still scalar while legacy inline collection is migrated,
    /// but consumers resolving CSS percentage edges must cross through this
    /// typed boundary rather than treating the value as a physical width.
    pub(in crate::layout) fn current_content_logical_inline_content_size(
        &self,
    ) -> LogicalInlineContentSize {
        LogicalInlineContentSize::new(content_box_pt(self.current_content_logical_inline_size()))
    }

    /// Return the active containing block's definite logical inline basis for
    /// CSS edge-percentage resolution.
    pub(in crate::layout) fn current_content_logical_inline_percentage_basis(
        &self,
    ) -> LogicalInlinePercentageBasis {
        PercentageBasis::definite(self.current_content_logical_inline_content_size())
    }

    pub(in crate::layout) fn page_child_available_space(&self) -> ChildAvailableSpace {
        ChildAvailableSpace::new(
            // The initial containing block takes the principal writing mode
            // from the document root. Its physical dimensions remain the page
            // area's dimensions, but treating a vertical root as orthogonal
            // would incorrectly shrink its auto inline size to its contents.
            // https://www.w3.org/TR/css-writing-modes-4/#principal-flow
            self.initial_containing_block_writing_mode,
            PhysicalContentWidth::new(content_box_pt(self.page_area_width())),
            true,
            Some(PhysicalContentHeight::new(content_box_pt(
                self.page_area_height(),
            ))),
            self.initial_containing_block_physical_height(),
        )
    }

    pub(in crate::layout) fn current_child_available_space(&self) -> ChildAvailableSpace {
        self.child_available_space_stack
            .last()
            .cloned()
            .unwrap_or_else(|| self.page_child_available_space())
    }

    pub(in crate::layout) fn resolved_page_context(
        &mut self,
        page_number: usize,
        is_blank: bool,
    ) -> PageContext {
        let page_name = self.current_page_name.clone();
        self.resolved_page_context_for_name(page_number, is_blank, page_name.as_deref())
    }

    /// Convert a scratch-local 1-based page ordinal into the page number of
    /// its eventual document destination. Normal flow has no scratch origin,
    /// so its ordinal is already the document page number.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-abspos>
    pub(in crate::layout) fn destination_document_page_number(
        &self,
        local_page_number: usize,
    ) -> usize {
        self.positioned_scratch_page_origin
            .map_or(local_page_number, |origin| origin.get() + local_page_number)
    }

    /// Resolves a concrete destination page context without changing the page
    /// type of the source page currently being committed.
    ///
    /// A named-page transition selects its destination before it materializes
    /// the new page box, while the previous page retains its existing named
    /// type for `@page` matching and final decoration.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    pub(in crate::layout) fn resolved_page_context_for_name(
        &mut self,
        page_number: usize,
        is_blank: bool,
        page_name: Option<&str>,
    ) -> PageContext {
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let page_style = self.page_context_style_for_declarations(&declarations);
        let ch_advance = self.ch_advance_for_style(&page_style, page_style.requires_ch_advance());
        // The first empty page context is needed before the document root is
        // traversed, so root-relative page lengths cannot yet use the selected
        // root font. Bootstrap with the page style's initial metric estimates;
        // `layout_dom_with_font_system` rebuilds this still-empty context as
        // soon as document-root metrics have been established.
        // <https://www.w3.org/TR/css-values-4/#root-relative-fonts>
        let root_metrics =
            self.root_metric_state
                .font_size_basis()
                .unwrap_or(css::RootFontMetricLengthBasis {
                    font_size: layout_pt(page_style.font_size),
                    ch_advance,
                    x_height: layout_pt(page_style.font_size * 0.5),
                    cap_height: layout_pt(page_style.font_size * 0.7),
                    ic_advance: ch_advance,
                    line_height: layout_pt(page_style.line_height),
                });
        // CSS Paged Media defines page size and page margins in the page
        // context; these declarations select the page box before its content
        // area is used for layout.
        // https://www.w3.org/TR/css-page-3/#page-model
        let size = css::page_size_from_with_ch_advance_and_root_metrics(
            &declarations,
            base.size,
            ch_advance,
            root_metrics,
        );
        let page_edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
            &declarations,
            size,
            ch_advance,
            root_metrics,
        );
        PageContext {
            size,
            margins:
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
                    &declarations,
                    base.margins,
                    size,
                    css::PageMarginResolutionContext {
                        viewport_size: self.page_descriptor_viewport_size,
                        non_margin_edges: page_edges.total(),
                        ch_advance,
                        style: &page_style,
                        root_metrics,
                    },
                ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    pub(in crate::layout) fn finished_page_context(
        &mut self,
        page_number: usize,
        page_size: PageSize,
    ) -> PageContext {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let page_style = self.page_context_style_for_declarations(&declarations);
        let ch_advance = self.ch_advance_for_style(&page_style, page_style.requires_ch_advance());
        let root_metrics = self.root_metric_state.resolved().basis();
        let page_edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
            &declarations,
            page_size,
            ch_advance,
            root_metrics,
        );
        PageContext {
            size: page_size,
            margins:
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
                    &declarations,
                    base.margins,
                    page_size,
                    css::PageMarginResolutionContext {
                        viewport_size: self.page_descriptor_viewport_size,
                        non_margin_edges: page_edges.total(),
                        ch_advance,
                        style: &page_style,
                        root_metrics,
                    },
                ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    /// Builds the inherited page context used for logical page properties.
    pub(in crate::layout) fn page_context_style_for_declarations(
        &self,
        declarations: &Declarations,
    ) -> ComputedStyle {
        let mut style = self.page_margin_inherited_style.clone();
        css::apply_declarations_with_inheritance_source(
            &mut style,
            declarations,
            &self.page_margin_inherited_style,
        );
        style
    }

    pub(in crate::layout) fn rebuild_empty_current_page_context(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let mut offsets = if self.pages.is_empty() {
            // Before the first page is materialized, a descendant may select
            // its named page from inside an ancestor's first fragment. Keep
            // that ancestor's initial block-start inset.
            self.current_fragment_offsets()
        } else {
            // A named-page selection before its first in-flow descendant is
            // laid out establishes a new page-area containing block. Retain
            // the ancestor offsets while rebuilding that empty context so the
            // document root/body margin is not lost merely because the page
            // name changed.
            // https://www.w3.org/TR/css-page-3/#using-named-pages
            self.current_fragment_offsets()
        };
        // Page-context replacement measures the current content edge against
        // the old page area's edge. When that old page has a larger margin,
        // that transient measurement is negative even though the active
        // ancestor inset (for example the body's used margin) is positive.
        // Fragment insets are distances, so retain their magnitude across
        // the page-area change.
        offsets.left = offsets.left.abs();
        offsets.right = offsets.right.abs();
        // The document canvas' block-start inset is intentionally removed
        // from normal fragment accounting, because it is not cloned at an
        // ordinary continuation. A named-page replacement is different: the
        // same root/body fragment continues in a new page area, so preserve
        // its used block-start margin.
        offsets.top += self
            .document_canvas_fragment_insets
            .iter()
            .map(|inset| inset.top)
            .sum::<f32>();
        let page_number = self.destination_document_page_number(self.pages.len() + 1);
        let context = self.resolved_page_context(page_number, false);
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
        self.current_page_selected_name = self.current_page_name.clone();
        // A first in-flow box can select a named page before it emits any
        // content. CSS viewport units use that first actual page's initial
        // containing block, not the renderer's provisional default page.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        if self.pages.is_empty() {
            self.initial_viewport_context = context;
        }
    }

    /// Selects a named page type for an already-committed, otherwise empty
    /// destination page.
    ///
    /// A forced break can materialize its destination before the succeeding
    /// class-A box supplies its page value. That page is not a first-page
    /// replacement: it is a continuation fragment, so its root/body canvas
    /// origin must remain the one installed by the forced break. Rebuilding
    /// it through the initial-page path would add that inset again.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    fn select_named_page_for_committed_empty_page(&mut self) {
        debug_assert!(!self.pages.is_empty());
        debug_assert!(!self.current_page_has_content());

        let previous_context = self.current_page_context;
        let offsets = FragmentOffsets {
            left: self.content_left - previous_context.left(),
            right: previous_context.right() - self.content_right,
            top: previous_context.top() - self.cursor_y,
        };
        let page_name = self.current_page_name.clone();
        let context = self.resolved_page_context_for_name(
            self.destination_document_page_number(self.pages.len() + 1),
            false,
            page_name.as_deref(),
        );
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
        self.current_page_selected_name = self.current_page_name.clone();
    }

    pub(in crate::layout) fn has_renderable_content(&self) -> bool {
        !self.pages.is_empty()
            || self.current_page_has_content()
            || !self.positioned_layers.is_empty()
            || !self.fixed_layers.is_empty()
            || self
                .page_rules
                .iter()
                .any(|rule| !rule.margin_boxes.is_empty())
    }

    pub(in crate::layout) fn cursor_is_at_page_top(&self) -> bool {
        (self.cursor_y - self.page_top()).abs() < 0.01
    }

    /// Resolves a captured assignment to the first source fragment's final page.
    ///
    /// CSS GCPM `start` lookups are based on the source fragment at the page
    /// boundary, not on the earlier style/counter capture point. If layout
    /// pushes a page after capture, the original page checkpoint tells whether
    /// the source painted there or moved wholly to the new current page:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
    pub(in crate::layout) fn final_source_assignment_placement(
        &self,
        style: &ComputedStyle,
        captured_page_index: usize,
        captured_paint_checkpoint: PaintCheckpoint,
        captured_starts_page_fragment: bool,
        captured_content_left: f32,
        captured_cursor_y: f32,
    ) -> AssignmentPlacement {
        let height = style.line_height.max(0.0);
        let width = (self.content_right - self.content_left).max(0.0);
        if captured_page_index < self.pages.len() {
            let original_page_changed =
                self.pages[captured_page_index].paint_checkpoint() != captured_paint_checkpoint;
            if original_page_changed {
                return AssignmentPlacement {
                    page_index: captured_page_index,
                    starts_page_fragment: captured_starts_page_fragment,
                    border_box: Some(
                        PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                            .paint_clip(),
                    ),
                };
            }
            return AssignmentPlacement {
                page_index: self.pages.len(),
                starts_page_fragment: true,
                border_box: Some(
                    PageTopRect::new(self.content_left, self.page_top(), width, height)
                        .paint_clip(),
                ),
            };
        }
        AssignmentPlacement {
            page_index: captured_page_index,
            starts_page_fragment: captured_starts_page_fragment,
            border_box: Some(
                PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                    .paint_clip(),
            ),
        }
    }
}

fn inline_split_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
}
