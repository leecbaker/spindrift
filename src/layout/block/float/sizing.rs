use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::super::super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct AutoFloatMeasurementKey {
    element: ElementId,
    content_width_bits: u32,
    border_box_width_bits: u32,
    margin_box_width_bits: u32,
    style_fingerprint: u64,
    percentage_basis_fingerprint: u64,
    counter_fingerprint: u64,
    quote_depth: usize,
    page_index: usize,
}

fn debug_fingerprint(value: &impl std::fmt::Debug) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{value:?}").hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedFloatInlineSize {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_width: BorderBoxLength,
    pub(in crate::layout) margin_box_width: MarginBoxLength,
}

/// A float's content size retained in its own logical axes before projection
/// into the containing block's physical float-placement geometry.
///
/// This distinction is essential for orthogonal floats: their definite
/// logical inline size is physical height, while shrink-to-fit physical width
/// is their measured logical block contribution.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatLogicalContentSize {
    inline: LogicalInlineContentSize,
    block: LogicalBlockContentSize,
}

impl FloatLogicalContentSize {
    fn physical_width(self, axes: FlowAxes) -> PhysicalContentWidth {
        axes.physical_width_from_logical_content_sizes(self.inline, self.block)
    }
}

/// Freeze a float's temporary replay style to the used inline size.
///
/// CSS 2.2 resolves a float's used width from its original containing block,
/// then lays out the float's contents in that used box. Spindrift replays the
/// floated element in an isolated temporary containing block, so percentage
/// widths and constraints must not resolve a second time against the replay
/// block:
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width> and
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn freeze_float_replay_width(
    style: &mut ComputedStyle,
    inline_size: ResolvedFloatInlineSize,
) {
    let replay_width = match style.box_sizing {
        BoxSizing::ContentBox => inline_size.content_width.points(),
        BoxSizing::BorderBox => inline_size.border_box_width.points(),
    };
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(replay_width.max(0.0)),
    );
    // The used width already incorporates the original min/max constraints.
    // Replaying those authored constraints would resolve them again in the
    // temporary containing block (and, for `border-box`, in a different box
    // coordinate from the frozen content width).
    // <https://www.w3.org/TR/css-sizing-3/#min-size-auto>
    style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Freeze a floated box's definite used block size before isolated replay.
///
/// A float establishes an independent formatting context, but its percentage
/// block size is resolved against the original containing block, not against
/// the float's own replayed content box.  Replaying an unresolved percentage
/// therefore applies it a second time (for example, `height: 60%` becoming
/// 36% of the page).  Keep the used size at this boundary, alongside the
/// already-frozen used inline size:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property> and
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(in crate::layout) fn freeze_float_replay_height(
    style: &mut ComputedStyle,
    containing_block_height: BlockSizePercentageBasis,
    _preserve_quirks_auto_height: bool,
) -> Option<ContentBoxLength> {
    // `auto` remains auto during float replay. A replay has a definite scratch
    // fragmentainer, but that must not turn an automatic float into a
    // fill-available page-height box merely because the original containing
    // block was definite. Quirks mode affects percentage descendants, not the
    // float's own used `height`.
    // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
    if matches!(
        style.box_values.height.value(),
        css::ComputedLengthPercentageOrAuto::Auto
    ) {
        return None;
    }
    let vertical_non_content = non_content_pt(
        style.padding.top
            + style.padding.bottom
            + used_border_widths(style).top
            + used_border_widths(style).bottom,
    );
    let content_height = used_content_box_size_with_basis(
        style.box_values.height.value().clone(),
        style.box_sizing,
        containing_block_height,
        vertical_non_content,
    )?;
    let replay_height = match style.box_sizing {
        BoxSizing::ContentBox => content_height.points(),
        BoxSizing::BorderBox => {
            content_box_to_border_box_length(content_height, vertical_non_content).points()
        }
    };
    style.box_values.height.replace_with_used(
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(replay_height.max(0.0)),
        ),
    );
    Some(content_height)
}

/// Prepare the principal box style used by both float measurement and final
/// float replay.
///
/// Float placement consumes the outer margins in margin-box coordinates. The
/// independent formatting context therefore starts at the border box with
/// those margins removed. Keeping this transformation shared prevents the
/// speculative used block size from resolving a different formatting context
/// than the final float replay.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(in crate::layout) fn float_replay_style(placed_style: &ComputedStyle) -> ComputedStyle {
    let mut replay_style = placed_style.clone();
    suppress_replayed_item_margins(&mut replay_style);
    replay_style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn float_margin_box_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> MarginBoxLength {
        self.resolved_float_inline_size(
            element,
            style,
            stylesheets,
            containing_width,
            child_boxes,
            table_fragment,
        )
        .margin_box_width
    }

    pub(in crate::layout) fn resolved_float_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ResolvedFloatInlineSize {
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::float_intrinsic_width_scope();
        let collapsed_table =
            style.display.is_table() && style.border_collapse == css::BorderCollapse::Collapse;
        let border_widths = if collapsed_table {
            css::Edges::ZERO
        } else {
            used_border_widths(style)
        };
        let horizontal_padding = if collapsed_table {
            0.0
        } else {
            style.padding.left + style.padding.right
        };
        let horizontal_extras =
            non_content_pt(border_widths.left + border_widths.right + horizontal_padding);
        let built_child_boxes;
        let built_table_fragment;
        let resolved_table_fragment = if style.display.is_table() {
            if let Some(fragment) = table_fragment {
                Some(fragment)
            } else {
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
                let signature = self
                    .ancestors
                    .last()
                    .cloned()
                    .unwrap_or_else(|| element_signature(element));
                built_table_fragment = box_tree::build_frozen_table_fragment(
                    element,
                    &signature,
                    style,
                    table_children,
                );
                Some(&built_table_fragment)
            }
        } else {
            table_fragment
        };
        let available_outer_width =
            (containing_width - style.margin.left - style.margin.right).max(0.0);
        let vertical_non_content = non_content_pt(
            style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom,
        );
        // A float with an explicitly definite height establishes the
        // percentage basis for descendants while its shrink-to-fit inline
        // size is measured. In particular, a replaced descendant's
        // `height: 100%` can transfer through its intrinsic ratio to produce
        // the float's intrinsic width.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let containing_block_height_basis = self
            .block_percentage_context_stack
            .current_percentage_basis();
        let specified_content_height = used_content_box_size_with_basis(
            style.box_values.height.value().clone(),
            style.box_sizing,
            containing_block_height_basis,
            vertical_non_content,
        );
        let intrinsic_height_basis = specified_content_height
            .map(|height| {
                PercentageBasis::definite_from(height, BlockSizeBasisSource::ContainingBlock)
            })
            .or_else(|| {
                // Browsers retain a definite ancestor height through an
                // auto-height float while calculating percentage heights in a
                // quirks document. HTML intentionally leaves much quirks layout
                // behavior undocumented; retain this as a narrow compatibility
                // rule rather than treating it as ordinary CSS percentage sizing.
                // <https://html.spec.whatwg.org/multipage/parsing.html>
                (element.document_compatibility_mode == dom::DocumentCompatibilityMode::Quirks
                    && containing_block_height_basis.is_definite())
                .then_some(containing_block_height_basis)
            });
        let measure_intrinsic_widths = |layout: &mut Self, available_width: f32| {
            if let Some(basis) = intrinsic_height_basis {
                layout
                    .block_percentage_context_stack
                    .push_percentage_basis(basis);
            }
            let sizes = if style.writing_mode.has_vertical_lines()
                && !style.display.is_flex()
                && !style.display.is_table()
                && let Some(content_height) = specified_content_height
            {
                let logical_size = FloatLogicalContentSize {
                    inline: LogicalInlineContentSize::new(content_height),
                    block: layout.block_logical_block_size_at_inline_size(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        LogicalInlineContentSize::new(content_height),
                        available_width,
                    ),
                };
                let physical_width = logical_size.physical_width(FlowAxes::for_style(style));
                (physical_width.points(), physical_width.points())
            } else {
                layout.formatting_context_intrinsic_widths(
                    element,
                    style,
                    stylesheets,
                    available_width,
                    child_boxes,
                    resolved_table_fragment,
                )
            };
            if intrinsic_height_basis.is_some() {
                layout.block_percentage_context_stack.pop();
            }
            sizes
        };
        let specified_content_width = used_content_box_size(
            style.box_values.width.clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(available_outer_width)),
            horizontal_extras,
        );
        let intrinsic_widths = ((style.display.is_table()
            || specified_content_width.is_none()
                && needs_intrinsic_width_contribution(style.box_values.width.clone()))
            || needs_intrinsic_width_contribution(style.box_values.min_width.clone())
            || needs_intrinsic_width_contribution(style.box_values.max_width.clone()))
        .then(|| {
            let content_available_width =
                (available_outer_width - horizontal_extras.points()).max(0.0);
            let (preferred_min, preferred) =
                measure_intrinsic_widths(self, content_available_width);
            let preferred = if style.display.is_flex()
                && style.box_values.width.is_auto()
                && style.flex_wrap != FlexWrap::NoWrap
                && style.flex_direction.is_column_axis()
                && content_available_width > preferred + 0.01
            {
                // CSS 2.2 floats apply shrink-to-fit after the flex container
                // intrinsic pass. WPT's column-wrap float case treats an
                // unconstrained available width as min-content, while still
                // letting narrower containing blocks clamp up to wrapped
                // max-content:
                // https://www.w3.org/TR/CSS22/visudet.html#float-width
                // https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes
                preferred_min
            } else {
                preferred
            };
            (preferred_min, preferred)
        });
        let content_width = specified_content_width.unwrap_or_else(|| {
            if let Some((preferred_min, preferred)) = intrinsic_widths {
                intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_outer_width),
                    horizontal_extras,
                    content_box_pt(preferred_min),
                    content_box_pt(preferred),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
            } else {
                let content_available_width =
                    (available_outer_width - horizontal_extras.points()).max(0.0);
                let (preferred_min, preferred) =
                    measure_intrinsic_widths(self, content_available_width);
                let preferred = if style.display.is_flex()
                    && style.box_values.width.is_auto()
                    && style.flex_wrap != FlexWrap::NoWrap
                    && style.flex_direction.is_column_axis()
                    && content_available_width > preferred + 0.01
                {
                    preferred_min
                } else {
                    preferred
                };
                intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_outer_width),
                    horizontal_extras,
                    content_box_pt(preferred_min),
                    content_box_pt(preferred),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
            }
        });
        let content_width = if let Some((preferred_min, preferred)) = intrinsic_widths {
            // CSS table width is a preferred used size rather than permission
            // to make the table grid narrower than its intrinsic minimum. A
            // floated table must expose that minimum before float placement:
            // otherwise its wrapper can fit beside an earlier float while the
            // grid itself overflows through that float's exclusion band.
            // <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>
            // <https://www.w3.org/TR/CSS22/visudet.html#float-width>
            let content_width = if style.display.is_table() {
                content_box_pt(content_width.points().max(preferred_min))
            } else {
                content_width
            };
            constrain_width_with_intrinsic(
                style,
                content_width,
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                PercentageBasis::definite(content_box_pt(available_outer_width)),
                horizontal_extras,
            )
        } else {
            constrain_content_width(
                style,
                content_width,
                PercentageBasis::definite(layout_pt(available_outer_width)),
            )
        };
        let visual_horizontal_extras = if collapsed_table {
            self.collapsed_table_outer_horizontal_insets(
                style,
                stylesheets,
                resolved_table_fragment,
            )
            .map(NonContentLength::points)
            .unwrap_or(0.0)
        } else {
            0.0
        };
        resolved_float_inline_size_from_content_box(
            style,
            content_width,
            horizontal_extras,
            visual_horizontal_extras,
        )
    }

    /// Return the floated margin-box block size used for placement.
    ///
    /// CSS 2.2 makes auto-height floating non-replaced elements use the same
    /// descendant-based height calculation as BFC roots. Empty floats therefore
    /// have zero content height instead of reserving a synthetic line box:
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout) fn float_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inline_size: ResolvedFloatInlineSize,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> MarginBoxLength {
        let built_child_boxes;
        let child_boxes = if child_boxes.is_some() || is_replaced_element(element) {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_child_boxes.as_slice())
        };
        // A flex float's automatic block size is its used flex formatting
        // context height at the already-resolved float width.  It cannot use
        // the generic flow-root cursor probe: that probe is intentionally
        // formatting-context agnostic, while Flexbox owns its final cross-size
        // resolution and line geometry.
        // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        if style.display.is_flex() {
            return self.measure_floated_flex_margin_box_height(
                element,
                style,
                stylesheets,
                PhysicalContentWidth::new(inline_size.content_width),
                child_boxes,
            );
        }
        // A definite used height needs no speculative formatting pass. Keep
        // the established estimator for non-flex formatting contexts, which is
        // also used by intrinsic float queries.
        if !has_auto_height(style) {
            return margin_box_pt(
                self.estimate_element_height(
                    element,
                    style,
                    stylesheets,
                    inline_size.margin_box_width.points(),
                    child_boxes,
                )
                .unwrap_or(0.0),
            );
        }

        // Grid's used automatic block size is established by final track
        // sizing at the already-resolved float width. Query that pure sizing
        // boundary directly, so the speculative float-height probe does not
        // lay out and discard every Grid child before final paint replay.
        // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
        // <https://www.w3.org/TR/css-grid-1/#grid-container-size>
        if style.display.is_grid()
            && let Some(height) = self.measure_floated_grid_margin_box_height(
                element,
                style,
                stylesheets,
                PhysicalContentWidth::new(inline_size.content_width),
                child_boxes,
            )
        {
            return height;
        }

        self.measure_auto_float_margin_box_height(
            element,
            style,
            stylesheets,
            inline_size,
            child_boxes,
            table_fragment,
        )
    }

    /// Measure an auto-height float in the same isolated flow-root replay
    /// used to paint it later.
    ///
    /// A generic descendant height estimate cannot model every inline and
    /// generated-content interaction. In particular, it can retain earlier
    /// float exclusions while recursively estimating a later floated list.
    /// Replay instead uses the frozen used width and a fresh float context,
    /// then runs in an ownership-isolated speculative transaction.
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    fn measure_auto_float_margin_box_height(
        &mut self,
        element: &Element,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inline_size: ResolvedFloatInlineSize,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> MarginBoxLength {
        // Isolated measurement lays out this same DOM element again. Complex
        // formatting contexts (notably a float below nested balanced columns)
        // can ask for the outer float's height while that replay is still in
        // progress. Layout snapshots intentionally roll back the replay, so a
        // snapshot-owned flag would lose the active cycle marker. Keep the
        // marker on the builder instead and use the ordinary finite estimator
        // for the recursive edge.
        //
        // CSS 2.2 requires an auto-height float to use its BFC's used height;
        // the fallback does not change the final replay path. It only provides
        // a terminating provisional size while that same BFC is being solved.
        // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
        let element_key = element.id;
        let cache_key = AutoFloatMeasurementKey {
            element: element.id,
            content_width_bits: inline_size.content_width.points().to_bits(),
            border_box_width_bits: inline_size.border_box_width.points().to_bits(),
            margin_box_width_bits: inline_size.margin_box_width.points().to_bits(),
            style_fingerprint: debug_fingerprint(placed_style),
            percentage_basis_fingerprint: debug_fingerprint(
                &self.block_percentage_context_stack.current_context(),
            ),
            counter_fingerprint: debug_fingerprint(&self.counter_set),
            quote_depth: self.quote_depth,
            page_index: self.pages.len(),
        };
        if let Some(height) = self
            .speculative_auto_float_margin_box_heights
            .get(&cache_key)
        {
            #[cfg(feature = "layout-profile")]
            crate::layout::layout_profile::record_float_auto_height_cache_hit();
            return *height;
        }
        #[cfg(feature = "layout-profile")]
        crate::layout::layout_profile::record_float_auto_height_cache_miss();

        if !self.active_auto_float_measurements.is_empty() {
            let height = self.nested_auto_float_margin_box_height(
                element,
                placed_style,
                stylesheets,
                inline_size,
                child_boxes,
            );
            if self.speculative_auto_float_margin_box_heights.len() < 256 {
                self.speculative_auto_float_margin_box_heights
                    .insert(cache_key, height);
            }
            return height;
        }
        #[cfg(feature = "layout-profile")]
        let _profile = crate::layout::layout_profile::float_auto_height_measurement_scope();
        self.active_auto_float_measurements.push(element_key);
        let probe_top = 10_000.0;
        let replay_style = float_replay_style(placed_style);
        let border_box_height = self.with_speculative_layout(|layout| {
            // The probe is a page-shaped, non-persistent formatting context. Its
            // fragmentainer is made effectively unbounded so it measures the
            // float's used block size rather than a page fragment.
            layout.current_page =
                Page::new(inline_size.border_box_width.points().max(1.0), probe_top);
            layout.content_left = placed_style.margin.left;
            layout.content_right =
                layout.content_left + inline_size.border_box_width.points().max(1.0);
            layout.cursor_y = probe_top - placed_style.margin.top;
            // The floated element is itself an orthogonal child of this
            // formatting context. Keep the parent flow here: block layout records
            // it before installing `placed_style` for descendants, and uses that
            // relationship to resolve the float's auto logical inline size.
            // Replacing it with the float's own flow makes a vertical float look
            // parallel to itself and leaves its physical height at zero.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            if !crate::layout::block::writing_modes_are_orthogonal(
                layout.containing_block_writing_mode,
                placed_style.writing_mode,
            ) {
                layout.containing_block_direction = placed_style.used_direction();
                layout.containing_block_writing_mode = placed_style.writing_mode;
            }
            layout.fragmentation_suppression_depth += 1;
            layout.push_page_name_scope_suppression();
            layout.push_float_context();
            layout.preserve_scoped_paint_public_order = true;
            layout.layout_element_with_child_boxes_and_table_fragment(
                element,
                &replay_style,
                stylesheets,
                child_boxes,
                table_fragment,
            );
            let border_box_height =
                (probe_top - placed_style.margin.top - layout.cursor_y).max(0.0);
            layout.pop_float_context();
            layout.pop_page_name_scope_suppression();
            layout.fragmentation_suppression_depth -= 1;
            border_box_height
        });
        let popped = self.active_auto_float_measurements.pop();
        debug_assert_eq!(popped, Some(element_key));

        let height =
            margin_box_pt(placed_style.margin.top + border_box_height + placed_style.margin.bottom);
        // The cache is bounded because a pathological document can otherwise
        // manufacture a distinct generated-content state for every replay.
        // Entries are only an optimization; omitting a new one leaves layout
        // semantics unchanged.
        if self.speculative_auto_float_margin_box_heights.len() < 256 {
            self.speculative_auto_float_margin_box_heights
                .insert(cache_key, height);
        }
        height
    }

    /// Return a finite provisional height for a float nested inside an
    /// in-progress isolated auto-height measurement.
    ///
    /// An exact isolated replay is needed at the top of the measurement tree,
    /// where inline and generated-content interactions determine the BFC's
    /// used height. Launching the same replay recursively for each floated
    /// descendant, however, makes nested float trees exponential. The final
    /// layout still replays each descendant exactly for placement and paint;
    /// this provisional edge only supplies the ancestor's finite BFC extent.
    fn nested_auto_float_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inline_size: ResolvedFloatInlineSize,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> MarginBoxLength {
        let element_key = element.id;
        let bare_height = self.minimum_auto_float_margin_box_height(style);
        if self
            .active_auto_float_measurement_fallbacks
            .contains(&element_key)
        {
            return bare_height;
        }

        self.active_auto_float_measurement_fallbacks
            .push(element_key);
        let estimated_height = self
            .estimate_element_height(
                element,
                style,
                stylesheets,
                inline_size.margin_box_width.points(),
                child_boxes,
            )
            .map(margin_box_pt)
            .unwrap_or(bare_height);
        let popped = self.active_auto_float_measurement_fallbacks.pop();
        debug_assert_eq!(popped, Some(element_key));
        estimated_height.max(bare_height)
    }

    /// The innermost leg of a recursive float measurement has no safe
    /// descendant traversal left. Preserve the float's authored minimum block
    /// size and box metrics, while treating its unresolved auto content as
    /// empty as CSS 2.2 does for an empty float.
    fn minimum_auto_float_margin_box_height(&self, style: &ComputedStyle) -> MarginBoxLength {
        let borders = used_border_widths(style);
        let vertical_non_content =
            non_content_pt(style.padding.top + style.padding.bottom + borders.top + borders.bottom);
        let content_height = constrain_content_height(
            style,
            content_box_pt(0.0),
            self.block_percentage_context_stack
                .current_percentage_basis(),
        );
        margin_box_pt(
            style.margin.top
                + vertical_non_content.points()
                + content_height.points()
                + style.margin.bottom,
        )
    }
}

fn resolved_float_inline_size_from_content_box(
    style: &ComputedStyle,
    content_width: ContentBoxLength,
    horizontal_extras: NonContentLength,
    visual_horizontal_extras: f32,
) -> ResolvedFloatInlineSize {
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_extras);
    ResolvedFloatInlineSize {
        content_width,
        border_box_width,
        margin_box_width: margin_box_pt(
            style.margin.left
                + border_box_width.points()
                + visual_horizontal_extras
                + style.margin.right,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn length_points(value: css::ComputedLengthPercentageOrAuto) -> f32 {
        match value {
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.length_points(),
            _ => panic!("expected resolved length"),
        }
    }

    #[test]
    fn float_inline_size_expands_content_box_to_border_and_margin_boxes() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = length(150.0);
        style.margin.left = 5.0;
        style.margin.right = 7.0;

        let inline_size = resolved_float_inline_size_from_content_box(
            &style,
            content_box_pt(150.0),
            non_content_pt(20.0),
            0.0,
        );

        assert_eq!(inline_size.content_width.points(), 150.0);
        assert_eq!(inline_size.border_box_width.points(), 170.0);
        assert_eq!(inline_size.margin_box_width, margin_box_pt(182.0));
    }

    #[test]
    fn border_box_float_width_clamps_content_and_keeps_extras_once() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = length(100.0);
        let extras = non_content_pt(150.0);
        let content_width = used_content_box_size(
            style.box_values.width.clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(300.0)),
            extras,
        )
        .unwrap();

        let inline_size =
            resolved_float_inline_size_from_content_box(&style, content_width, extras, 0.0);

        assert_eq!(inline_size.content_width.points(), 0.0);
        assert_eq!(inline_size.border_box_width.points(), 150.0);
        assert_eq!(inline_size.margin_box_width, margin_box_pt(150.0));
    }

    #[test]
    fn auto_float_shrink_to_fit_returns_content_box_length() {
        let style = ComputedStyle::initial();
        let width = intrinsic::content_box_width_from_intrinsic(
            &style,
            layout_pt(150.0),
            non_content_pt(20.0),
            content_box_pt(80.0),
            content_box_pt(200.0),
            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );

        let _typed: ContentBoxLength = width;
        assert_eq!(width.points(), 130.0);
    }

    #[test]
    fn freeze_float_replay_width_writes_box_sizing_specific_used_width() {
        let inline_size = ResolvedFloatInlineSize {
            content_width: content_box_pt(80.0),
            border_box_width: border_box_pt(120.0),
            margin_box_width: margin_box_pt(120.0),
        };
        let mut content_box_style = ComputedStyle::initial();
        content_box_style.box_sizing = BoxSizing::ContentBox;
        freeze_float_replay_width(&mut content_box_style, inline_size);

        let mut border_box_style = ComputedStyle::initial();
        border_box_style.box_sizing = BoxSizing::BorderBox;
        freeze_float_replay_width(&mut border_box_style, inline_size);

        assert_eq!(length_points(content_box_style.box_values.width), 80.0);
        assert!(content_box_style.box_values.min_width.is_auto());
        assert!(content_box_style.box_values.max_width.is_auto());
        assert_eq!(length_points(border_box_style.box_values.width), 120.0);
        assert!(border_box_style.box_values.min_width.is_auto());
        assert!(border_box_style.box_values.max_width.is_auto());
    }
}
